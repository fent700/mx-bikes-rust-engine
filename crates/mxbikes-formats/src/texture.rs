//! Embedded texture records and material tables shared by `.edf` meshes and `.map` tracks.
//!
//! Image record (diffuse / paint layers, "full"):
//!   name[100] flags:u32 width:u32 height:u32 hash[16] extra:u32 hash[16]*extra blob*(extra+1)
//!   (the base frame blob may be empty when only the extra frames carry texels)
//! Image record (reflection / specular / env, "short"):
//!   name[100] width:u32 height:u32 hash[16] blob
//!   With shading mode 2 the reflection and specular maps carry the frame list like a full
//!   record (without flags); the env map never does.
//! Normal-map records are "full" without the flags word.
//! Blob: size:u32 { format:u32 reserved:u32 raw-deflate[size - 8] }

use crate::reader::{ReadError, Reader};
use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit RGBA, 4 bytes per texel.
    Rgba8,
    /// Single channel 8-bit (reflection / specular maps, terrain blend masks).
    L8,
    /// BC3 / DXT5, 1 byte per texel (not used by stock content, kept for mods).
    Bc3,
    /// BC1 / DXT1, 0.5 bytes per texel.
    Bc1,
    /// Six RGB faces (+X,-X,+Y,-Y,+Z,-Z), 3 bytes per texel.
    CubeRgb8,
    Unknown(u32),
}

impl PixelFormat {
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => PixelFormat::Rgba8,
            1 => PixelFormat::Bc1,
            2 => PixelFormat::L8,
            5 => PixelFormat::CubeRgb8,
            other => PixelFormat::Unknown(other),
        }
    }
}

/// One compressed texel payload inside an image record. Decompression is deferred until use.
#[derive(Debug, Clone)]
pub struct TextureBlob {
    pub offset: usize,
    pub size: usize,
    pub format: PixelFormat,
}

#[derive(Debug, Clone)]
pub struct ImageRecord {
    pub name: String,
    pub flags: u32,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<TextureBlob>,
}

impl ImageRecord {
    pub const FLAG_ALPHA_TEST: u32 = 0x1;
    pub const FLAG_PAINTABLE: u32 = 0x2;

    pub fn has_alpha_test(&self) -> bool {
        self.flags & (Self::FLAG_ALPHA_TEST | 0x100 | 0x400) != 0
    }
}

#[derive(Debug, Clone)]
pub struct Reflection {
    pub params: [f32; 3],
    pub map: Option<ImageRecord>,
    pub env: Option<ImageRecord>,
}

#[derive(Debug, Clone)]
pub struct Specular {
    pub power: f32,
    pub map: Option<ImageRecord>,
}

#[derive(Debug, Clone)]
pub struct NormalMap {
    pub strength: f32,
    pub map: ImageRecord,
}

#[derive(Debug, Clone)]
pub struct PaintLayer {
    /// Header in the same 56-byte shape as an object material slot: flags, colour, slots.
    pub flags: u32,
    pub color: [f32; 6],
    pub slots: [u32; 7],
    /// Each layer is a complete material (diffuse plus its own shading block).
    pub material: Box<Material>,
}

/// Additional parameters carried by shading mode 2 materials (number plates, decals).
#[derive(Debug, Clone, Copy)]
pub struct ShadingExtra {
    pub pair: Option<[f32; 3]>,
    pub mode: u32,
}

#[derive(Debug, Clone)]
pub struct Material {
    pub diffuse: ImageRecord,
    pub shading: u32,
    pub reflection: Option<Reflection>,
    pub specular: Option<Specular>,
    pub normal: Option<NormalMap>,
    pub extra: Option<ShadingExtra>,
    pub paint_layers: Vec<PaintLayer>,
}

const MAX_DIM: u32 = 16384;

