//! Number plates and suit lettering rasterizer ported from FUN_14003eb50.
//! Renders race numbers onto `w_plate` (bike) and `w_number` (rider),
//! and rider name onto `w_name` (rider suit back).

use mxbikes_formats::assets::read_cfg;
use mxbikes_formats::texture::DecodedImage;
use mxbikes_vfs::MxBikesVfs;
use std::collections::HashMap;

/// Decodes a truecolor TGA into width, height and RGBA bytes.
pub fn decode_tga_rgba(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    mxbikes_formats::tga::decode_tga(data).map(|i| (i.width, i.height, i.rgba))
}

struct FontSpec {
    glyphs: Vec<u8>,
    width: u32,
    height: u32,
    fontstring: String,
    spacing: u32,
    posx: i32,
    posy: i32,
    tracking: i32,
}

impl FontSpec {
    fn load(vfs: &MxBikesVfs, font_dir: &str, block: &str) -> Option<Self> {
        let cfg = read_cfg(vfs, &format!("{font_dir}/gfx.cfg"))?;
        let sec = cfg.get_child(block)?;
        let font_file = sec.get_str("font")?;
        let fontstring = sec.get_str("fontstring")?.to_string();
        let spacing = sec.get_f32("spacing").unwrap_or(85.0) as u32;
        let posx = sec.get_f32("posx").unwrap_or(0.0) as i32;
        let posy = sec.get_f32("posy").unwrap_or(0.0) as i32;
        let tracking = sec.get_f32("tracking").unwrap_or(0.0) as i32;

        let tga_data = vfs.read(&format!("{font_dir}/{font_file}")).ok()?;
        let (width, height, glyphs) = decode_tga_rgba(&tga_data)?;
        Some(Self {
            glyphs,
            width,
            height,
            fontstring,
            spacing,
            posx,
            posy,
            tracking,
        })
    }

    /// Blits string onto canvas using the centered placement algorithm from FUN_14003eb50.
    fn render_string(&self, canvas: &mut [u8], canvas_w: u32, canvas_h: u32, text: &str, max_chars: i32) {
        let len = text.len() as i32;
        // FUN_14003eb50 lines 75 & 137: iVar9 = ((tracking + spacing) * (max_chars - len)) / 2 + posx
        let step = (self.spacing as i32 + self.tracking).max(1);
        let mut cur_x = (step * (max_chars - len)) / 2 + self.posx;

        for ch in text.chars() {
            let ch_lower = ch.to_ascii_lowercase();
            if let Some(idx) = self.fontstring.find(ch_lower) {
                let src_x0 = idx as u32 * self.spacing;
                for gy in 0..self.height {
                    let ty = self.posy + gy as i32;
                    if ty < 0 || ty >= canvas_h as i32 {
                        continue;
                    }
                    for gx in 0..self.spacing {
                        let tx = cur_x + gx as i32;
                        if tx < 0 || tx >= canvas_w as i32 {
                            continue;
                        }
                        let s_idx = ((gy * self.width + (src_x0 + gx)) * 4) as usize;
                        let d_idx = ((ty as u32 * canvas_w + tx as u32) * 4) as usize;
                        let sa = self.glyphs[s_idx + 3] as u32;
                        if sa == 0 {
                            continue;
                        }
                        let sr = self.glyphs[s_idx] as u32;
                        let sg = self.glyphs[s_idx + 1] as u32;
                        let sb = self.glyphs[s_idx + 2] as u32;

                        let da = canvas[d_idx + 3] as u32;
                        let inv_sa = 255 - sa;
                        canvas[d_idx] = ((sr * sa + canvas[d_idx] as u32 * inv_sa) / 255) as u8;
                        canvas[d_idx + 1] = ((sg * sa + canvas[d_idx + 1] as u32 * inv_sa) / 255) as u8;
                        canvas[d_idx + 2] = ((sb * sa + canvas[d_idx + 2] as u32 * inv_sa) / 255) as u8;
                        canvas[d_idx + 3] = (sa + da * inv_sa / 255).min(255) as u8;
                    }
                }
            }
            cur_x += step;
        }
    }
}

/// Renders textures `w_plate` (256x128), `w_number` (256x256), and `w_name` (256x64).
pub fn render_plates(
    vfs: &MxBikesVfs,
    font_name: &str,
    number: &str,
    name: &str,
) -> HashMap<String, DecodedImage> {
    let font_dir = format!("rider/fonts/{font_name}");
    let mut out = HashMap::new();

    if let Some(font) = FontSpec::load(vfs, &font_dir, "number") {
        // w_plate: 256x128
        let mut plate_rgba = vec![0u8; 256 * 128 * 4];
        font.render_string(&mut plate_rgba, 256, 128, number, 3);
        out.insert("w_plate".to_string(), DecodedImage {
            width: 256,
            height: 128,
            layers: 1,
            rgba: plate_rgba,
        });

        // w_number: 256x256
        let mut num_rgba = vec![0u8; 256 * 256 * 4];
        font.render_string(&mut num_rgba, 256, 256, number, 3);
        out.insert("w_number".to_string(), DecodedImage {
            width: 256,
            height: 256,
            layers: 1,
            rgba: num_rgba,
        });
    }

    if let Some(font) = FontSpec::load(vfs, &font_dir, "name") {
        // w_name: 256x64
        let mut name_rgba = vec![0u8; 256 * 64 * 4];
        font.render_string(&mut name_rgba, 256, 64, name, 8);
        out.insert("w_name".to_string(), DecodedImage {
            width: 256,
            height: 64,
            layers: 1,
            rgba: name_rgba,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tga_empty_is_none() {
        assert!(decode_tga_rgba(&[]).is_none());
    }
}
