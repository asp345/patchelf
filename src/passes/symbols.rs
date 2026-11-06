//! `--clear-symbol-version` and `--rename-dynamic-symbols`.

use std::collections::HashSet;
use std::io::Write;

use super::{NameMap, Names, Pass};
use crate::elf::consts::*;
use crate::elf::hash::{self, GnuHeader};
use crate::elf::types::Sym;
use crate::error::Result;
use crate::image::{Image, SymTab};
use crate::{bail, debug, log::show};

pub struct ClearVersion(pub Names);

impl Pass for ClearVersion {
    fn name(&self) -> &'static str {
        "clear-symbol-version"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let Some(t) = img.dynsym()? else { bail!("the file has no dynamic symbol table") };
        let Some(versym) = img.versym(&t)? else { bail!("the file has no symbol version table") };
        for i in 0..t.count {
            let name = img.string(u64::from(img.read_sym(&t, i)?.name))?;
            if self.0.contains(&name) {
                debug!("clearing symbol version for {}", show(&name));
                // Index 1 is the unversioned global definition.
                let ctx = img.ctx;
                ctx.put_u16(&mut img.data, versym + 2 * i, 1)?;
                img.changed = true;
            }
        }
        Ok(())
    }
}

pub struct Rename(pub NameMap);

impl Pass for Rename {
    fn name(&self) -> &'static str {
        "rename-dynamic-symbols"
    }
    fn run(&self, img: &mut Image, _: &mut dyn Write) -> Result<()> {
        let Some(t) = img.dynsym()? else { bail!("the file has no dynamic symbol table") };
        let mut renamed = false;
        for i in 0..t.count {
            let mut sym = img.read_sym(&t, i)?;
            let name = img.string(u64::from(sym.name))?;
            let Some(new) = self.0.get(&name) else { continue };
            debug!("renaming dynamic symbol {} to {}", show(&name), show(new));
            img.release_string(u64::from(sym.name))?;
            sym.name = u32::try_from(img.intern(new)?).unwrap_or(u32::MAX);
            img.write_sym(&t, i, &sym)?;
            renamed = true;
        }
        if !renamed {
            return Ok(());
        }
        img.changed = true;
        rebuild_hash_tables(img, &t)
    }
}

/// Symbol names changed, so their hashes did too.
fn rebuild_hash_tables(img: &mut Image, t: &SymTab) -> Result<()> {
    let dynamic = img.dynamic()?;
    let gnu = dynamic.get(DT_GNU_HASH);
    let xhash = dynamic.get(DT_MIPS_XHASH);
    let sysv = dynamic.get(DT_HASH);
    if let Some(addr) = gnu {
        // On MIPS, symbol order is tied to the GOT, so .dynsym can't be
        // reordered as the GNU hash table would need (MIPS uses .MIPS.xhash).
        if img.ehdr.machine == EM_MIPS {
            bail!("cannot rebuild a GNU hash table on MIPS");
        }
        let off = img.map_addr(addr, 16)?;
        rebuild_gnu_hash(img, t, off)?;
    }
    if let Some(addr) = xhash {
        let off = img.map_addr(addr, 16)?;
        rebuild_mips_xhash(img, t, off)?;
    }
    if let Some(addr) = sysv {
        let ent = hash::sysv_entry_size(img.ctx, img.ehdr.machine);
        let off = img.map_addr(addr, 2 * ent)?;
        let mut hashes = Vec::with_capacity(t.count as usize);
        for i in 0..t.count {
            let name = img.string(u64::from(img.read_sym(t, i)?.name))?;
            hashes.push(hash::sysv(&name));
        }
        let ctx = img.ctx;
        hash::sysv_rewrite(ctx, &mut img.data, off, ent, &hashes)?;
    }
    Ok(())
}

/// Hashes of symbols `first..t.count` and their order sorted by bucket
/// (stable, so symbols already in the right bucket keep their order).
fn bucket_order(img: &Image, t: &SymTab, first: u64, nbuckets: u32) -> Result<(Vec<u32>, Vec<usize>)> {
    let mut hashes = Vec::new();
    for i in first..t.count {
        hashes.push(hash::gnu(&img.string(u64::from(img.read_sym(t, i)?.name))?));
    }
    let mut order: Vec<usize> = (0..hashes.len()).collect();
    order.sort_by_key(|&i| hashes[i] % nbuckets);
    Ok((hashes, order))
}

