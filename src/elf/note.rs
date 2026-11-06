//! ELF notes: `Elf_Nhdr` followed by a 4-byte-aligned name and descriptor.

use super::codec::{align_up, slice, Ctx, Writer};
use crate::error::Result;

pub struct Note<'a> {
    /// Offset of the note header within the buffer it was parsed from.
    pub offset: u64,
    /// Total size, including padding.
    pub size: u64,
    pub name: &'a [u8],
    pub n_type: u32,
    pub desc: &'a [u8],
}

/// Parse the notes in `data[start..start + len]`. Parsing stops at the first
/// malformed note, so a damaged note section never makes patchelf fail.
pub fn parse(ctx: Ctx, data: &[u8], start: u64, len: u64, align: u64) -> Vec<Note<'_>> {
    let align = if align == 8 { 8 } else { 4 };
    let end = start.saturating_add(len);
    let mut notes = Vec::new();
    let mut off = start;
    while off + 12 <= end {
        let Ok(parsed) = (|| -> Result<Note<'_>> {
            let namesz = u64::from(ctx.u32(data, off)?);
            let descsz = u64::from(ctx.u32(data, off + 4)?);
            let n_type = ctx.u32(data, off + 8)?;
            let name_off = off + 12;
            let desc_off = align_up(name_off + namesz, align);
            let next = align_up(desc_off + descsz, align);
            crate::ensure!(next <= end, "note extends past its segment");
            let mut name = slice(data, name_off, namesz)?;
            if let Some(stripped) = name.strip_suffix(b"\0") {
                name = stripped;
            }
            let desc = slice(data, desc_off, descsz)?;
            Ok(Note { offset: off, size: next - off, name, n_type, desc })
        })() else {
            break;
        };
        off += parsed.size;
        notes.push(parsed);
    }
    notes
}

/// Encode a note with 4-byte alignment.
pub fn encode(ctx: Ctx, name: &[u8], n_type: u32, desc: &[u8]) -> Vec<u8> {
    let mut w = Writer::new(ctx);
    w.u32(name.len() as u32 + 1);
    w.u32(desc.len() as u32);
    w.u32(n_type);
    w.bytes(name);
    w.u8(0);
    w.buf.resize(align_up(w.buf.len() as u64, 4) as usize, 0);
    w.bytes(desc);
    w.buf.resize(align_up(w.buf.len() as u64, 4) as usize, 0);
    w.buf
}
