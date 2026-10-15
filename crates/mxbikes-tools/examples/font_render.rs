//! Renders a string with a game font at native size: font_render <font> <text> <out.png>
use mxbikes_formats::FontFile;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let f = FontFile::parse(&vfs.read(&a[0]).unwrap()).unwrap();
    let bytes = a[1].as_bytes();
    for &c in bytes {
        println!("{:?} {:?}", c as char, f.glyph(c));
    }
    let w = (f.measure(bytes) + 4) as u32;
    let h = f.size;
    let mut img = image::GrayImage::new(w, h);
    let mut pen = 0;
    for (i, &c) in bytes.iter().enumerate() {
        let Some(g) = f.glyph(c) else { continue };
        for gy in 0..(g.y1 - g.y0) {
            for gx in 0..(g.x1 + 2 - g.x0) {
                let (dx, dy) = (pen + g.a + gx, gy);
                if dx < 0 || dx as u32 >= w || dy as u32 >= h { continue; }
                let v = f.coverage[((g.y0 + gy) as u32 * f.width + (g.x0 + gx) as u32) as usize];
                let p = img.get_pixel_mut(dx as u32, dy as u32);
                p.0[0] = p.0[0].max(v);
            }
        }
        pen += g.a + g.b + g.c;
        if let Some(&n) = bytes.get(i + 1) { pen += f.kern(c, n); }
    }
    img.save(&a[2]).unwrap();
    // Whole atlas for reference.
    image::GrayImage::from_raw(f.width, f.height, f.coverage.clone()).unwrap().save(format!("{}.atlas.png", a[2])).unwrap();
}
