//! Regions: tables that only headers point at, and that can therefore move.

use crate::elf::consts::*;
use crate::elf::hash;
use crate::error::Result;
use crate::image::Image;

/// Where a region's bytes come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Content {
    Interp,
    DynStr,
    Dynamic,
    /// Bytes copied unchanged from the file.
    Raw,
}

/// Something that records a region's position and must follow it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Ref {
    Phdr(usize),
    Tag(u64),
    Section(usize),
}

/// What happens to a region in the output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Fate {
    /// Left as it is.
    Keep,
    /// Rewritten in its old place (it still fits).
    Rewrite,
    /// Moved into the new segment.
    Move,
}

/// A contiguous file range holding one or more movable tables.
#[derive(Debug)]
pub(super) struct Region {
    pub(super) content: Content,
    pub(super) offset: u64,
    pub(super) addr: u64,
    pub(super) size: u64,
    pub(super) align: u64,
    pub(super) refs: Vec<Ref>,
    /// False if something else overlaps it or no single segment maps it.
    pub(super) movable: bool,
    pub(super) fate: Fate,
}

impl Region {
    pub(super) fn end(&self) -> u64 {
        self.offset.saturating_add(self.size)
    }
    pub(super) fn contains(&self, off: u64, len: u64) -> bool {
        off >= self.offset && off.saturating_add(len) <= self.end()
    }
}

/// Find the movable regions of `img`.
pub(super) fn collect_regions(img: &Image) -> Vec<Region> {
    let ctx = img.ctx;
    let word = ctx.word() as u64;
    let mut regions: Vec<Region> = Vec::new();
    let mut add = |content, offset, addr, size, align, refs| {
        regions.push(Region { content, offset, addr, size, align, refs, movable: true, fate: Fate::Keep })
    };
    let phdr_index = |t: u32| img.phdrs.iter().position(|p| p.p_type == t);

    if let (Some(i), Some(pi)) = (&img.interp, phdr_index(PT_INTERP)) {
        add(Content::Interp, i.loc.offset, i.loc.addr, i.loc.size, 1, vec![Ref::Phdr(pi)]);
    }
    for (pi, p) in img.phdrs.iter().enumerate().filter(|(_, p)| p.p_type == PT_MIPS_ABIFLAGS && p.filesz > 0) {
        if img.ehdr.machine == crate::elf::consts::EM_MIPS {
            add(Content::Raw, p.offset, p.vaddr, p.filesz, 8, vec![Ref::Phdr(pi)]);
        }
    }
    for (pi, p) in img.phdrs.iter().enumerate().filter(|(_, p)| p.p_type == PT_NOTE && p.filesz > 0) {
        let mut refs = vec![Ref::Phdr(pi)];
        for (qi, q) in img.phdrs.iter().enumerate() {
            if q.p_type == PT_GNU_PROPERTY && q.filesz > 0 && p.contains_offset(q.offset, q.filesz) {
                refs.push(Ref::Phdr(qi));
            }
        }
        add(Content::Raw, p.offset, p.vaddr, p.filesz, p.align.clamp(4, 8), refs);
    }
    if let Some(s) = &img.dynstr {
        add(Content::DynStr, s.loc.offset, s.loc.addr, s.loc.size, 1, vec![Ref::Tag(DT_STRTAB)]);
    }
    if let (Some(d), Some(pi)) = (&img.dynamic, phdr_index(PT_DYNAMIC)) {
        add(Content::Dynamic, d.loc.offset, d.loc.addr, d.loc.size, word, vec![Ref::Phdr(pi)]);
    }
    for (tag, addr, size, align) in tag_tables(img) {
        if let Ok(off) = img.map_addr(addr, size) {
            add(Content::Raw, off, addr, size, align, vec![Ref::Tag(tag)]);
        }
    }

    // Section headers follow the region they lie in.
    for (si, s) in img.shdrs.iter().enumerate() {
        if s.flags & SHF_ALLOC == 0 || !s.has_file_data() || s.size == 0 {
            continue;
        }
        let end = s.offset.saturating_add(s.size);
        for r in regions.iter_mut() {
            let overlaps = s.offset < r.end() && r.offset < end;
            if !overlaps {
                continue;
            }
            let fits = if r.content == Content::Raw { r.contains(s.offset, s.size) } else { s.offset == r.offset };
            if fits && s.addr.wrapping_sub(r.addr) == s.offset - r.offset {
                r.refs.push(Ref::Section(si));
                r.align = r.align.max(s.addralign.min(64));
            } else {
                r.movable = false;
            }
        }
    }

    // Overlapping raw regions move together; anything overlapping a table we rewrite stays put.
    regions.sort_by_key(|r| (r.offset, std::cmp::Reverse(r.size)));
    let mut merged: Vec<Region> = Vec::new();
    for r in regions {
        if let Some(last) = merged.last_mut() {
            if r.offset < last.end() {
                if last.content == Content::Raw
                    && r.content == Content::Raw
                    && r.addr.wrapping_sub(last.addr) == r.offset - last.offset
                {
                    let end = last.end().max(r.end());
                    last.size = end - last.offset;
                    last.align = last.align.max(r.align);
                    last.movable &= r.movable;
                    for rf in r.refs {
                        if !last.refs.contains(&rf) {
                            last.refs.push(rf);
                        }
                    }
                    continue;
                }
                last.movable = false;
                let mut r = r;
                r.movable = false;
                merged.push(r);
                continue;
            }
        }
        merged.push(r);
    }

    // A region can only move if one segment maps all of it.
    for r in merged.iter_mut() {
        r.movable &= img.phdrs.iter().any(|p| {
            p.p_type == PT_LOAD && p.contains_offset(r.offset, r.size) && p.vaddr + (r.offset - p.offset) == r.addr
        });
    }
    merged
}

