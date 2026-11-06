//! Turn an edited [`Image`] back into bytes.
//!
//! The layout works on *regions*: tables that the loader finds only through
//! headers, never through code, and that can therefore move. They are
//! `PT_INTERP`, notes, `.dynamic`, and the tables the dynamic section points
//! at (`.dynstr`, `.dynsym`, hash and version tables, relocation tables).
//! Code and data never move.
//!
//! 1. A region whose contents grew beyond its old size moves into one new
//!    `PT_LOAD` segment placed after everything else. If a previous run
//!    already appended such a segment at the end of the file, it is rebuilt
//!    instead of adding another one.
//! 2. The program header table stays where it is, because tools such as GNU
//!    `strip` expect it right after the ELF header. If it needs more slots,
//!    the regions right after it are moved out of the way. Only if that is
//!    impossible does the table itself move into the new segment.
//! 3. For executables, the new segment is placed (when that costs little
//!    padding) where loaders that compute the program header address as
//!    `first segment base + e_phoff` will look.
//! 4. Old copies of moved regions are zeroed, so stale strings such as store
//!    paths don't stay in the file, unless `--no-clobber-old-sections`.

mod headers;
mod placement;
mod regions;

use crate::elf::codec::{align_up, slice, slice_mut, Writer};
use crate::elf::consts::*;
use crate::elf::types::{Phdr, Shdr};
use crate::error::Result;
use crate::image::Image;
use crate::{debug, ensure};
use headers::{encode_section_counts, move_symbols};
use placement::{evacuation, place_segment, reusable_tail, segment_alignment};
use regions::{collect_regions, Content, Fate, Ref, Region};

pub struct Config {
    /// Alignment for the new segment (`--page-size`).
    pub page_size: Option<u64>,
    /// Zero old copies of moved regions.
    pub clobber_old: bool,
}

