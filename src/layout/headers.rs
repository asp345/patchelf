//! Keeping symbol and section header tables consistent with moved data.

use crate::elf::codec::slice_mut;
use crate::elf::consts::*;
use crate::elf::types::{Shdr, Sym};
use crate::error::Result;
use crate::image::Image;

/// Add each moved section's address change to the symbols defined in it.
pub(super) fn move_symbols(img: &mut Image, moves: &[(usize, u64)]) -> Result<()> {
    if moves.is_empty() {
        return Ok(());
    }
    let ctx = img.ctx;
    let entsize = ctx.sym_size() as u64;
    let tables: Vec<Shdr> = img
        .shdrs
        .iter()
        .filter(|s| matches!(s.sh_type, SHT_SYMTAB | SHT_DYNSYM) && s.entsize == entsize && s.has_file_data())
        .cloned()
        .collect();
    for t in tables {
        for i in 0..t.size / entsize {
            let Some(off) = (i * entsize).checked_add(t.offset) else { break };
            let Ok(mut sym) = Sym::read(ctx, &img.data, off) else { break };
            if sym.shndx == SHN_UNDEF || sym.shndx >= SHN_LORESERVE {
                continue;
            }
            if let Some(&(_, delta)) = moves.iter().find(|(si, _)| *si == usize::from(sym.shndx)) {
                sym.value = sym.value.wrapping_add(delta);
                let bytes = sym.encode(ctx);
                slice_mut(&mut img.data, off, bytes.len() as u64)?.copy_from_slice(&bytes);
            }
        }
    }
    Ok(())
}

/// Section counts for the ELF header, using section 0 for values that overflow.
pub(super) fn encode_section_counts(shdrs: &mut [Shdr], shstrndx: usize) -> (u16, u16) {
    let shnum = shdrs.len();
    let e_shnum = if shnum >= usize::from(SHN_LORESERVE) {
        shdrs[0].size = shnum as u64;
        0
    } else {
        shnum as u16
    };
    let e_shstrndx = if shstrndx >= usize::from(SHN_LORESERVE) {
        shdrs[0].link = shstrndx as u32;
        SHN_XINDEX
    } else {
        shstrndx as u16
    };
    (e_shnum, e_shstrndx)
}
