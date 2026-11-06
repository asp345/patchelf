//! The editable in-memory model of one ELF file.
//!
//! Passes change the model; they never decide where bytes go in the output.
//! Tables that can grow (`PT_INTERP`, `.dynamic`, `.dynstr`, the program
//! header table and new notes) are kept as values here, and
//! [`crate::layout`] decides afterwards whether each still fits in its
//! original place. Fixed-size tables (`.dynsym`, `.gnu.version`, hash
//! tables, version requirements) are edited directly in [`Image::data`].
//!
//! Everything is found through the program headers and the dynamic section,
//! the way the dynamic loader finds it. Section headers are only updated to
//! stay consistent; they are never needed to understand the file.

use crate::elf::codec::{cstr, slice, Ctx};
use crate::elf::consts::*;
use crate::elf::types::{Dyn, Ehdr, Phdr, Shdr, Sym};
use crate::elf::{self, hash, Elf};
use crate::error::{Error, Result};
use crate::{bail, debug, ensure};

/// Where a table lives in the input file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc {
    pub offset: u64,
    pub addr: u64,
    pub size: u64,
}

/// The `PT_INTERP` string.
pub struct Interp {
    /// Interpreter path, without the terminating NUL.
    pub value: Vec<u8>,
    pub loc: Loc,
    pub dirty: bool,
}

/// The dynamic section.
pub struct Dynamic {
    /// Entries before the first `DT_NULL`. `DT_MIPS_RLD_MAP_REL` values are
    /// stored as absolute addresses here and made relative again on output,
    /// so entries can move freely.
    pub entries: Vec<Dyn>,
    pub loc: Loc,
    /// Number of entries (including `DT_NULL` padding) that fit in place.
    pub capacity: usize,
    pub dirty: bool,
}

impl Dynamic {
    pub fn get(&self, tag: u64) -> Option<u64> {
        self.entries.iter().find(|d| d.tag == tag).map(|d| d.val)
    }

    pub fn position(&self, tag: u64) -> Option<usize> {
        self.entries.iter().position(|d| d.tag == tag)
    }

    /// New entries go first, matching what earlier patchelf releases did.
    pub fn insert_front(&mut self, tag: u64, val: u64) {
        self.entries.insert(0, Dyn { tag, val });
        self.dirty = true;
    }
}

/// The dynamic string table.
pub struct StrTab {
    pub bytes: Vec<u8>,
    pub loc: Loc,
    /// Ranges `[start, nul)` of strings that something stopped referencing.
    /// Whatever in them is still unreferenced at the end gets overwritten,
    /// so stale paths don't linger in the file (Nix scans files for them).
    pub released: Vec<(usize, usize)>,
    pub dirty: bool,
}

impl StrTab {
    pub fn get(&self, off: u64) -> Result<&[u8]> {
        ensure!(off < self.bytes.len() as u64, "string table index {off} out of bounds");
        cstr(&self.bytes, off)
    }

    /// Offset of an existing NUL-terminated occurrence of `s` (possibly the tail of a longer string).
    fn find(&self, s: &[u8]) -> Option<usize> {
        let needle_len = s.len() + 1;
        self.bytes.windows(needle_len).position(|w| &w[..s.len()] == s && w[s.len()] == 0)
    }
}

/// A note that the layout must add to the file.
pub struct NewNote {
    pub section_name: &'static [u8],
    pub bytes: Vec<u8>,
}

/// The dynamic symbol table.
#[derive(Clone, Copy, Debug)]
pub struct SymTab {
    pub offset: u64,
    pub count: u64,
    pub entsize: u64,
}

pub struct Image {
    pub ctx: Ctx,
    /// Working copy of the file. In-place edits are made here.
    pub data: Vec<u8>,
    pub ehdr: Ehdr,
    pub phdrs: Vec<Phdr>,
    pub shdrs: Vec<Shdr>,
    pub shstrndx: usize,
    /// The program header table as read; its slots can be reused in place.
    pub orig_phdrs: Vec<Phdr>,
    /// Number of section headers as read (the space available at `e_shoff`).
    pub orig_shnum: usize,
    pub interp: Option<Interp>,
    pub dynamic: Option<Dynamic>,
    /// `PT_DYNAMIC` exists, but its contents are not in this file (e.g. a
    /// file produced by `objcopy --only-keep-debug`).
    pub dynamic_stripped: bool,
    /// The same for `PT_INTERP`.
    pub interp_stripped: bool,
    pub dynstr: Option<StrTab>,
    pub new_notes: Vec<NewNote>,
    /// File ranges to zero in the output.
    pub zero_ranges: Vec<(u64, u64)>,
    pub changed: bool,
}

