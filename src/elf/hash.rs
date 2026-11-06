//! The GNU and System V symbol hash tables.

use super::codec::Ctx;
use super::consts::{EM_ALPHA, EM_S390};
use crate::ensure;
use crate::error::Result;

pub fn gnu(name: &[u8]) -> u32 {
    name.iter().fold(5381u32, |h, &c| h.wrapping_mul(33).wrapping_add(u32::from(c)))
}

pub fn sysv(name: &[u8]) -> u32 {
    let mut h = 0u32;
    for &c in name {
        h = (h << 4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
        }
        h &= !g;
    }
    h
}

#[derive(Clone, Copy, Debug)]
pub struct GnuHeader {
    pub nbuckets: u32,
    pub symoffset: u32,
    pub bloom_size: u32,
    pub shift2: u32,
}

impl GnuHeader {
    pub fn read(ctx: Ctx, data: &[u8], off: u64) -> Result<GnuHeader> {
        let h = GnuHeader {
            nbuckets: ctx.u32(data, off)?,
            symoffset: ctx.u32(data, off + 4)?,
            bloom_size: ctx.u32(data, off + 8)?,
            shift2: ctx.u32(data, off + 12)?,
        };
        let end = 16u64
            .checked_add(u64::from(h.bloom_size) * ctx.word() as u64)
            .and_then(|v| v.checked_add(4 * u64::from(h.nbuckets)))
            .and_then(|v| v.checked_add(off));
        ensure!(
            h.nbuckets > 0 && h.bloom_size > 0 && end.is_some_and(|e| e <= data.len() as u64),
            "GNU hash table header out of range"
        );
        Ok(h)
    }
    fn buckets_off(&self, ctx: Ctx, off: u64) -> u64 {
        off + 16 + u64::from(self.bloom_size) * ctx.word() as u64
    }
    fn chains_off(&self, ctx: Ctx, off: u64) -> u64 {
        self.buckets_off(ctx, off) + 4 * u64::from(self.nbuckets)
    }
}

/// Number of dynamic symbols, derived from a GNU hash table at `off`.
pub fn gnu_symbol_count(ctx: Ctx, data: &[u8], off: u64) -> Result<u64> {
    let h = GnuHeader::read(ctx, data, off)?;
    let buckets = h.buckets_off(ctx, off);
    let mut last = 0u32;
    for i in 0..u64::from(h.nbuckets) {
        last = last.max(ctx.u32(data, buckets + 4 * i)?);
    }
    if last < h.symoffset {
        return Ok(u64::from(h.symoffset));
    }
    let chains = h.chains_off(ctx, off);
    let mut idx = u64::from(last);
    loop {
        let v = ctx.u32(data, chains + 4 * (idx - u64::from(h.symoffset)))?;
        if v & 1 != 0 {
            return Ok(idx + 1);
        }
        idx += 1;
    }
}

/// Rewrite a GNU hash table in place. `hashes[i]` is the hash of symbol
/// `symoffset + i`; symbols must already be ordered by bucket.
pub fn gnu_rewrite(ctx: Ctx, data: &mut [u8], off: u64, h: &GnuHeader, hashes: &[u32]) -> Result<()> {
    let bits = ctx.word() as u32 * 8;
    let mut bloom = vec![0u64; h.bloom_size as usize];
    let mut buckets = vec![0u32; h.nbuckets as usize];
    for (i, &hash) in hashes.iter().enumerate() {
        let word = &mut bloom[((hash / bits) % h.bloom_size) as usize];
        *word |= 1u64 << (hash % bits);
        *word |= 1u64 << (hash.wrapping_shr(h.shift2) % bits);
        let b = &mut buckets[(hash % h.nbuckets) as usize];
        if *b == 0 {
            *b = h.symoffset + i as u32;
        }
    }
    for (i, w) in bloom.iter().enumerate() {
        ctx.put_word(data, off + 16 + (i * ctx.word()) as u64, *w)?;
    }
    let boff = h.buckets_off(ctx, off);
    for (i, b) in buckets.iter().enumerate() {
        ctx.put_u32(data, boff + 4 * i as u64, *b)?;
    }
    let coff = h.chains_off(ctx, off);
    for (i, &hash) in hashes.iter().enumerate() {
        let last = hashes.get(i + 1).is_none_or(|next| next % h.nbuckets != hash % h.nbuckets);
        let v = if last { hash | 1 } else { hash & !1 };
        ctx.put_u32(data, coff + 4 * i as u64, v)?;
    }
    Ok(())
}

/// Entry size of `.hash`: 8 bytes on 64-bit s390 and Alpha, 4 elsewhere.
pub fn sysv_entry_size(ctx: Ctx, machine: u16) -> u64 {
    if ctx.is64 && (machine == EM_S390 || machine == EM_ALPHA) {
        8
    } else {
        4
    }
}

fn read_entry(ctx: Ctx, data: &[u8], off: u64, ent: u64) -> Result<u64> {
    if ent == 8 {
        ctx.u64(data, off)
    } else {
        ctx.u32(data, off).map(u64::from)
    }
}

fn write_entry(ctx: Ctx, data: &mut [u8], off: u64, ent: u64, v: u64) -> Result<()> {
    if ent == 8 {
        ctx.put_u64(data, off, v)
    } else {
        ctx.put_u32(data, off, v as u32)
    }
}

/// `(nbucket, nchain)` of a System V hash table.
pub fn sysv_header(ctx: Ctx, data: &[u8], off: u64, ent: u64) -> Result<(u64, u64)> {
    let nbucket = read_entry(ctx, data, off, ent)?;
    let nchain = read_entry(ctx, data, off + ent, ent)?;
    let end = nbucket
        .checked_add(nchain)
        .and_then(|n| n.checked_add(2))
        .and_then(|n| n.checked_mul(ent))
        .and_then(|n| n.checked_add(off));
    ensure!(nbucket > 0 && end.is_some_and(|e| e <= data.len() as u64), "hash table header out of range");
    Ok((nbucket, nchain))
}

/// Rewrite a System V hash table in place. `hashes[i]` is the hash of symbol `i`.
pub fn sysv_rewrite(ctx: Ctx, data: &mut [u8], off: u64, ent: u64, hashes: &[u32]) -> Result<()> {
    let (nbucket, nchain) = sysv_header(ctx, data, off, ent)?;
    ensure!(hashes.len() as u64 == nchain, ".hash covers {nchain} symbols, expected {}", hashes.len());
    let mut buckets = vec![0u64; nbucket as usize];
    let mut chains = vec![0u64; nchain as usize];
    // Symbol 0 is the undefined symbol and terminates every chain.
    for (i, &hash) in hashes.iter().enumerate().skip(1) {
        let b = &mut buckets[(u64::from(hash) % nbucket) as usize];
        chains[i] = *b;
        *b = i as u64;
    }
    let mut pos = off + 2 * ent;
    for v in buckets.into_iter().chain(chains) {
        write_entry(ctx, data, pos, ent, v)?;
        pos += ent;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hashes() {
        assert_eq!(gnu(b""), 5381);
        assert_eq!(gnu(b"printf"), 0x156b_2bb8);
        assert_eq!(sysv(b"printf"), 0x077905a6);
    }
}
