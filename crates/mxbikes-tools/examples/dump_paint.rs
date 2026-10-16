//! Writes each texture of a `.pnt` paint as a PNG (RGB and alpha side by side).
use mxbikes_formats::PaintFile;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut args = std::env::args().skip(1);
    let pnt = args.next().expect("pnt path");
    let out = args.next().expect("output dir");
    let data = vfs.read(&pnt).unwrap();
    let p = PaintFile::parse(&data).unwrap();
    for t in &p.textures {
        let img = t.decode_frame(&data, 0).unwrap();
        let (w, h) = (img.width, img.height);
        let mut rgb = image::RgbImage::new(w * 2, h);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let px = &img.rgba[i..i + 4];
                rgb.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
                rgb.put_pixel(w + x, y, image::Rgb([px[3], px[3], px[3]]));
            }
        }
        let small = image::imageops::resize(&rgb, 512, 256, image::imageops::FilterType::Triangle);
        small.save(format!("{out}/{}.png", t.name)).unwrap();
        println!("{} {}x{}", t.name, w, h);
    }
}
