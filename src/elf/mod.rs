//! Reading ELF headers. Everything here is read-only and bounds-checked;
//! editing happens on [`crate::image::Image`].

pub mod codec;
pub mod consts;
pub mod hash;
pub mod note;
pub mod types;

use crate::error::Result;
use crate::{bail, ensure};
use codec::Ctx;
use consts::*;
use types::{Ehdr, Phdr, Shdr};

/// The parsed header tables of an ELF file.
#[derive(Clone, Debug)]
pub struct Elf {
    pub ctx: Ctx,
    pub ehdr: Ehdr,
    pub phdrs: Vec<Phdr>,
    pub shdrs: Vec<Shdr>,
    /// Index of the section name string table (resolved through `SHN_XINDEX`).
    pub shstrndx: usize,
}

/// Check the identification bytes and return the encoding they declare.
pub fn identify(data: &[u8]) -> Result<Ctx> {
    ensure!(data.len() >= 16 && &data[..4] == ELFMAG, "not an ELF executable");
    ensure!(data[EI_VERSION] == EV_CURRENT, "unsupported ELF version");
    let is64 = match data[EI_CLASS] {
        ELFCLASS32 => false,
        ELFCLASS64 => true,
        _ => bail!("ELF executable is not 32 or 64 bit"),
    };
    let le = match data[EI_DATA] {
        ELFDATA2LSB => true,
        ELFDATA2MSB => false,
        _ => bail!("unsupported ELF data encoding"),
    };
    Ok(Ctx { is64, le })
}

/// The `e_machine` of an ELF header, if `header` starts with one.
pub fn machine_of(header: &[u8]) -> Option<u16> {
    let ctx = identify(header).ok()?;
    ctx.u16(header, 18).ok()
}

impl Elf {
    pub fn parse(data: &[u8]) -> Result<Elf> {
        let ctx = identify(data)?;
        ensure!(data.len() >= ctx.ehdr_size(), "missing ELF header");
        let ehdr = Ehdr::read(ctx, data)?;
        ensure!(ehdr.e_type == ET_EXEC || ehdr.e_type == ET_DYN, "wrong ELF type");

        // Section header 0 carries the real counts when they overflow the ELF header.
        let mut shnum = u64::from(ehdr.shnum);
        let mut shstrndx = u64::from(ehdr.shstrndx);
        let mut phnum = u64::from(ehdr.phnum);
        if ehdr.shoff != 0 {
            ensure!(usize::from(ehdr.shentsize) == ctx.shdr_size(), "section headers have the wrong size");
            let first = Shdr::read(ctx, data, ehdr.shoff).map_err(|e| e.context("reading section header 0"))?;
            if shnum == 0 {
                shnum = first.size;
            }
            if ehdr.shstrndx == SHN_XINDEX {
                shstrndx = u64::from(first.link);
            }
            if ehdr.phnum == PN_XNUM {
                phnum = u64::from(first.info);
            }
        } else {
            shnum = 0;
        }

        if phnum > 0 {
            ensure!(usize::from(ehdr.phentsize) == ctx.phdr_size(), "program headers have the wrong size");
        }
        let table_fits = |off: u64, n: u64, size: usize| {
            n.checked_mul(size as u64).and_then(|len| off.checked_add(len)).is_some_and(|end| end <= data.len() as u64)
        };
        ensure!(table_fits(ehdr.phoff, phnum, ctx.phdr_size()), "program header table out of bounds");
        ensure!(table_fits(ehdr.shoff, shnum, ctx.shdr_size()), "section header table out of bounds");

        let phdrs = (0..phnum)
            .map(|i| Phdr::read(ctx, data, ehdr.phoff + i * ctx.phdr_size() as u64))
            .collect::<Result<Vec<_>>>()?;
        for (i, p) in phdrs.iter().enumerate() {
            let fits = p.offset.checked_add(p.filesz).is_some() && p.vaddr.checked_add(p.filesz.max(p.memsz)).is_some();
            ensure!(fits, "program header {i} is out of range");
        }
        let shdrs = (0..shnum)
            .map(|i| Shdr::read(ctx, data, ehdr.shoff + i * ctx.shdr_size() as u64))
            .collect::<Result<Vec<_>>>()?;
        ensure!(shnum == 0 || shstrndx < shnum, "invalid section name string table index");

        Ok(Elf { ctx, ehdr, phdrs, shdrs, shstrndx: shstrndx as usize })
    }
}

/// Map `[addr, addr + len)` to a file offset through the `PT_LOAD` segments.
pub fn vaddr_to_offset(phdrs: &[Phdr], addr: u64, len: u64) -> Option<u64> {
    phdrs
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .find(|p| addr >= p.vaddr && addr.checked_add(len).is_some_and(|end| end <= p.vaddr.saturating_add(p.filesz)))
        .map(|p| p.offset + (addr - p.vaddr))
}

/// Map a file offset back to a virtual address through the `PT_LOAD` segments.
pub fn offset_to_vaddr(phdrs: &[Phdr], off: u64) -> Option<u64> {
    phdrs.iter().filter(|p| p.p_type == PT_LOAD).find(|p| p.contains_offset(off, 1)).map(|p| p.vaddr + (off - p.offset))
}

/// The name of section `idx`, looked up in the section name string table.
pub fn section_name<'a>(data: &'a [u8], shdrs: &[Shdr], shstrndx: usize, idx: usize) -> Option<&'a [u8]> {
    let strtab = shdrs.get(shstrndx)?;
    let sh = shdrs.get(idx)?;
    if u64::from(sh.name) >= strtab.size {
        return None;
    }
    codec::cstr(data, strtab.offset.checked_add(u64::from(sh.name))?).ok()
}

/// The architecture's largest page size, used to align new segments.
/// The list follows GNU gold's `abi_pagesize`.
pub fn default_page_size(machine: u16) -> u64 {
    match machine {
        EM_ALPHA | EM_IA_64 | EM_MIPS | EM_PPC | EM_PPC64 | EM_AARCH64 | EM_TILEGX | EM_LOONGARCH => 0x10000,
        EM_SPARC | EM_SPARCV9 => 0x2000,
        _ => 0x1000,
    }
}