impl Image {
    pub fn load(data: Vec<u8>) -> Result<Image> {
        let elf = Elf::parse(&data)?;
        let Elf { ctx, ehdr, phdrs, shdrs, shstrndx } = elf;
        let mut img = Image {
            ctx,
            data,
            ehdr,
            orig_phdrs: phdrs.clone(),
            orig_shnum: shdrs.len(),
            phdrs,
            shdrs,
            shstrndx,
            interp: None,
            dynamic: None,
            dynamic_stripped: false,
            interp_stripped: false,
            dynstr: None,
            new_notes: Vec::new(),
            zero_ranges: Vec::new(),
            changed: false,
        };
        img.interp = img.load_interp()?;
        img.dynamic = img.load_dynamic()?;
        if img.dynamic.is_some() {
            img.dynstr = img.load_dynstr()?;
        }
        Ok(img)
    }

    fn load_interp(&mut self) -> Result<Option<Interp>> {
        let Some(p) = self.phdrs.iter().find(|p| p.p_type == PT_INTERP).cloned() else {
            return Ok(None);
        };
        if p.filesz == 0 || self.is_stripped_at(p.vaddr) {
            self.interp_stripped = true;
            return Ok(None);
        }
        let bytes = slice(&self.data, p.offset, p.filesz).map_err(|e| e.context("reading PT_INTERP"))?;
        let value = bytes.split(|&b| b == 0).next().unwrap_or_default().to_vec();
        Ok(Some(Interp { value, loc: Loc { offset: p.offset, addr: p.vaddr, size: p.filesz }, dirty: false }))
    }

    fn load_dynamic(&mut self) -> Result<Option<Dynamic>> {
        let Some(p) = self.phdrs.iter().find(|p| p.p_type == PT_DYNAMIC).cloned() else {
            return Ok(None);
        };
        if p.filesz == 0 || self.is_stripped_at(p.vaddr) {
            self.dynamic_stripped = true;
            return Ok(None);
        }
        let entsize = self.ctx.dyn_size() as u64;
        slice(&self.data, p.offset, p.filesz).map_err(|e| e.context("reading PT_DYNAMIC"))?;
        let capacity = (p.filesz / entsize) as usize;
        let mut entries = Vec::new();
        for i in 0..capacity {
            let mut d = Dyn::read(self.ctx, &self.data, p.offset + i as u64 * entsize)?;
            if d.tag == DT_NULL {
                break;
            }
            if d.tag == DT_MIPS_RLD_MAP_REL {
                d.val = (p.vaddr + i as u64 * entsize).wrapping_add(d.val);
            }
            entries.push(d);
        }
        ensure!(entries.len() < capacity, "dynamic section is not terminated by DT_NULL");
        Ok(Some(Dynamic {
            entries,
            loc: Loc { offset: p.offset, addr: p.vaddr, size: p.filesz },
            capacity,
            dirty: false,
        }))
    }

    fn load_dynstr(&self) -> Result<Option<StrTab>> {
        let dynamic = self.dynamic.as_ref().unwrap();
        let (Some(addr), Some(size)) = (dynamic.get(DT_STRTAB), dynamic.get(DT_STRSZ)) else {
            return Ok(None);
        };
        let offset = self.map_addr(addr, size).map_err(|e| e.context("locating DT_STRTAB"))?;
        let bytes = slice(&self.data, offset, size)?.to_vec();
        Ok(Some(StrTab { bytes, loc: Loc { offset, addr, size }, released: Vec::new(), dirty: false }))
    }