/// `.MIPS.xhash` is a GNU hash table followed by a translation table from
/// chain position to symbol index. MIPS orders `.dynsym` for the GOT, so the
/// hashed symbols are exactly those the translation table lists (not simply
/// those from `symoffset` on); they are re-sorted by bucket without moving
/// any symbol.
fn rebuild_mips_xhash(img: &mut Image, t: &SymTab, off: u64) -> Result<()> {
    let ctx = img.ctx;
    let h = GnuHeader::read(ctx, &img.data, off)?;
    let first = u64::from(h.symoffset);
    let n = t.count.saturating_sub(first);
    let xlat = off + 16 + u64::from(h.bloom_size) * ctx.word() as u64 + 4 * u64::from(h.nbuckets) + 4 * n;
    let mut syms = Vec::with_capacity(n as usize);
    let mut hashes = Vec::with_capacity(n as usize);
    for pos in 0..n {
        let index = u64::from(ctx.u32(&img.data, xlat + 4 * pos)?);
        crate::ensure!(index < t.count, ".MIPS.xhash refers to symbol {index} of {}", t.count);
        hashes.push(hash::gnu(&img.string(u64::from(img.read_sym(t, index)?.name))?));
        syms.push(index);
    }
    let mut order: Vec<usize> = (0..syms.len()).collect();
    order.sort_by_key(|&i| hashes[i] % h.nbuckets);
    let sorted: Vec<u32> = order.iter().map(|&i| hashes[i]).collect();
    hash::gnu_rewrite(ctx, &mut img.data, off, &h, &sorted)?;
    for (pos, &i) in order.iter().enumerate() {
        ctx.put_u32(&mut img.data, xlat + 4 * pos as u64, syms[i] as u32)?;
    }
    Ok(())
}

/// The GNU hash table requires hashed symbols to be grouped by bucket, so
/// renaming can reorder `.dynsym`. Everything that refers to dynamic symbols
/// by index (the version table and relocations) is renumbered to match.
fn rebuild_gnu_hash(img: &mut Image, t: &SymTab, off: u64) -> Result<()> {
    let ctx = img.ctx;
    let h = GnuHeader::read(ctx, &img.data, off)?;
    let first = u64::from(h.symoffset);
    if first >= t.count {
        return Ok(());
    }
    let syms: Vec<Sym> = (first..t.count).map(|i| img.read_sym(t, i)).collect::<Result<_>>()?;
    let (hashes, order) = bucket_order(img, t, first, h.nbuckets)?;
    let mut new_index = vec![0u64; syms.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = first + new as u64;
    }

    for (new, &old) in order.iter().enumerate() {
        img.write_sym(t, first + new as u64, &syms[old])?;
    }
    let versym = img.versym(t)?;
    let shndx = match img.dynamic()?.get(DT_SYMTAB_SHNDX) {
        Some(addr) => Some(img.map_addr(addr, 4 * t.count)?),
        None => None,
    };
    for (table, width) in [(versym, 2u64), (shndx, 4u64)] {
        let Some(base) = table else { continue };
        let old = crate::elf::codec::slice(&img.data, base + first * width, syms.len() as u64 * width)?.to_vec();
        for (new, &o) in order.iter().enumerate() {
            let src = &old[o * width as usize..(o + 1) * width as usize];
            let dst = base + (first + new as u64) * width;
            crate::elf::codec::slice_mut(&mut img.data, dst, width)?.copy_from_slice(src);
        }
    }
    let sorted: Vec<u32> = order.iter().map(|&i| hashes[i]).collect();
    hash::gnu_rewrite(ctx, &mut img.data, off, &h, &sorted)?;

    let remap = |sym: u64| if sym >= first && sym < t.count { new_index[(sym - first) as usize] } else { sym };
    renumber_relocations(img, &remap)
}

/// Apply `remap` to the symbol index of every dynamic relocation.
fn renumber_relocations(img: &mut Image, remap: &dyn Fn(u64) -> u64) -> Result<()> {
    let ctx = img.ctx;
    let word = ctx.word() as u64;
    let d = img.dynamic()?;
    let mut tables = Vec::new();
    if let (Some(a), Some(s)) = (d.get(DT_REL), d.get(DT_RELSZ)) {
        tables.push((a, s, 2 * word));
    }
    if let (Some(a), Some(s)) = (d.get(DT_RELA), d.get(DT_RELASZ)) {
        tables.push((a, s, 3 * word));
    }
    if let (Some(a), Some(s)) = (d.get(DT_JMPREL), d.get(DT_PLTRELSZ)) {
        let ent = if d.get(DT_PLTREL) == Some(DT_RELA) { 3 * word } else { 2 * word };
        tables.push((a, s, ent));
    }
    // DT_JMPREL often lies inside the DT_RELA range; renumber each entry once.
    let mut seen = HashSet::new();
    for (addr, size, ent) in tables {
        let base = img.map_addr(addr, size)?;
        for i in 0..size / ent {
            let info_off = base + i * ent + word;
            if !seen.insert(info_off) {
                continue;
            }
            let info = ctx.word_at(&img.data, info_off)?;
            let (sym, ty) = if ctx.is64 { (info >> 32, info & 0xffff_ffff) } else { (info >> 8, info & 0xff) };
            let new = remap(sym);
            if new != sym {
                let info = if ctx.is64 { (new << 32) | ty } else { (new << 8) | ty };
                ctx.put_word(&mut img.data, info_off, info)?;
            }
        }
    }
    Ok(())
}
