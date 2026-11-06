//! Where the new segment goes, and what has to move to make room.

use super::regions::{Ref, Region};
use super::Config;
use crate::bail;
use crate::elf::codec::{align_down, align_up};
use crate::elf::consts::*;
use crate::elf::default_page_size;
use crate::elf::types::Phdr;
use crate::error::Result;
use crate::image::Image;

/// Executables get their new segment where pre-5.18 kernels expect the
/// program headers, unless that would need more padding than this.
pub(super) const COMPAT_PADDING_LIMIT: u64 = 256 * 1024;

/// Alignments above this are treated as bogus when aligning the new segment.
pub(super) const MAX_SEGMENT_ALIGN: u64 = 16 * 1024 * 1024;

/// Regions to move so the program header table can grow to `n` entries in
/// place, or `None` if something that can't move is in the way.
pub(super) fn evacuation(img: &Image, regions: &[Region], n: u64) -> Option<Vec<usize>> {
    let phent = img.ctx.phdr_size() as u64;
    let phoff = img.ehdr.phoff;
    let old_end = phoff + img.orig_phdrs.len() as u64 * phent;
    let need_end = phoff + n * phent;
    let host = img.phdrs.iter().find(|p| p.p_type == PT_LOAD && p.contains_offset(phoff, old_end - phoff))?;
    if need_end > host.file_end() {
        return None;
    }
    let overlaps = |off: u64, len: u64| off < need_end && off.saturating_add(len) > old_end;
    let mut chosen = Vec::new();
    for (i, r) in regions.iter().enumerate() {
        if overlaps(r.offset, r.size) {
            if !r.movable || r.offset < old_end {
                return None;
            }
            chosen.push(i);
        }
    }
    let covered = |off: u64, len: u64| chosen.iter().any(|&i| regions[i].contains(off, len));
    for s in &img.shdrs {
        if s.has_file_data() && s.size > 0 && overlaps(s.offset, s.size) && !covered(s.offset, s.size) {
            return None;
        }
    }
    for p in &img.phdrs {
        if p.p_type != PT_LOAD
            && p.p_type != PT_PHDR
            && p.filesz > 0
            && overlaps(p.offset, p.filesz)
            && !covered(p.offset, p.filesz)
        {
            return None;
        }
    }
    let lo = host.vaddr + (old_end - host.offset);
    let hi = host.vaddr + (need_end - host.offset);
    let in_window = |a: u64| a >= lo && a < hi;
    if in_window(img.ehdr.entry) {
        return None;
    }
    if let Some(d) = &img.dynamic {
        let owned = |tag: u64| chosen.iter().any(|&i| regions[i].refs.contains(&Ref::Tag(tag)));
        if d.entries.iter().any(|e| ADDRESS_TAGS.contains(&e.tag) && in_window(e.val) && !owned(e.tag)) {
            return None;
        }
    }
    Some(chosen)
}

/// Alignment for the new segment: `--page-size`, or the largest of the
/// architecture's page size and the existing segments' alignment.
pub(super) fn segment_alignment(img: &Image, cfg: &Config) -> u64 {
    if let Some(p) = cfg.page_size {
        return p;
    }
    let existing = img
        .phdrs
        .iter()
        .filter(|p| p.p_type == PT_LOAD && p.align <= MAX_SEGMENT_ALIGN && p.align.is_power_of_two())
        .map(|p| p.align)
        .max()
        .unwrap_or(1);
    existing.max(default_page_size(img.ehdr.machine))
}

