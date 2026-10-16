//! Writes the diffuse (and first paint layer) of each material of an `.edf` as small PNGs.
use mxbikes_formats::{DecodedImage, EdfModel, ImageRecord};
use mxbikes_vfs::MxBikesVfs;

fn save(img: &DecodedImage, path: &str) {
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
    image::imageops::resize(&rgb, 512, 256, image::imageops::FilterType::Triangle).save(path).unwrap();
}

fn dump(data: &[u8], r: &ImageRecord, path: &str) {
    for f in 0..r.frames.len().max(1) {
        match r.decode_frame(data, f) {
            Ok(img) if img.width > 0 => {
                save(&img, &format!("{path}_f{f}.png"));
                println!("{path} frame {f} {}x{} flags {:#x}", img.width, img.height, r.flags);
            }
            Ok(_) => println!("{path} frame {f} empty"),
            Err(e) => println!("{path} frame {f} error {e}"),
        }
    }
}

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut args = std::env::args().skip(1);
    let edf = args.next().expect("edf path");
    let out = args.next().expect("output dir");
    let data = vfs.read(&edf).unwrap();
    let m = EdfModel::parse(&data).unwrap();
    for (i, mat) in m.materials.iter().enumerate() {
        dump(&data, &mat.diffuse, &format!("{out}/m{i}_{}", mat.diffuse.name));
        for (j, l) in mat.paint_layers.iter().enumerate() {
            dump(&data, &l.material.diffuse, &format!("{out}/m{i}_layer{j}"));
        }
    }
}