fn read_blob(r: &mut Reader) -> Result<TextureBlob, ReadError> {
    let at = r.pos();
    let size = r.u32()? as usize;
    if size == 0 {
        // Empty slot: the texture is not embedded.
        return Ok(TextureBlob {
            offset: at,
            size: 0,
            format: PixelFormat::Unknown(0),
        });
    }
    if size < 8 || size > r.remaining() {
        return Err(ReadError::Implausible {
            what: "texture blob size",
            value: size as u64,
            offset: at,
        });
    }
    let format = PixelFormat::from_code(r.peek_u32()?);
    let offset = r.pos();
    r.skip(size)?;
    Ok(TextureBlob {
        offset,
        size,
        format,
    })
}

fn read_dims(r: &mut Reader) -> Result<(u32, u32), ReadError> {
    let at = r.pos();
    let w = r.u32()?;
    let h = r.u32()?;
    if w > MAX_DIM || h > MAX_DIM || (w == 0) != (h == 0) {
        return Err(ReadError::Implausible {
            what: "texture dimensions",
            value: ((w as u64) << 32) | h as u64,
            offset: at,
        });
    }
    Ok((w, h))
}

pub fn read_full_image(r: &mut Reader, with_flags: bool) -> Result<ImageRecord, ReadError> {
    let name = r.name(100)?;
    let flags = if with_flags { r.u32()? } else { 0 };
    let (width, height) = read_dims(r)?;
    r.skip(16)?;
    // `extra` additional frames follow the base frame; each frame has a hash and a blob,
    // and the base frame blob may be empty (animated or layered textures).
    let extra = r.bounded_count("texture frame count", 1024)?;
    r.skip(16 * extra)?;
    let mut frames = Vec::with_capacity(extra + 1);
    for _ in 0..=extra {
        frames.push(read_blob(r)?);
    }
    if frames.len() > 1 && frames[0].size == 0 {
        frames.remove(0);
    }
    Ok(ImageRecord {
        name,
        flags,
        width,
        height,
        frames,
    })
}

pub fn read_short_image(r: &mut Reader) -> Result<ImageRecord, ReadError> {
    let name = r.name(100)?;
    let (width, height) = read_dims(r)?;
    r.skip(16)?;
    let blob = read_blob(r)?;
    Ok(ImageRecord {
        name,
        flags: 0,
        width,
        height,
        frames: vec![blob],
    })
}

fn read_map_image(r: &mut Reader, layered: bool) -> Result<ImageRecord, ReadError> {
    if layered {
        read_full_image(r, false)
    } else {
        read_short_image(r)
    }
}

fn optional<T>(
    r: &mut Reader,
    f: impl FnOnce(&mut Reader) -> Result<T, ReadError>,
) -> Result<Option<T>, ReadError> {
    if r.u32()? != 0 {
        Ok(Some(f(r)?))
    } else {
        Ok(None)
    }
}

/// Reads one material entry. Object tables follow each entry with `count:u32 { flags:u32 color:f32*6
/// slots:u32*7 Material }` paint layers; terrain layer variants have none.
pub fn read_material(r: &mut Reader, with_paint_layers: bool) -> Result<Material, ReadError> {
    let diffuse = read_full_image(r, true)?;
    let at = r.pos();
    let shading = r.u32()?;
    if shading > 3 {
        return Err(ReadError::Implausible {
            what: "material shading mode",
            value: shading as u64,
            offset: at,
        });
    }
    let (mut reflection, mut specular, mut normal) = (None, None, None);
    if shading != 0 {
        let layered = shading == 2;
        reflection = optional(r, |r| {
            let p = r.f32s(3)?;
            let map = optional(r, |r| read_map_image(r, layered))?;
            let env = optional(r, read_short_image)?;
            Ok(Reflection {
                params: [p[0], p[1], p[2]],
                map,
                env,
            })
        })?;
        specular = optional(r, |r| {
            let power = r.f32()?;
            let map = optional(r, |r| read_map_image(r, layered))?;
            Ok(Specular { power, map })
        })?;
        normal = optional(r, |r| {
            let strength = r.f32()?;
            let map = read_full_image(r, false)?;
            Ok(NormalMap { strength, map })
        })?;
    }
    let extra = if shading == 2 {
        let pair = optional(r, |r| Ok([r.f32()?, r.f32()?, r.f32()?]))?;
        let mode = r.u32()?;
        Some(ShadingExtra { pair, mode })
    } else {
        None
    };
    let mut paint_layers = Vec::new();
    if with_paint_layers {
        let count = r.bounded_count("paint layer count", 64)?;
        for _ in 0..count {
            let flags = r.u32()?;
            let c = r.f32s(6)?;
            let slots = r.u32s(7)?;
            let material = read_material(r, false)?;
            paint_layers.push(PaintLayer {
                flags,
                color: [c[0], c[1], c[2], c[3], c[4], c[5]],
                slots: slots.try_into().unwrap(),
                material: Box::new(material),
            });
        }
    }
    Ok(Material {
        diffuse,
        shading,
        reflection,
        specular,
        normal,
        extra,
        paint_layers,
    })
}