/// Pick the file offset and address of a new segment of at most `size` bytes.
pub(super) fn place_segment(img: &Image, file_end: u64, size: u64, align: u64, start_align: u64) -> Result<(u64, u64)> {
    let loads: Vec<&Phdr> = img.phdrs.iter().filter(|p| p.p_type == PT_LOAD).collect();
    let Some(first) = loads.iter().min_by_key(|p| p.vaddr) else {
        bail!("the file has no loadable segments");
    };
    let start = align_up(file_end, start_align.max(8));

    // Best effort for loaders that compute the program header address as
    // `first segment base + e_phoff`: keep the new segment at that same base.
    if img.is_executable() && first.vaddr >= first.offset {
        let base = first.vaddr - first.offset;
        if base % align == 0 {
            let mut off = start;
            'search: for _ in 0..=loads.len() {
                let Some(end) = base.checked_add(off).and_then(|v| v.checked_add(size)) else { break };
                let (lo, hi) = (align_down(base + off, align), align_up(end, align));
                for p in &loads {
                    let (plo, phi) = (align_down(p.vaddr, align), align_up(p.mem_end(), align));
                    if lo < phi && plo < hi {
                        off = phi - base;
                        continue 'search;
                    }
                }
                if off - file_end <= COMPAT_PADDING_LIMIT {
                    return Ok((off, base + off));
                }
                break;
            }
        }
    }

    let top = loads.iter().map(|p| p.mem_end()).max().unwrap();
    match align_up(top, align).checked_add(start % align) {
        Some(vaddr) if vaddr.checked_add(size).is_some() => Ok((start, vaddr)),
        _ => bail!("no room for a new segment in the address space"),
    }
}

/// The last `PT_LOAD`, if it holds nothing but movable regions and nothing
/// but non-allocated data follows it, so it can be rebuilt from scratch.
pub(super) fn reusable_tail(img: &Image, regions: &[Region]) -> Option<usize> {
    let (li, l) = img.phdrs.iter().enumerate().filter(|(_, p)| p.p_type == PT_LOAD).max_by_key(|(_, p)| p.vaddr)?;
    if l.filesz == 0 || l.filesz != l.memsz || l.file_end() > img.data.len() as u64 {
        return None;
    }
    let inside: Vec<&Region> = regions.iter().filter(|r| l.contains_offset(r.offset, r.size)).collect();
    if inside.iter().any(|r| !r.movable) {
        return None;
    }
    let refs_phdr = |i: usize| inside.iter().any(|r| r.refs.contains(&Ref::Phdr(i)));
    let phsize = img.orig_phdrs.len() as u64 * img.ctx.phdr_size() as u64;
    for (i, p) in img.phdrs.iter().enumerate() {
        if i == li || p.filesz == 0 {
            continue;
        }
        if l.contains_offset(p.offset, p.filesz) {
            if p.p_type == PT_PHDR || refs_phdr(i) {
                continue;
            }
            return None;
        }
        if p.file_end() > l.offset {
            return None;
        }
    }
    if img.phdrs.iter().any(|p| p.p_type == PT_LOAD && p.vaddr != l.vaddr && p.mem_end() > l.vaddr) {
        return None;
    }
    let phoff = img.ehdr.phoff;
    if phoff + phsize > l.offset && !l.contains_offset(phoff, phsize) {
        return None;
    }
    for s in img.shdrs.iter().filter(|s| s.has_file_data() && s.size > 0) {
        let end = s.offset.saturating_add(s.size);
        if end <= l.offset {
            continue;
        }
        if s.offset < l.offset {
            return None;
        }
        if s.flags & SHF_ALLOC == 0 {
            if s.offset < l.file_end() {
                return None;
            }
        } else if !inside.iter().any(|r| r.contains(s.offset, s.size)) {
            return None;
        }
    }
    if img.ehdr.shoff > l.offset && img.ehdr.shoff < l.file_end() {
        return None;
    }
    let in_l = |a: u64| a >= l.vaddr && a < l.mem_end();
    if in_l(img.ehdr.entry) {
        return None;
    }
    if let Some(d) = &img.dynamic {
        let owned = |tag: u64| inside.iter().any(|r| r.refs.contains(&Ref::Tag(tag)));
        if d.entries.iter().any(|e| ADDRESS_TAGS.contains(&e.tag) && in_l(e.val) && !owned(e.tag)) {
            return None;
        }
    }
    Some(li)
}