    /// Whether a `NOBITS` section stands in for allocated data at `addr`,
    /// as in files made by `objcopy --only-keep-debug`.
    fn is_stripped_at(&self, addr: u64) -> bool {
        self.shdrs.iter().any(|s| s.sh_type == SHT_NOBITS && s.flags & SHF_ALLOC != 0 && s.addr == addr && s.size > 0)
    }

    // ----- basic queries -----

    pub fn is_library_or_pie(&self) -> bool {
        self.ehdr.e_type == ET_DYN
    }

    /// Whether the kernel loads this file directly (it has an interpreter or is non-PIE).
    pub fn is_executable(&self) -> bool {
        self.ehdr.e_type == ET_EXEC || self.phdrs.iter().any(|p| p.p_type == PT_INTERP)
    }

    pub fn map_addr(&self, addr: u64, len: u64) -> Result<u64> {
        elf::vaddr_to_offset(&self.phdrs, addr, len)
            .ok_or_else(|| Error::new(format!("address {addr:#x} (+{len}) is not in a loaded part of the file")))
    }

    pub fn section_name(&self, idx: usize) -> Option<&[u8]> {
        elf::section_name(&self.data, &self.shdrs, self.shstrndx, idx)
    }

    pub fn find_section(&self, name: &[u8]) -> Option<usize> {
        (0..self.shdrs.len()).find(|&i| self.section_name(i) == Some(name))
    }

    pub fn dynamic(&self) -> Result<&Dynamic> {
        match &self.dynamic {
            Some(d) => Ok(d),
            None if self.dynamic_stripped => bail!("the dynamic section's contents are not in this file"),
            None => bail!("cannot find the dynamic section; the input file is most likely statically linked"),
        }
    }

    pub fn dynamic_mut(&mut self) -> Result<&mut Dynamic> {
        self.dynamic()?;
        Ok(self.dynamic.as_mut().unwrap())
    }

    pub fn dynstr(&self) -> Result<&StrTab> {
        self.dynamic()?;
        self.dynstr.as_ref().ok_or_else(|| Error::new("the dynamic section has no string table"))
    }

    /// The string at `off` in the dynamic string table.
    pub fn string(&self, off: u64) -> Result<Vec<u8>> {
        Ok(self.dynstr()?.get(off)?.to_vec())
    }

    /// Values of all dynamic entries with `tag`, resolved as strings.
    pub fn dynamic_strings(&self, tag: u64) -> Result<Vec<Vec<u8>>> {
        let dynamic = self.dynamic()?;
        dynamic.entries.iter().filter(|d| d.tag == tag).map(|d| self.string(d.val)).collect()
    }

    // ----- dynamic symbols -----

    pub fn dynsym(&self) -> Result<Option<SymTab>> {
        let Some(dynamic) = &self.dynamic else { return Ok(None) };
        let Some(addr) = dynamic.get(DT_SYMTAB) else { return Ok(None) };
        let entsize = dynamic.get(DT_SYMENT).unwrap_or(self.ctx.sym_size() as u64);
        ensure!(entsize == self.ctx.sym_size() as u64, "unexpected DT_SYMENT {entsize}");
        let count = self.dynsym_count(addr, entsize)?;
        let size = count.checked_mul(entsize).ok_or_else(|| Error::new("dynamic symbol table too large"))?;
        let offset = self.map_addr(addr, size).map_err(|e| e.context("locating DT_SYMTAB"))?;
        Ok(Some(SymTab { offset, count, entsize }))
    }

    fn dynsym_count(&self, addr: u64, entsize: u64) -> Result<u64> {
        if let Some(s) = self.shdrs.iter().find(|s| s.sh_type == SHT_DYNSYM && s.addr == addr) {
            return Ok(s.size / entsize);
        }
        let dynamic = self.dynamic()?;
        if let Some(h) = dynamic.get(DT_HASH) {
            let ent = hash::sysv_entry_size(self.ctx, self.ehdr.machine);
            let off = self.map_addr(h, 2 * ent)?;
            return Ok(hash::sysv_header(self.ctx, &self.data, off, ent)?.1);
        }
        if let Some(h) = dynamic.get(DT_GNU_HASH) {
            let off = self.map_addr(h, 16)?;
            return hash::gnu_symbol_count(self.ctx, &self.data, off);
        }
        bail!("cannot determine the number of dynamic symbols")
    }