/// `(tag, address, size, alignment)` of each table the dynamic section points at.
pub(super) fn tag_tables(img: &Image) -> Vec<(u64, u64, u64, u64)> {
    let mut out = Vec::new();
    let Some(d) = &img.dynamic else { return out };
    let ctx = img.ctx;
    let word = ctx.word() as u64;
    let data = &img.data;

    if let Some(addr) = d.get(DT_GNU_HASH) {
        let size = (|| -> Result<u64> {
            let off = img.map_addr(addr, 16)?;
            let h = hash::GnuHeader::read(ctx, data, off)?;
            let count = hash::gnu_symbol_count(ctx, data, off)?;
            Ok(16
                + u64::from(h.bloom_size) * word
                + 4 * u64::from(h.nbuckets)
                + 4 * count.saturating_sub(u64::from(h.symoffset)))
        })();
        if let Ok(size) = size {
            out.push((DT_GNU_HASH, addr, size, word));
        }
    }
    if let Some(addr) = d.get(DT_HASH) {
        let ent = hash::sysv_entry_size(ctx, img.ehdr.machine);
        if let Ok((nb, nc)) = img.map_addr(addr, 2 * ent).and_then(|off| hash::sysv_header(ctx, data, off, ent)) {
            out.push((DT_HASH, addr, (2 + nb + nc) * ent, ent));
        }
    }
    if let Ok(Some(t)) = img.dynsym() {
        out.push((DT_SYMTAB, d.get(DT_SYMTAB).unwrap(), t.count * t.entsize, word));
        if let Some(addr) = d.get(DT_VERSYM) {
            out.push((DT_VERSYM, addr, 2 * t.count, 2));
        }
    }
    for (tag, num, entry, cnt_at, aux_at, next_at, aux_size, aux_next_at) in [
        (DT_VERNEED, DT_VERNEEDNUM, 16u64, 2u64, 8u64, 12u64, 16u64, 12u64),
        (DT_VERDEF, DT_VERDEFNUM, 20, 6, 12, 16, 8, 4),
    ] {
        let (Some(addr), Some(n)) = (d.get(tag), d.get(num)) else { continue };
        let size = (|| -> Result<u64> {
            let base = img.map_addr(addr, entry)?;
            let (mut off, mut end) = (base, base);
            for _ in 0..n {
                end = end.max(off + entry);
                let cnt = ctx.u16(data, off + cnt_at)?;
                let mut aux = off + u64::from(ctx.u32(data, off + aux_at)?);
                for _ in 0..cnt {
                    end = end.max(aux + aux_size);
                    let next = ctx.u32(data, aux + aux_next_at)?;
                    if next == 0 {
                        break;
                    }
                    aux += u64::from(next);
                }
                let next = ctx.u32(data, off + next_at)?;
                if next == 0 {
                    break;
                }
                off += u64::from(next);
            }
            Ok(end - base)
        })();
        if let Ok(size) = size {
            out.push((tag, addr, size, word));
        }
    }
    for (tag, size_tag) in [(DT_RELA, DT_RELASZ), (DT_REL, DT_RELSZ), (DT_JMPREL, DT_PLTRELSZ), (DT_RELR, DT_RELRSZ)] {
        if let (Some(addr), Some(size)) = (d.get(tag), d.get(size_tag)) {
            if size > 0 {
                out.push((tag, addr, size, word));
            }
        }
    }
    out
}
