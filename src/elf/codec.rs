//! Bounds-checked, class- and byte-order-aware reading and writing.

use crate::error::{Error, Result};

/// The two properties that decide how every ELF field is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ctx {
    pub is64: bool,
    pub le: bool,
}

fn out_of_bounds(off: u64, len: u64) -> Error {
    Error::new(format!("data at offset {off:#x} (+{len}) extends past the end of the file"))
}

fn range(off: u64, len: u64) -> Result<std::ops::Range<usize>> {
    let start = usize::try_from(off).map_err(|_| out_of_bounds(off, len))?;
    let len_usize = usize::try_from(len).map_err(|_| out_of_bounds(off, len))?;
    let end = start.checked_add(len_usize).ok_or_else(|| out_of_bounds(off, len))?;
    Ok(start..end)
}

/// Return `data[off..off + len]`, or an error if that range is not in `data`.
pub fn slice(data: &[u8], off: u64, len: u64) -> Result<&[u8]> {
    data.get(range(off, len)?).ok_or_else(|| out_of_bounds(off, len))
}

pub fn slice_mut(data: &mut [u8], off: u64, len: u64) -> Result<&mut [u8]> {
    data.get_mut(range(off, len)?).ok_or_else(|| out_of_bounds(off, len))
}

/// Read a NUL-terminated string starting at `off` (without the NUL).
pub fn cstr(data: &[u8], off: u64) -> Result<&[u8]> {
    let start = usize::try_from(off).map_err(|_| out_of_bounds(off, 1))?;
    let rest = data.get(start..).ok_or_else(|| out_of_bounds(off, 1))?;
    let len = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::new(format!("unterminated string at offset {off:#x}")))?;
    Ok(&rest[..len])
}

impl Ctx {
    pub fn word(self) -> usize {
        if self.is64 {
            8
        } else {
            4
        }
    }
    pub fn ehdr_size(self) -> usize {
        if self.is64 {
            64
        } else {
            52
        }
    }
    pub fn phdr_size(self) -> usize {
        if self.is64 {
            56
        } else {
            32
        }
    }
    pub fn shdr_size(self) -> usize {
        if self.is64 {
            64
        } else {
            40
        }
    }
    pub fn dyn_size(self) -> usize {
        2 * self.word()
    }
    pub fn sym_size(self) -> usize {
        if self.is64 {
            24
        } else {
            16
        }
    }

    pub fn u16(self, data: &[u8], off: u64) -> Result<u16> {
        let b: [u8; 2] = slice(data, off, 2)?.try_into().unwrap();
        Ok(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }
    pub fn u32(self, data: &[u8], off: u64) -> Result<u32> {
        let b: [u8; 4] = slice(data, off, 4)?.try_into().unwrap();
        Ok(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }
    pub fn u64(self, data: &[u8], off: u64) -> Result<u64> {
        let b: [u8; 8] = slice(data, off, 8)?.try_into().unwrap();
        Ok(if self.le { u64::from_le_bytes(b) } else { u64::from_be_bytes(b) })
    }
    /// A class-sized word (address, offset, size, ...).
    pub fn word_at(self, data: &[u8], off: u64) -> Result<u64> {
        if self.is64 {
            self.u64(data, off)
        } else {
            self.u32(data, off).map(u64::from)
        }
    }

    pub fn put_u16(self, data: &mut [u8], off: u64, v: u16) -> Result<()> {
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        slice_mut(data, off, 2)?.copy_from_slice(&b);
        Ok(())
    }
    pub fn put_u32(self, data: &mut [u8], off: u64, v: u32) -> Result<()> {
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        slice_mut(data, off, 4)?.copy_from_slice(&b);
        Ok(())
    }
    pub fn put_u64(self, data: &mut [u8], off: u64, v: u64) -> Result<()> {
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        slice_mut(data, off, 8)?.copy_from_slice(&b);
        Ok(())
    }
    pub fn put_word(self, data: &mut [u8], off: u64, v: u64) -> Result<()> {
        if self.is64 {
            self.put_u64(data, off, v)
        } else {
            let v =
                u32::try_from(v).map_err(|_| Error::new(format!("value {v:#x} does not fit a 32-bit ELF field")))?;
            self.put_u32(data, off, v)
        }
    }
}

/// Sequential reader over a byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: u64,
    ctx: Ctx,
}

impl<'a> Reader<'a> {
    pub fn new(ctx: Ctx, data: &'a [u8], pos: u64) -> Self {
        Reader { data, pos, ctx }
    }
    pub fn u8(&mut self) -> Result<u8> {
        let v = slice(self.data, self.pos, 1)?[0];
        self.pos += 1;
        Ok(v)
    }
    pub fn u16(&mut self) -> Result<u16> {
        let v = self.ctx.u16(self.data, self.pos)?;
        self.pos += 2;
        Ok(v)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let v = self.ctx.u32(self.data, self.pos)?;
        self.pos += 4;
        Ok(v)
    }
    pub fn u64(&mut self) -> Result<u64> {
        let v = self.ctx.u64(self.data, self.pos)?;
        self.pos += 8;
        Ok(v)
    }
    pub fn word(&mut self) -> Result<u64> {
        if self.ctx.is64 {
            self.u64()
        } else {
            self.u32().map(u64::from)
        }
    }
    pub fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let v: [u8; N] = slice(self.data, self.pos, N as u64)?.try_into().unwrap();
        self.pos += N as u64;
        Ok(v)
    }
}

/// Sequential writer into a growing buffer.
pub struct Writer {
    pub buf: Vec<u8>,
    ctx: Ctx,
}

impl Writer {
    pub fn new(ctx: Ctx) -> Self {
        Writer { buf: Vec::new(), ctx }
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        let b = if self.ctx.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.buf.extend_from_slice(&b);
    }
    pub fn u32(&mut self, v: u32) {
        let b = if self.ctx.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.buf.extend_from_slice(&b);
    }
    pub fn u64(&mut self, v: u64) {
        let b = if self.ctx.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.buf.extend_from_slice(&b);
    }
    /// Write a class-sized word. 32-bit values are truncated; callers check
    /// ranges where it matters (addresses always fit by construction).
    pub fn word(&mut self, v: u64) {
        if self.ctx.is64 {
            self.u64(v)
        } else {
            self.u32(v as u32)
        }
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
}

/// Round `v` up to a multiple of `align`, saturating at `u64::MAX`.
pub fn align_up(v: u64, align: u64) -> u64 {
    if align <= 1 {
        v
    } else {
        v.checked_next_multiple_of(align).unwrap_or(u64::MAX)
    }
}

pub fn align_down(v: u64, align: u64) -> u64 {
    if align <= 1 {
        v
    } else {
        v / align * align
    }
}