    pub fn read_sym(&self, t: &SymTab, i: u64) -> Result<Sym> {
        Sym::read(self.ctx, &self.data, t.offset + i * t.entsize)
    }

    pub fn write_sym(&mut self, t: &SymTab, i: u64, sym: &Sym) -> Result<()> {
        let bytes = sym.encode(self.ctx);
        crate::elf::codec::slice_mut(&mut self.data, t.offset + i * t.entsize, bytes.len() as u64)?
            .copy_from_slice(&bytes);
        Ok(())
    }

    /// File offset of the symbol version table (`DT_VERSYM`), if any.
    pub fn versym(&self, t: &SymTab) -> Result<Option<u64>> {
        let Some(addr) = self.dynamic.as_ref().and_then(|d| d.get(DT_VERSYM)) else {
            return Ok(None);
        };
        Ok(Some(self.map_addr(addr, t.count.saturating_mul(2)).map_err(|e| e.context("locating DT_VERSYM"))?))
    }

    /// File offsets of the `u32` fields in the version tables that hold
    /// dynamic string table offsets. `vn_file` fields come first, tagged `true`.
    pub fn version_string_fields(&self) -> Result<Vec<(u64, bool)>> {
        let mut fields = Vec::new();
        let Some(dynamic) = &self.dynamic else { return Ok(fields) };
        let ctx = self.ctx;
        if let (Some(addr), Some(num)) = (dynamic.get(DT_VERNEED), dynamic.get(DT_VERNEEDNUM)) {
            let mut off = self.map_addr(addr, 16).map_err(|e| e.context("locating DT_VERNEED"))?;
            for _ in 0..num {
                let cnt = ctx.u16(&self.data, off + 2)?;
                fields.push((off + 4, true));
                let mut aux = off + u64::from(ctx.u32(&self.data, off + 8)?);
                for _ in 0..cnt {
                    fields.push((aux + 8, false));
                    let next = ctx.u32(&self.data, aux + 12)?;
                    if next == 0 {
                        break;
                    }
                    aux += u64::from(next);
                }
                let next = ctx.u32(&self.data, off + 12)?;
                if next == 0 {
                    break;
                }
                off += u64::from(next);
            }
        }
        if let (Some(addr), Some(num)) = (dynamic.get(DT_VERDEF), dynamic.get(DT_VERDEFNUM)) {
            let mut off = self.map_addr(addr, 20).map_err(|e| e.context("locating DT_VERDEF"))?;
            for _ in 0..num {
                let cnt = ctx.u16(&self.data, off + 6)?;
                let mut aux = off + u64::from(ctx.u32(&self.data, off + 12)?);
                for _ in 0..cnt {
                    fields.push((aux, false));
                    let next = ctx.u32(&self.data, aux + 4)?;
                    if next == 0 {
                        break;
                    }
                    aux += u64::from(next);
                }
                let next = ctx.u32(&self.data, off + 16)?;
                if next == 0 {
                    break;
                }
                off += u64::from(next);
            }
        }
        Ok(fields)
    }

    // ----- strings -----

    /// Every known reference into the dynamic string table.
    pub fn string_refs(&self) -> Result<Vec<u64>> {
        let mut refs = Vec::new();
        let Some(dynamic) = &self.dynamic else { return Ok(refs) };
        refs.extend(dynamic.entries.iter().filter(|d| STRING_TAGS.contains(&d.tag)).map(|d| d.val));
        if let Some(t) = self.dynsym()? {
            for i in 0..t.count {
                refs.push(u64::from(self.read_sym(&t, i)?.name));
            }
        }
        for (field, _) in self.version_string_fields()? {
            refs.push(u64::from(self.ctx.u32(&self.data, field)?));
        }
        Ok(refs)
    }

    /// Bytes of the string table covered by some reference.
    fn live_bytes(&self) -> Result<Vec<bool>> {
        let strtab = self.dynstr()?;
        let mut live = vec![false; strtab.bytes.len()];
        for r in self.string_refs()? {
            let Ok(mut i) = usize::try_from(r) else { continue };
            while i < live.len() && !live[i] {
                live[i] = true;
                if strtab.bytes[i] == 0 {
                    break;
                }
                i += 1;
            }
        }
        Ok(live)
    }

