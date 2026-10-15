//! PiBoSo bitmap fonts (`ui/*.fnt`), layout from the game's loader `FUN_140208e70`:
//!
//!   "FNT\0" name[260] size:u32
//!   256 x { exists:u32 a:i32 b:i32 c:i32 x0:i32 x1:i32 y0:i32 y1:i32 kern_start:i32 kern_count:i32 }
//!   kern_count:u32 { second_char:i32 amount:i32 } * kern_count
//!   width:u32 height:u32 blob_size:u32 Blob   (texture blob: format 2 = L8 coverage, deflated)
//!
//! Glyphs use Windows ABC widths: the pen moves `a`, draws `b` pixels, then moves `c`. The game's
//! UVs are `x0 / W .. (x1 + 2) / W` and `y0 / H .. y1 / H` (the 2 px pad holds anti-aliasing).

use crate::reader::{ReadError, Reader};
use crate::texture::{DecodedImage, PixelFormat, TextureBlob};

#[derive(Debug, Clone, Copy, Default)]
pub struct Glyph {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub x0: i32,
    pub x1: i32,
    pub y0: i32,
    pub y1: i32,
    pub kern_start: i32,
    pub kern_count: i32,
}

#[derive(Debug, Clone)]
pub struct FontFile {
    pub name: String,
    /// Line height in texels.
    pub size: u32,
    pub glyphs: Vec<Option<Glyph>>,
    pub kerning: Vec<(u32, i32)>,
    pub width: u32,
    pub height: u32,
    /// Coverage 0..255, row-major, `width * height`.
    pub coverage: Vec<u8>,
}

impl FontFile {
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"FNT\0" {
            return Err(ReadError::Magic { expected: *b"FNT\0", found: magic });
        }
        let name = r.name(260)?;
        let size = r.u32()?;
        let mut glyphs = Vec::with_capacity(256);
        for _ in 0..256 {
            let v = r.u32s(10)?;
            let i = |k: usize| v[k] as i32;
            glyphs.push((v[0] != 0).then(|| Glyph {
                a: i(1),
                b: i(2),
                c: i(3),
                x0: i(4),
                x1: i(5),
                y0: i(6),
                y1: i(7),
                kern_start: i(8),
                kern_count: i(9),
            }));
        }
        let kern_count = r.bounded_count("font kerning pairs", 65536)?;
        let kerning = (0..kern_count).map(|_| Ok((r.u32()?, r.u32()? as i32))).collect::<Result<Vec<_>, ReadError>>()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let at = r.pos();
        let blob_size = r.bounded_count("font bitmap size", 64 << 20)?;
        let offset = r.pos();
        let format = PixelFormat::from_code(r.peek_u32()?);
        r.skip(blob_size)?;
        let blob = TextureBlob { offset, size: blob_size, format };
        let img: DecodedImage = blob.decode(data, width, height).map_err(|_| ReadError::Implausible {
            what: "font bitmap",
            value: blob_size as u64,
            offset: at,
        })?;
        let coverage = img.rgba.chunks_exact(4).map(|p| p[0]).take((width * height) as usize).collect();
        if r.remaining() != 0 {
            return Err(ReadError::Implausible { what: "trailing font bytes", value: r.remaining() as u64, offset: r.pos() });
        }
        Ok(Self { name, size, glyphs, kerning, width, height, coverage })
    }

    pub fn glyph(&self, ch: u8) -> Option<&Glyph> {
        self.glyphs.get(ch as usize)?.as_ref()
    }

    /// Kerning adjustment between `first` and the following character.
    pub fn kern(&self, first: u8, second: u8) -> i32 {
        let Some(g) = self.glyph(first) else { return 0 };
        let start = g.kern_start.max(0) as usize;
        let end = (start + g.kern_count.max(0) as usize).min(self.kerning.len());
        self.kerning[start.min(end)..end].iter().find(|(c, _)| *c == second as u32).map_or(0, |(_, k)| *k)
    }

    /// Pen advance in texels for a string (Latin-1 bytes), including kerning.
    pub fn measure(&self, text: &[u8]) -> i32 {
        let mut w = 0;
        for (i, &ch) in text.iter().enumerate() {
            if let Some(g) = self.glyph(ch) {
                w += g.a + g.b + g.c;
            }
            if let Some(&next) = text.get(i + 1) {
                w += self.kern(ch, next);
            }
        }
        w
    }
}