pub fn read_material_table(r: &mut Reader) -> Result<Vec<Material>, ReadError> {
    let count = r.bounded_count("material count", 4096)?;
    (0..count).map(|_| read_material(r, true)).collect()
}

/// Decoded texel data, always expanded to RGBA8.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Number of array layers (6 for cubemaps).
    pub layers: u32,
    pub rgba: Vec<u8>,
}

impl TextureBlob {
    /// Inflates the raw-deflate payload of this blob.
    pub fn inflate(&self, file: &[u8]) -> Result<Vec<u8>, ReadError> {
        if self.size < 8 {
            return Err(ReadError::Implausible {
                what: "empty texture slot",
                value: 0,
                offset: self.offset,
            });
        }
        let body = &file[self.offset + 8..self.offset + self.size];
        let mut out = Vec::new();
        flate2::read::DeflateDecoder::new(body)
            .read_to_end(&mut out)
            .map_err(|e| ReadError::Inflate {
                offset: self.offset,
                message: e.to_string(),
            })?;
        Ok(out)
    }

    pub fn decode(&self, file: &[u8], width: u32, height: u32) -> Result<DecodedImage, ReadError> {
        let raw = self.inflate(file)?;
        let texels = (width * height) as usize;
        let (layers, rgba) = match self.format {
            PixelFormat::Rgba8 if raw.len() >= texels * 4 => (1, raw[..texels * 4].to_vec()),
            PixelFormat::L8 if raw.len() >= texels => {
                let mut out = Vec::with_capacity(texels * 4);
                for &l in &raw[..texels] {
                    out.extend_from_slice(&[l, l, l, 255]);
                }
                (1, out)
            }
            PixelFormat::Bc3 if raw.len() >= texels => (1, decode_bc3(&raw, width, height)),
            PixelFormat::Bc1 if raw.len() >= texels / 2 => (1, decode_bc1(&raw, width, height)),
            PixelFormat::CubeRgb8 if raw.len() >= texels * 18 => {
                let mut out = Vec::with_capacity(texels * 24);
                for px in raw[..texels * 18].chunks_exact(3) {
                    out.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                (6, out)
            }
            _ => {
                // Fall back on payload size when the format code is unfamiliar.
                if raw.len() == texels * 4 {
                    (1, raw)
                } else if raw.len() == texels {
                    (1, raw.iter().flat_map(|&l| [l, l, l, 255]).collect())
                } else if raw.len() * 2 == texels {
                    (1, decode_bc1(&raw, width, height))
                } else {
                    return Err(ReadError::Implausible {
                        what: "texture payload size",
                        value: raw.len() as u64,
                        offset: self.offset,
                    });
                }
            }
        };
        Ok(DecodedImage {
            width,
            height,
            layers,
            rgba,
        })
    }
}

impl ImageRecord {
    pub fn decode_frame(&self, file: &[u8], frame: usize) -> Result<DecodedImage, ReadError> {
        self.frames[frame.min(self.frames.len() - 1)].decode(file, self.width, self.height)
    }
}

#[inline]
fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [
        ((r * 527 + 23) >> 6) as u8,
        ((g * 259 + 33) >> 6) as u8,
        ((b * 527 + 23) >> 6) as u8,
    ]
}