    /// Note that the string at `off` lost a reference.
    pub fn release_string(&mut self, off: u64) -> Result<()> {
        let strtab = self.dynstr.as_mut().ok_or_else(|| Error::new("no dynamic string table"))?;
        let len = strtab.get(off)?.len();
        strtab.released.push((off as usize, off as usize + len));
        Ok(())
    }

    /// Return an offset at which `s` can be referenced, adding it if needed.
    pub fn intern(&mut self, s: &[u8]) -> Result<u64> {
        ensure!(!s.contains(&0), "string contains a NUL byte");
        if let Some(off) = self.dynstr()?.find(s) {
            return Ok(off as u64);
        }
        // Reuse the space of a released string nothing points into any more.
        let live = self.live_bytes()?;
        let strtab = self.dynstr.as_mut().unwrap();
        strtab.dirty = true;
        for &(start, nul) in &strtab.released {
            if nul - start >= s.len() && !live[start..=nul].iter().any(|&l| l) {
                strtab.bytes[start..start + s.len()].copy_from_slice(s);
                strtab.bytes[start + s.len()] = 0;
                return Ok(start as u64);
            }
        }
        let off = strtab.bytes.len();
        strtab.bytes.extend_from_slice(s);
        strtab.bytes.push(0);
        Ok(off as u64)
    }

    /// Overwrite released strings that nothing references any more.
    pub fn scrub_released_strings(&mut self) -> Result<()> {
        if self.dynstr.as_ref().is_none_or(|s| s.released.is_empty()) {
            return Ok(());
        }
        let live = self.live_bytes()?;
        let strtab = self.dynstr.as_mut().unwrap();
        for &(start, nul) in &strtab.released {
            for (byte, &is_live) in strtab.bytes[start..nul].iter_mut().zip(&live[start..nul]) {
                if !is_live && *byte != 0 {
                    *byte = b'X';
                    strtab.dirty = true;
                }
            }
        }
        strtab.released.clear();
        Ok(())
    }

    // ----- sections -----

    /// Delete section `idx` and renumber every reference to later sections.
    pub fn remove_section(&mut self, idx: usize) -> Result<()> {
        ensure!(idx != 0 && idx < self.shdrs.len(), "invalid section index {idx}");
        ensure!(idx != self.shstrndx, "cannot remove the section name string table");
        let renumber = |i: u32| -> u32 {
            match (i as usize).cmp(&idx) {
                std::cmp::Ordering::Less => i,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => i - 1,
            }
        };
        // Symbol tables store section indices.
        let ctx = self.ctx;
        for s in self.shdrs.clone() {
            if s.sh_type != SHT_SYMTAB && s.sh_type != SHT_DYNSYM || s.entsize == 0 {
                continue;
            }
            for i in 0..s.size / s.entsize {
                let field = i
                    .checked_mul(s.entsize)
                    .and_then(|v| v.checked_add(s.offset))
                    .and_then(|v| v.checked_add(if ctx.is64 { 6 } else { 14 }))
                    .ok_or_else(|| Error::new("symbol table out of range"))?;
                let shndx = ctx.u16(&self.data, field)?;
                if shndx != SHN_UNDEF && shndx < SHN_LORESERVE && usize::from(shndx) >= idx {
                    ctx.put_u16(&mut self.data, field, renumber(u32::from(shndx)) as u16)?;
                }
            }
        }
        self.shdrs.remove(idx);
        if self.shstrndx > idx {
            self.shstrndx -= 1;
        }
        for s in &mut self.shdrs {
            s.link = renumber(s.link);
            if s.sh_type == SHT_REL || s.sh_type == SHT_RELA || s.flags & SHF_INFO_LINK != 0 {
                s.info = renumber(s.info);
            }
        }
        debug!("removed section {idx}");
        Ok(())
    }

    /// Find the section header describing an allocated table at `loc`.
    pub fn section_at(&self, loc: &Loc) -> Option<usize> {
        self.shdrs
            .iter()
            .position(|s| s.flags & SHF_ALLOC != 0 && s.has_file_data() && s.offset == loc.offset && s.addr == loc.addr)
    }
}
