//! Little-endian cursor over PiBoSo binary assets.

use glam::{Mat4, Vec3};
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ReadError {
    #[error("unexpected end of data at offset {offset:#x} (wanted {wanted} bytes, {available} available)")]
    Eof {
        offset: usize,
        wanted: usize,
        available: usize,
    },
    #[error("invalid magic at offset 0: expected {expected:?}, found {found:?}")]
    Magic { expected: [u8; 4], found: [u8; 4] },
    #[error("implausible {what} = {value} at offset {offset:#x}")]
    Implausible {
        what: &'static str,
        value: u64,
        offset: usize,
    },
    #[error("texture blob decompression failed at offset {offset:#x}: {message}")]
    Inflate { offset: usize, message: String },
}

#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    #[inline]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], ReadError> {
        if self.remaining() < n {
            return Err(ReadError::Eof {
                offset: self.pos,
                wanted: n,
                available: self.remaining(),
            });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), ReadError> {
        self.bytes(n).map(|_| ())
    }

    pub fn u32(&mut self) -> Result<u32, ReadError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn u16(&mut self) -> Result<u16, ReadError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32, ReadError> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn peek_u32(&self) -> Result<u32, ReadError> {
        self.clone().u32()
    }

    pub fn vec3(&mut self) -> Result<Vec3, ReadError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    /// Reads `n` little-endian f32 values.
    pub fn f32s(&mut self, n: usize) -> Result<Vec<f32>, ReadError> {
        let raw = self.bytes(n * 4)?;
        Ok(raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }

    pub fn u32s(&mut self, n: usize) -> Result<Vec<u32>, ReadError> {
        let raw = self.bytes(n * 4)?;
        Ok(raw
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }

    /// Fixed-size, NUL-terminated name field (PiBoSo pads unused bytes with 0x00 or 0xCD).
    pub fn name(&mut self, len: usize) -> Result<String, ReadError> {
        let raw = self.bytes(len)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(len);
        Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
    }

    /// Row-major 4x4 matrix with translation in elements 3, 7 and 11.
    pub fn mat4_row_major(&mut self) -> Result<Mat4, ReadError> {
        let m = self.f32s(16)?;
        Ok(Mat4::from_cols_array(&[
            m[0], m[4], m[8], m[12], m[1], m[5], m[9], m[13], m[2], m[6], m[10], m[14], m[3],
            m[7], m[11], m[15],
        ]))
    }

    pub fn bounded_count(&mut self, what: &'static str, max: u32) -> Result<usize, ReadError> {
        let offset = self.pos;
        let v = self.u32()?;
        if v > max {
            return Err(ReadError::Implausible {
                what,
                value: v as u64,
                offset,
            });
        }
        Ok(v as usize)
    }
}

/// PiBoSo assets are authored in a left-handed frame (+X right, +Y up, +Z forward).
/// The engine runs right-handed with -Z forward, so Z is mirrored at load time.
#[inline]
pub fn lh_to_rh(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, -v.z)
}

/// Converts a left-handed affine transform into the right-handed frame: F * M * F with F = diag(1,1,-1).
pub fn lh_to_rh_mat(m: Mat4) -> Mat4 {
    let f = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
    f * m * f
}
