//! Class-neutral views of the ELF structures patchelf touches. Every value is
//! widened to 64 bits; `Ctx` decides the on-disk encoding.

use super::codec::{Ctx, Reader, Writer};
use crate::error::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ehdr {
    pub ident: [u8; 16],
    pub e_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

impl Ehdr {
    pub fn read(ctx: Ctx, data: &[u8]) -> Result<Ehdr> {
        let mut r = Reader::new(ctx, data, 0);
        Ok(Ehdr {
            ident: r.bytes()?,
            e_type: r.u16()?,
            machine: r.u16()?,
            version: r.u32()?,
            entry: r.word()?,
            phoff: r.word()?,
            shoff: r.word()?,
            flags: r.u32()?,
            ehsize: r.u16()?,
            phentsize: r.u16()?,
            phnum: r.u16()?,
            shentsize: r.u16()?,
            shnum: r.u16()?,
            shstrndx: r.u16()?,
        })
    }

    pub fn encode(&self, ctx: Ctx) -> Vec<u8> {
        let mut w = Writer::new(ctx);
        w.bytes(&self.ident);
        w.u16(self.e_type);
        w.u16(self.machine);
        w.u32(self.version);
        w.word(self.entry);
        w.word(self.phoff);
        w.word(self.shoff);
        w.u32(self.flags);
        w.u16(self.ehsize);
        w.u16(self.phentsize);
        w.u16(self.phnum);
        w.u16(self.shentsize);
        w.u16(self.shnum);
        w.u16(self.shstrndx);
        w.buf
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Phdr {
    pub p_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl Phdr {
    pub fn read(ctx: Ctx, data: &[u8], off: u64) -> Result<Phdr> {
        let mut r = Reader::new(ctx, data, off);
        if ctx.is64 {
            Ok(Phdr {
                p_type: r.u32()?,
                flags: r.u32()?,
                offset: r.u64()?,
                vaddr: r.u64()?,
                paddr: r.u64()?,
                filesz: r.u64()?,
                memsz: r.u64()?,
                align: r.u64()?,
            })
        } else {
            let p_type = r.u32()?;
            let offset = r.word()?;
            let vaddr = r.word()?;
            let paddr = r.word()?;
            let filesz = r.word()?;
            let memsz = r.word()?;
            let flags = r.u32()?;
            let align = r.word()?;
            Ok(Phdr { p_type, flags, offset, vaddr, paddr, filesz, memsz, align })
        }
    }

    pub fn encode_into(&self, w: &mut Writer, ctx: Ctx) {
        if ctx.is64 {
            w.u32(self.p_type);
            w.u32(self.flags);
            w.u64(self.offset);
            w.u64(self.vaddr);
            w.u64(self.paddr);
            w.u64(self.filesz);
            w.u64(self.memsz);
            w.u64(self.align);
        } else {
            w.u32(self.p_type);
            w.word(self.offset);
            w.word(self.vaddr);
            w.word(self.paddr);
            w.word(self.filesz);
            w.word(self.memsz);
            w.u32(self.flags);
            w.word(self.align);
        }
    }

    pub fn file_end(&self) -> u64 {
        self.offset.saturating_add(self.filesz)
    }
    pub fn mem_end(&self) -> u64 {
        self.vaddr.saturating_add(self.memsz)
    }
    /// Whether the file range `[off, off + len)` lies inside this segment's file image.
    pub fn contains_offset(&self, off: u64, len: u64) -> bool {
        off >= self.offset && off.saturating_add(len) <= self.file_end()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shdr {
    pub name: u32,
    pub sh_type: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub addralign: u64,
    pub entsize: u64,
}

impl Shdr {
    pub fn read(ctx: Ctx, data: &[u8], off: u64) -> Result<Shdr> {
        let mut r = Reader::new(ctx, data, off);
        Ok(Shdr {
            name: r.u32()?,
            sh_type: r.u32()?,
            flags: r.word()?,
            addr: r.word()?,
            offset: r.word()?,
            size: r.word()?,
            link: r.u32()?,
            info: r.u32()?,
            addralign: r.word()?,
            entsize: r.word()?,
        })
    }

    pub fn encode_into(&self, w: &mut Writer) {
        w.u32(self.name);
        w.u32(self.sh_type);
        w.word(self.flags);
        w.word(self.addr);
        w.word(self.offset);
        w.word(self.size);
        w.u32(self.link);
        w.u32(self.info);
        w.word(self.addralign);
        w.word(self.entsize);
    }

    /// Whether the section occupies bytes in the file.
    pub fn has_file_data(&self) -> bool {
        self.sh_type != super::consts::SHT_NOBITS && self.sh_type != super::consts::SHT_NULL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dyn {
    pub tag: u64,
    pub val: u64,
}

impl Dyn {
    pub fn read(ctx: Ctx, data: &[u8], off: u64) -> Result<Dyn> {
        let mut r = Reader::new(ctx, data, off);
        Ok(Dyn { tag: r.word()?, val: r.word()? })
    }
    pub fn encode_into(&self, w: &mut Writer) {
        w.word(self.tag);
        w.word(self.val);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sym {
    pub name: u32,
    pub info: u8,
    pub other: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

impl Sym {
    pub fn read(ctx: Ctx, data: &[u8], off: u64) -> Result<Sym> {
        let mut r = Reader::new(ctx, data, off);
        if ctx.is64 {
            Ok(Sym { name: r.u32()?, info: r.u8()?, other: r.u8()?, shndx: r.u16()?, value: r.u64()?, size: r.u64()? })
        } else {
            let name = r.u32()?;
            let value = r.word()?;
            let size = r.word()?;
            Ok(Sym { name, value, size, info: r.u8()?, other: r.u8()?, shndx: r.u16()? })
        }
    }

    pub fn encode(&self, ctx: Ctx) -> Vec<u8> {
        let mut w = Writer::new(ctx);
        if ctx.is64 {
            w.u32(self.name);
            w.u8(self.info);
            w.u8(self.other);
            w.u16(self.shndx);
            w.u64(self.value);
            w.u64(self.size);
        } else {
            w.u32(self.name);
            w.word(self.value);
            w.word(self.size);
            w.u8(self.info);
            w.u8(self.other);
            w.u16(self.shndx);
        }
        w.buf
    }
}
