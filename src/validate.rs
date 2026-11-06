//! Structural checks on a finished file.
//!
//! patchelf refuses to write a file in which any of these checks fails,
//! unless the same problem was already present in the input (some
//! toolchains produce odd but working files, and patching them should
//! still work).

use std::collections::BTreeSet;

use crate::elf::codec::{cstr, slice};
use crate::elf::consts::*;
use crate::elf::types::{Dyn, Phdr};
use crate::elf::{vaddr_to_offset, Elf};

/// Return a description of every problem found in `data`.
pub fn problems(data: &[u8]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let elf = match Elf::parse(data) {
        Ok(e) => e,
        Err(e) => {
            found.insert(format!("cannot parse headers: {e}"));
            return found;
        }
    };
    let mut report = |msg: String| {
        found.insert(msg);
    };
    let len = data.len() as u64;
    let loads: Vec<&Phdr> = elf.phdrs.iter().filter(|p| p.p_type == PT_LOAD).collect();

    for p in &loads {
        if p.offset.checked_add(p.filesz).is_none_or(|end| end > len) {
            report(format!("segment at {:#x} extends past the end of the file", p.vaddr));
        }
        if p.filesz > p.memsz {
            report(format!("segment at {:#x} has p_filesz > p_memsz", p.vaddr));
        }
        if p.align > 1 && p.offset % p.align != p.vaddr % p.align {
            report(format!("segment at {:#x} has an offset that disagrees with its alignment", p.vaddr));
        }
    }
    for w in loads.windows(2) {
        if w[1].vaddr < w[0].vaddr {
            report(format!("segments at {:#x} and {:#x} are not sorted by address", w[0].vaddr, w[1].vaddr));
        }
    }
    let page = crate::elf::default_page_size(elf.ehdr.machine).min(0x1000);
    for (i, a) in loads.iter().enumerate() {
        for b in &loads[i + 1..] {
            if a.vaddr < b.mem_end() && b.vaddr < a.mem_end() && a.memsz > 0 && b.memsz > 0 {
                report(format!("segments at {:#x} and {:#x} overlap", a.vaddr, b.vaddr));
            }
            // Mappings are made page by page: two segments sharing a page must
            // map it from the same place in the file.
            let shares_page =
                a.vaddr / page <= (b.mem_end().max(1) - 1) / page && b.vaddr / page <= (a.mem_end().max(1) - 1) / page;
            if shares_page && a.vaddr.wrapping_sub(a.offset) != b.vaddr.wrapping_sub(b.offset) {
                report(format!("segments at {:#x} and {:#x} map the same page differently", a.vaddr, b.vaddr));
            }
        }
    }

    let in_load = |off: u64, size: u64| loads.iter().any(|p| p.contains_offset(off, size));
    let phsize = elf.phdrs.len() as u64 * elf.ctx.phdr_size() as u64;
    let needs_mapped_phdrs = elf.phdrs.iter().any(|p| p.p_type == PT_PHDR || p.p_type == PT_INTERP);
    if needs_mapped_phdrs && !loads.is_empty() && !in_load(elf.ehdr.phoff, phsize) {
        report("the program header table is not in a loaded segment".into());
    }
    for p in &elf.phdrs {
        match p.p_type {
            PT_PHDR => {
                let expected = crate::elf::offset_to_vaddr(&elf.phdrs, elf.ehdr.phoff);
                if p.offset != elf.ehdr.phoff || p.filesz != phsize || expected != Some(p.vaddr) {
                    report("PT_PHDR does not describe the program header table".into());
                }
            }
            PT_INTERP => {
                if !in_load(p.offset, p.filesz) {
                    report("PT_INTERP is not in a loaded segment".into());
                }
                match slice(data, p.offset, p.filesz) {
                    Ok(b) if b.last() == Some(&0) => {}
                    _ => report("the interpreter path is not NUL-terminated".into()),
                }
            }
            PT_DYNAMIC => check_dynamic(&elf, data, p, &mut report),
            _ => {}
        }
    }

    for (i, s) in elf.shdrs.iter().enumerate() {
        if !s.has_file_data() {
            continue;
        }
        if s.offset.checked_add(s.size).is_none_or(|end| end > len) {
            report(format!("section {i} extends past the end of the file"));
            continue;
        }
        if s.flags & SHF_ALLOC != 0 && s.size > 0 && !loads.is_empty() {
            let mapped = loads
                .iter()
                .any(|p| p.contains_offset(s.offset, s.size) && s.addr.wrapping_sub(p.vaddr) == s.offset - p.offset);
            if !mapped {
                report(format!("allocated section {i} is not where its segment maps it"));
            }
        }
    }
    found
}

fn check_dynamic(elf: &Elf, data: &[u8], p: &Phdr, report: &mut impl FnMut(String)) {
    let ctx = elf.ctx;
    let entsize = ctx.dyn_size() as u64;
    let dynamic_is_stripped = elf
        .shdrs
        .iter()
        .any(|s| s.sh_type == SHT_NOBITS && s.flags & SHF_ALLOC != 0 && s.addr == p.vaddr && s.size > 0);
    if dynamic_is_stripped || p.filesz == 0 {
        return;
    }
    if !elf.phdrs.iter().any(|l| l.p_type == PT_LOAD && l.contains_offset(p.offset, p.filesz)) {
        report("PT_DYNAMIC is not in a loaded segment".into());
        return;
    }
    let mut entries = Vec::new();
    for i in 0..p.filesz / entsize {
        match Dyn::read(ctx, data, p.offset + i * entsize) {
            Ok(d) if d.tag == DT_NULL => break,
            Ok(d) => entries.push(d),
            Err(_) => break,
        }
    }
    if entries.len() as u64 >= p.filesz / entsize {
        report("the dynamic section is not terminated".into());
    }
    let get = |tag| entries.iter().find(|d| d.tag == tag).map(|d| d.val);
    for d in &entries {
        let checked = [DT_STRTAB, DT_SYMTAB, DT_HASH, DT_GNU_HASH, DT_VERSYM, DT_VERNEED, DT_VERDEF];
        if checked.contains(&d.tag) && vaddr_to_offset(&elf.phdrs, d.val, 1).is_none() {
            report(format!("dynamic tag {:#x} points outside the loaded segments", d.tag));
        }
    }
    if let (Some(addr), Some(size)) = (get(DT_STRTAB), get(DT_STRSZ)) {
        match vaddr_to_offset(&elf.phdrs, addr, size) {
            None => report("the dynamic string table is not in a loaded segment".into()),
            Some(off) => match slice(data, off, size) {
                Err(_) => report("the dynamic string table extends past the end of the file".into()),
                Ok(strtab) => {
                    for d in entries.iter().filter(|d| STRING_TAGS.contains(&d.tag)) {
                        if d.val >= size || cstr(strtab, d.val).is_err() {
                            report(format!("dynamic tag {:#x} has an invalid string", d.tag));
                        }
                    }
                }
            },
        }
    }
}