fn color_block(block: &[u8], out: &mut [[u8; 4]; 16], force_four: bool) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mut pal = [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], [0; 4], [0; 4]];
    let lerp = |x: u8, y: u8, wx: u32, wy: u32, d: u32| ((x as u32 * wx + y as u32 * wy) / d) as u8;
    if c0 > c1 || force_four {
        for i in 0..3 {
            pal[2][i] = lerp(a[i], b[i], 2, 1, 3);
            pal[3][i] = lerp(a[i], b[i], 1, 2, 3);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for i in 0..3 {
            pal[2][i] = lerp(a[i], b[i], 1, 1, 2);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

fn scatter(out: &mut [u8], w: u32, h: u32, bx: u32, by: u32, px: &[[u8; 4]; 16]) {
    for y in 0..4 {
        for x in 0..4 {
            let (tx, ty) = (bx * 4 + x, by * 4 + y);
            if tx < w && ty < h {
                let o = ((ty * w + tx) * 4) as usize;
                out[o..o + 4].copy_from_slice(&px[(y * 4 + x) as usize]);
            }
        }
    }
}

pub fn decode_bc1(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut px = [[0u8; 4]; 16];
    for by in 0..bh {
        for bx in 0..bw {
            let o = ((by * bw + bx) * 8) as usize;
            if o + 8 > data.len() {
                return out;
            }
            color_block(&data[o..o + 8], &mut px, false);
            scatter(&mut out, w, h, bx, by, &px);
        }
    }
    out
}

pub fn decode_bc3(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut px = [[0u8; 4]; 16];
    for by in 0..bh {
        for bx in 0..bw {
            let o = ((by * bw + bx) * 16) as usize;
            if o + 16 > data.len() {
                return out;
            }
            let blk = &data[o..o + 16];
            let (a0, a1) = (blk[0] as u32, blk[1] as u32);
            let mut alpha = [0u8; 8];
            alpha[0] = a0 as u8;
            alpha[1] = a1 as u8;
            if a0 > a1 {
                for i in 1..7u32 {
                    alpha[(i + 1) as usize] = (((7 - i) * a0 + i * a1) / 7) as u8;
                }
            } else {
                for i in 1..5u32 {
                    alpha[(i + 1) as usize] = (((5 - i) * a0 + i * a1) / 5) as u8;
                }
                alpha[6] = 0;
                alpha[7] = 255;
            }
            let abits = u64::from_le_bytes([blk[2], blk[3], blk[4], blk[5], blk[6], blk[7], 0, 0]);
            color_block(&blk[8..16], &mut px, true);
            for (i, p) in px.iter_mut().enumerate() {
                p[3] = alpha[((abits >> (3 * i)) & 7) as usize];
            }
            scatter(&mut out, w, h, bx, by, &px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_solid_block() {
        // c0 = pure red (0xF800), c1 = black, all indices 0.
        let block = [0x00, 0xF8, 0x00, 0x00, 0, 0, 0, 0];
        let px = decode_bc1(&block, 4, 4);
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
        assert_eq!(&px[60..64], &[255, 0, 0, 255]);
    }

    #[test]
    fn bc3_alpha_endpoints() {
        let mut block = [0u8; 16];
        block[0] = 200;
        block[1] = 10;
        // indices all 1 -> alpha1
        let bits: u64 = (0..16).fold(0, |acc, i| acc | (1u64 << (3 * i)));
        block[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
        block[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let px = decode_bc3(&block, 4, 4);
        assert_eq!(px[3], 10);
        assert_eq!(&px[0..3], &[255, 255, 255]);
    }
}