pub fn emit(mut img: Image, cfg: &Config) -> Result<Vec<u8>> {
    img.scrub_released_strings()?;
    let ctx = img.ctx;
    let phent = ctx.phdr_size() as u64;
    let phoff = img.ehdr.phoff;
    let orig_phnum = img.orig_phdrs.len() as u64;
    // Unused (`PT_NULL`) slots are given up only if the table needs room.
    let unused = img.phdrs.iter().filter(|p| p.p_type == PT_NULL).count() as u64;
    let mut give_up;
    let mut regions = collect_regions(&img);

    // ---- regions whose contents changed ----
    for r in regions.iter_mut() {
        let (dirty, size) = match r.content {
            Content::Interp => {
                let i = img.interp.as_ref().unwrap();
                (i.dirty, i.value.len() as u64 + 1)
            }
            Content::DynStr => {
                let s = img.dynstr.as_ref().unwrap();
                (s.dirty, s.bytes.len() as u64)
            }
            Content::Dynamic => {
                let d = img.dynamic.as_ref().unwrap();
                (d.dirty, (d.entries.len() as u64 + 1) * ctx.dyn_size() as u64)
            }
            Content::Raw => (false, 0),
        };
        if dirty {
            r.fate = if size <= r.size { Fate::Rewrite } else { Fate::Move };
            ensure!(r.fate != Fate::Move || r.movable, "cannot grow {:?}: it overlaps other data", r.content);
        }
    }

    // ---- decide what moves, and where the program headers go ----
    let mut pht_moves = false;
    let mut reuse: Option<usize> = None;
    let mut reuse_checked = false;
    let final_phnum = loop {
        let need_segment = pht_moves || !img.new_notes.is_empty() || regions.iter().any(|r| r.fate == Fate::Move);
        if need_segment && !reuse_checked {
            reuse_checked = true;
            if let Some(li) = reusable_tail(&img, &regions) {
                let l = img.phdrs[li].clone();
                debug!("rebuilding the segment at file offset {:#x} instead of adding one", l.offset);
                for r in regions.iter_mut().filter(|r| l.contains_offset(r.offset, r.size)) {
                    r.fate = Fate::Move;
                }
                pht_moves |= l.contains_offset(phoff, orig_phnum * phent);
                reuse = Some(li);
            }
        }
        let wanted = img.phdrs.len() as u64 + img.new_notes.len() as u64 + u64::from(need_segment && reuse.is_none());
        give_up = wanted.saturating_sub(orig_phnum).min(unused);
        let n = wanted - give_up;
        if n <= orig_phnum || pht_moves {
            break n;
        }
        match evacuation(&img, &regions, n) {
            Some(idx) if idx.iter().all(|&i| regions[i].fate == Fate::Move) => break n,
            Some(idx) => {
                for i in idx {
                    debug!(
                        "moving {:?} at {:#x} to make room for program headers",
                        regions[i].content, regions[i].offset
                    );
                    regions[i].fate = Fate::Move;
                }
            }
            None => {
                debug!("no room to grow the program header table in place; moving it");
                pht_moves = true;
            }
        }
    };
    let need_segment = pht_moves || !img.new_notes.is_empty() || regions.iter().any(|r| r.fate == Fate::Move);

    let moved_size = |img: &Image, r: &Region| -> u64 {
        match r.content {
            Content::Interp => img.interp.as_ref().unwrap().value.len() as u64 + 1,
            Content::DynStr => img.dynstr.as_ref().unwrap().bytes.len() as u64,
            Content::Dynamic => (img.dynamic.as_ref().unwrap().entries.len() as u64 + 1) * ctx.dyn_size() as u64,
            Content::Raw => r.size,
        }
    };

    // ---- place the new segment ----
    let sht_size = img.orig_shnum as u64 * ctx.shdr_size() as u64;
    let sht_is_last = img.orig_shnum > 0 && img.ehdr.shoff + sht_size == img.data.len() as u64;
    let truncate_at = match reuse {
        Some(li) => img.phdrs[li].offset,
        // The section headers are rewritten after the new segment anyway.
        None if need_segment && sht_is_last => img.ehdr.shoff,
        None => img.data.len() as u64,
    };
    // (what, file offset, address, size)
    let mut placed: Vec<(Item, u64, u64, u64)> = Vec::new();
    let mut segment: Option<Phdr> = None;
    if need_segment {
        let mut items: Vec<(Item, u64, u64, Option<u64>)> = Vec::new(); // (item, size, align, congruent to)
        if pht_moves {
            items.push((Item::Phdrs, final_phnum * phent, ctx.word() as u64, None));
        }
        let mut order: Vec<usize> = (0..regions.len()).filter(|&i| regions[i].fate == Fate::Move).collect();
        order.sort_by_key(|&i| regions[i].offset);
        for i in order {
            let r = &regions[i];
            let congruent = if r.content == Content::Raw { Some(r.offset) } else { None };
            items.push((Item::Region(i), moved_size(&img, r), r.align, congruent));
        }
        for (i, n) in img.new_notes.iter().enumerate() {
            items.push((Item::Note(i), n.bytes.len() as u64, 4, None));
        }
        let max_align = items.iter().map(|it| it.2).max().unwrap_or(1).min(4096);
        let upper_bound: u64 = items.iter().map(|it| it.1 + it.2).sum();
        let align = segment_alignment(&img, cfg);
        let (seg_off, seg_vaddr) = match reuse {
            Some(li) => (img.phdrs[li].offset, img.phdrs[li].vaddr),
            None => place_segment(&img, truncate_at, upper_bound, align, max_align)?,
        };
        let mut cursor = seg_off;
        for (item, size, a, congruent) in items {
            let a = a.max(1);
            cursor = match congruent {
                Some(old) => cursor + (old.wrapping_sub(cursor) % a),
                None => align_up(cursor, a),
            };
            placed.push((item, cursor, seg_vaddr + (cursor - seg_off), size));
            cursor += size;
        }
        let size = cursor - seg_off;
        let limit = if ctx.is64 { u64::MAX } else { u64::from(u32::MAX) };
        ensure!(
            seg_vaddr.checked_add(size).is_some_and(|end| end <= limit),
            "no room for a new segment in the address space"
        );
        debug!("new segment: offset {seg_off:#x}, address {seg_vaddr:#x}, size {size:#x}");
        let writable = placed.iter().any(|p| matches!(p.0, Item::Region(i) if regions[i].content == Content::Dynamic));
        segment = Some(Phdr {
            p_type: PT_LOAD,
            flags: PF_R | if writable { PF_W } else { 0 },
            offset: seg_off,
            vaddr: seg_vaddr,
            paddr: seg_vaddr,
            filesz: size,
            memsz: size,
            align,
        });
    }
    let place_of = |item: Item| placed.iter().find(|p| p.0 == item).map(|p| (p.1, p.2, p.3));

    // Symbols defined in a moved section move with it. This edits the symbol
    // tables in place, so it happens before moved tables are copied.
    let mut section_moves = Vec::new();
    for (i, r) in regions.iter().enumerate() {
        if let Some((_, addr, _)) = place_of(Item::Region(i)) {
            for rf in &r.refs {
                if let Ref::Section(si) = *rf {
                    section_moves.push((si, addr.wrapping_sub(r.addr)));
                }
            }
        }
    }
    move_symbols(&mut img, &section_moves)?;

    // ---- snapshot the bytes of regions that move unchanged ----
    let mut raw_bytes: Vec<Option<Vec<u8>>> = Vec::new();
    for r in &regions {
        raw_bytes.push(if r.fate == Fate::Move && r.content == Content::Raw {
            Some(slice(&img.data, r.offset, r.size)?.to_vec())
        } else {
            None
        });
    }

    // ---- make every reference follow its region ----
    let mut strsz_changed = false;
    for (i, r) in regions.iter().enumerate() {
        let new_size = match r.fate {
            Fate::Keep => continue,
            Fate::Rewrite => match r.content {
                Content::Dynamic => continue,
                _ => moved_size(&img, r),
            },
            Fate::Move => moved_size(&img, r),
        };
        let (off, addr) = match place_of(Item::Region(i)) {
            Some((off, addr, _)) => (off, addr),
            None => (r.offset, r.addr),
        };
        let resized = r.content != Content::Raw;
        let (doff, daddr) = (off.wrapping_sub(r.offset), addr.wrapping_sub(r.addr));
        for rf in &r.refs {
            match *rf {
                Ref::Phdr(pi) => {
                    let p = &mut img.phdrs[pi];
                    p.offset = p.offset.wrapping_add(doff);
                    p.vaddr = p.vaddr.wrapping_add(daddr);
                    p.paddr = p.paddr.wrapping_add(daddr);
                    if resized {
                        (p.filesz, p.memsz) = (new_size, new_size);
                    }
                }
                Ref::Tag(tag) => {
                    if daddr != 0 {
                        let d = img.dynamic.as_mut().unwrap();
                        for e in d.entries.iter_mut().filter(|e| e.tag == tag) {
                            e.val = e.val.wrapping_add(daddr);
                        }
                        d.dirty = true;
                    }
                }
                Ref::Section(si) => {
                    let s = &mut img.shdrs[si];
                    s.offset = s.offset.wrapping_add(doff);
                    s.addr = s.addr.wrapping_add(daddr);
                    if resized {
                        s.size = new_size;
                    }
                }
            }
        }
        if r.content == Content::DynStr && new_size != r.size {
            strsz_changed = true;
        }
    }
    if strsz_changed {
        let len = img.dynstr.as_ref().unwrap().bytes.len() as u64;
        let d = img.dynamic.as_mut().unwrap();
        for e in d.entries.iter_mut().filter(|e| e.tag == DT_STRSZ) {
            e.val = len;
        }
        d.dirty = true;
    }

    // ---- new program headers and sections ----
    if let Some(load) = segment.clone() {
        match reuse {
            Some(li) => img.phdrs[li] = load,
            None => {
                drop_unused(&mut img.phdrs, &mut give_up);
                let pos = img
                    .phdrs
                    .iter()
                    .rposition(|p| p.p_type == PT_LOAD && p.vaddr < load.vaddr)
                    .map(|i| i + 1)
                    .or_else(|| img.phdrs.iter().position(|p| p.p_type == PT_LOAD))
                    .unwrap_or(img.phdrs.len());
                img.phdrs.insert(pos, load);
            }
        }
    }
    let mut shstrtab_extra: Vec<u8> = Vec::new();
    let shstrtab_size = img.shdrs.get(img.shstrndx).map_or(0, |s| s.size);
    drop_unused(&mut img.phdrs, &mut give_up);
    for i in 0..img.new_notes.len() {
        let (off, addr, size) = place_of(Item::Note(i)).unwrap();
        img.phdrs.push(Phdr {
            p_type: PT_NOTE,
            flags: PF_R,
            offset: off,
            vaddr: addr,
            paddr: addr,
            filesz: size,
            memsz: size,
            align: 4,
        });
        if !img.shdrs.is_empty() {
            let name = shstrtab_size + shstrtab_extra.len() as u64;
            shstrtab_extra.extend_from_slice(img.new_notes[i].section_name);
            shstrtab_extra.push(0);
            img.shdrs.push(Shdr {
                name: name as u32,
                sh_type: SHT_NOTE,
                flags: SHF_ALLOC,
                addr,
                offset: off,
                size,
                link: 0,
                info: 0,
                addralign: 4,
                entsize: 0,
            });
        }
    }
    ensure!(
        img.phdrs.len() as u64 == final_phnum,
        "internal error: expected {final_phnum} program headers, got {}",
        img.phdrs.len()
    );
    ensure!(img.phdrs.len() < usize::from(PN_XNUM), "too many program headers");

    let (new_phoff, phaddr) = match place_of(Item::Phdrs) {
        Some((off, addr, _)) => (off, addr),
        None => (phoff, crate::elf::offset_to_vaddr(&img.phdrs, phoff).unwrap_or(0)),
    };
    let phsize = final_phnum * phent;
    for p in img.phdrs.iter_mut().filter(|p| p.p_type == PT_PHDR) {
        (p.offset, p.vaddr, p.paddr, p.filesz, p.memsz) = (new_phoff, phaddr, phaddr, phsize, phsize);
    }

    // ---- serialize the dynamic section, now that everything it points at is final ----
    let mut dynamic_out: Option<(u64, Vec<u8>)> = None;
    if let Some(ri) = regions.iter().position(|r| r.content == Content::Dynamic) {
        let r = &regions[ri];
        let d = img.dynamic.as_ref().unwrap();
        if d.dirty || r.fate != Fate::Keep {
            let (off, addr, size) = place_of(Item::Region(ri)).unwrap_or((r.offset, r.addr, r.size));
            let mut w = Writer::new(ctx);
            for (i, e) in d.entries.iter().enumerate() {
                let mut e = *e;
                if e.tag == DT_MIPS_RLD_MAP_REL {
                    e.val = e.val.wrapping_sub(addr + i as u64 * ctx.dyn_size() as u64);
                }
                e.encode_into(&mut w);
            }
            w.buf.resize(size as usize, 0);
            dynamic_out = Some((off, w.buf));
        }
    }

    // ---- write ----
    let mut out = img.data[..truncate_at as usize].to_vec();
    let zero = |out: &mut Vec<u8>, off: u64, len: u64| {
        if off < truncate_at {
            let end = off.saturating_add(len).min(truncate_at);
            if let Ok(s) = slice_mut(out, off, end - off) {
                s.fill(0);
            }
        }
    };
    for &(off, len) in &img.zero_ranges {
        zero(&mut out, off, len);
    }
    for r in regions.iter().filter(|r| r.fate == Fate::Move) {
        if cfg.clobber_old || r.offset < phoff + phsize {
            zero(&mut out, r.offset, r.size);
        }
    }
    if pht_moves && cfg.clobber_old {
        zero(&mut out, phoff, orig_phnum * phent);
    }
    if let Some(seg) = &segment {
        out.resize((seg.offset + seg.filesz) as usize, 0);
    }
    let put = |out: &mut Vec<u8>, off: u64, bytes: &[u8]| -> Result<()> {
        let end = off as usize + bytes.len();
        if out.len() < end {
            out.resize(end, 0);
        }
        slice_mut(out, off, bytes.len() as u64)?.copy_from_slice(bytes);
        Ok(())
    };

    let mut phw = Writer::new(ctx);
    for p in &img.phdrs {
        p.encode_into(&mut phw, ctx);
    }
    if !pht_moves && final_phnum < orig_phnum {
        zero(&mut out, phoff, orig_phnum * phent);
    }
    put(&mut out, new_phoff, &phw.buf)?;

    for (i, r) in regions.iter().enumerate() {
        let target = match (r.fate, place_of(Item::Region(i))) {
            (Fate::Move, Some((off, _, _))) => off,
            (Fate::Rewrite, _) => r.offset,
            _ => continue,
        };
        match r.content {
            Content::Interp => {
                let mut v = img.interp.as_ref().unwrap().value.clone();
                v.push(0);
                if r.fate == Fate::Rewrite {
                    zero(&mut out, r.offset, r.size);
                }
                put(&mut out, target, &v)?;
            }
            Content::DynStr => put(&mut out, target, &img.dynstr.as_ref().unwrap().bytes)?,
            Content::Dynamic => {}
            Content::Raw => put(&mut out, target, raw_bytes[i].as_ref().unwrap())?,
        }
    }
    if let Some((off, bytes)) = &dynamic_out {
        put(&mut out, *off, bytes)?;
    }
    for (i, n) in img.new_notes.iter().enumerate() {
        let (off, _, _) = place_of(Item::Note(i)).unwrap();
        put(&mut out, off, &n.bytes)?;
    }

    // ---- non-allocated tail: sections after a rebuilt segment, .shstrtab, section headers ----
    if reuse.is_some() {
        for si in 0..img.shdrs.len() {
            let s = &img.shdrs[si];
            if s.has_file_data() && s.size > 0 && s.flags & SHF_ALLOC == 0 && s.offset >= truncate_at {
                let bytes = slice(&img.data, s.offset, s.size)?.to_vec();
                let off = align_up(out.len() as u64, s.addralign.max(1));
                put(&mut out, off, &bytes)?;
                img.shdrs[si].offset = off;
            }
        }
    }
    if !shstrtab_extra.is_empty() {
        let s = &img.shdrs[img.shstrndx];
        let mut bytes = slice(&img.data, s.offset, s.size)?.to_vec();
        bytes.extend_from_slice(&shstrtab_extra);
        let off = out.len() as u64;
        put(&mut out, off, &bytes)?;
        let s = &mut img.shdrs[img.shstrndx];
        (s.offset, s.size) = (off, bytes.len() as u64);
    }
    let shnum = img.shdrs.len();
    if shnum > 0 {
        let shent = ctx.shdr_size() as u64;
        let old = img.ehdr.shoff;
        // Keep the section headers last in the file, as linkers do.
        let fits = shnum <= img.orig_shnum && old + shnum as u64 * shent <= truncate_at && segment.is_none();
        zero(&mut out, old, img.orig_shnum as u64 * shent);
        if !fits {
            img.ehdr.shoff = align_up(out.len() as u64, ctx.word() as u64);
        }
        let (e_shnum, e_shstrndx) = encode_section_counts(&mut img.shdrs, img.shstrndx);
        img.ehdr.shnum = e_shnum;
        img.ehdr.shstrndx = e_shstrndx;
        let mut w = Writer::new(ctx);
        for s in &img.shdrs {
            s.encode_into(&mut w);
        }
        put(&mut out, img.ehdr.shoff, &w.buf)?;
    }

    img.ehdr.phoff = new_phoff;
    img.ehdr.phnum = final_phnum as u16;
    put(&mut out, 0, &img.ehdr.encode(ctx))?;
    Ok(out)
}

/// Remove up to `count` `PT_NULL` entries, to make room for new ones.
fn drop_unused(phdrs: &mut Vec<Phdr>, count: &mut u64) {
    while *count > 0 {
        let Some(i) = phdrs.iter().rposition(|p| p.p_type == PT_NULL) else { break };
        phdrs.remove(i);
        *count -= 1;
    }
}

/// Something placed in the new segment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Phdrs,
    Region(usize),
    Note(usize),
}
