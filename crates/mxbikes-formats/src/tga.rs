//! Truecolor TGA images (UI sprites, fonts for plates, particle textures): uncompressed (type 2)
//! and run-length encoded (type 10), 16 (ARGB1555), 24 or 32 bits per pixel, either row order.

use crate::texture::DecodedImage;

pub fn decode_tga(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 18 {
        return None;
    }
    let id_len = data[0] as usize;
    let image_type = data[2];
    let width = u16::from_le_bytes([data[12], data[13]]) as u32;
    let height = u16::from_le_bytes([data[14], data[15]]) as u32;
    let bpp = data[16] as usize / 8;
    let top_down = data[17] & 0x20 != 0;
    let alpha_bits = data[17] & 0x0f;
    if !matches!(image_type, 2 | 10) || !matches!(bpp, 2..=4) || width == 0 || height == 0 {
        return None;
    }
    let colormap = if data[1] != 0 {
        u16::from_le_bytes([data[5], data[6]]) as usize * (data[7] as usize).div_ceil(8)
    } else {
        0
    };
    let mut src = data.get(18 + id_len + colormap..)?;
    let count = (width * height) as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    let mut push = |p: &[u8]| {
        if bpp == 2 {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let c = |shift: u16| (((v >> shift) & 31) as u32 * 255 / 31) as u8;
            let a = if alpha_bits == 0 || v & 0x8000 != 0 { 255 } else { 0 };
            pixels.extend_from_slice(&[c(10), c(5), c(0), a]);
        } else {
            pixels.extend_from_slice(&[p[2], p[1], p[0], if bpp == 4 { p[3] } else { 255 }]);
        }
    };
    if image_type == 2 {
        for p in src.get(..count * bpp)?.chunks_exact(bpp) {
            push(p);
        }
    } else {
        let mut done = 0;
        while done < count {
            let (&header, rest) = src.split_first()?;
            let run = (header & 0x7f) as usize + 1;
            if header & 0x80 != 0 {
                let p = rest.get(..bpp)?;
                for _ in 0..run {
                    push(p);
                }
                src = &rest[bpp..];
            } else {
                for p in rest.get(..run * bpp)?.chunks_exact(bpp) {
                    push(p);
                }
                src = &rest[run * bpp..];
            }
            done += run;
        }
        pixels.truncate(count * 4);
    }
    let row = width as usize * 4;
    let rgba = if top_down {
        pixels
    } else {
        pixels.chunks_exact(row).rev().flatten().copied().collect()
    };
    Some(DecodedImage { width, height, layers: 1, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, w: u16, h: u16, bpp: u8, desc: u8) -> Vec<u8> {
        let mut v = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&[bpp, desc]);
        v
    }

    #[test]
    fn rle_and_raw_agree() {
        // 2x1, bottom-up, 32 bpp: blue then red (BGRA in the file).
        let mut raw = header(2, 2, 1, 32, 0);
        raw.extend_from_slice(&[255, 0, 0, 255, 0, 0, 255, 128]);
        let mut rle = header(10, 2, 1, 32, 0);
        rle.extend_from_slice(&[0x01, 255, 0, 0, 255, 0, 0, 255, 128]);
        let a = decode_tga(&raw).unwrap();
        let b = decode_tga(&rle).unwrap();
        assert_eq!(a.rgba, vec![0, 0, 255, 255, 255, 0, 0, 128]);
        assert_eq!(a.rgba, b.rgba);
    }

    #[test]
    fn rle_run_packet() {
        let mut rle = header(10, 3, 1, 24, 0x20);
        rle.extend_from_slice(&[0x82, 10, 20, 30]);
        assert_eq!(decode_tga(&rle).unwrap().rgba, [30, 20, 10, 255].repeat(3));
    }
}
