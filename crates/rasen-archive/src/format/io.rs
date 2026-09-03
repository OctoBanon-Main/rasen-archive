use std::io::Read;

use crate::error::{Error, Result};

pub(crate) fn align_up(value: u64, alignment: u64) -> Result<u64> {
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|v| v & !mask)
        .ok_or(Error::TooLarge("aligned offset"))
}

pub(crate) fn read_u16<R: Read>(r: &mut R) -> Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

pub(crate) fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

pub(crate) fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

pub(crate) fn usize_to_u32(v: usize, what: &'static str) -> Result<u32> {
    u32::try_from(v).map_err(|_| Error::TooLarge(what))
}

pub(crate) fn usize_to_u64(v: usize, what: &'static str) -> Result<u64> {
    u64::try_from(v).map_err(|_| Error::TooLarge(what))
}

pub(crate) fn u64_to_usize(v: u64, what: &'static str) -> Result<usize> {
    usize::try_from(v).map_err(|_| Error::TooLarge(what))
}

pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(Error::Corrupt("TOC cursor overflow"))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or(Error::Corrupt("truncated TOC"))?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        let b: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| Error::Corrupt("truncated TOC"))?;
        Ok(u16::from_le_bytes(b))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| Error::Corrupt("truncated TOC"))?;
        Ok(u32::from_le_bytes(b))
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        let b: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| Error::Corrupt("truncated TOC"))?;
        Ok(u64::from_le_bytes(b))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pos == self.data.len()
    }
}
