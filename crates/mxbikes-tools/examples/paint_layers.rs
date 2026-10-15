//! Shows the base diffuse and the paint layer of each material in a model, and saves both
//! textures as PNG for inspection.
use mxbikes_formats::EdfModel;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let path = std::env::args().nth(1).unwrap_or_else(|| "rider/riders/default_mx/rider.edf".into());
    let out = std::env::args().nth(2);
    let data = vfs.read(&path).unwrap();
    let m = EdfModel::parse(&data).unwrap();
    for (i, mat) in m.materials.iter().enumerate() {
        println!(
            "mat {} '{}' {}x{} flags {:#x} shading {} frames {}",
            i + 1,
            mat.diffuse.name,
            mat.diffuse.width,
            mat.diffuse.height,
            mat.diffuse.flags,
            mat.shading,
            mat.diffuse.frames.len()
        );
        for (k, l) in mat.paint_layers.iter().enumerate() {
            let d = &l.material.diffuse;
            println!(
                "   layer {k}: flags {:#x} color {:?} slots {:?} diffuse '{}' {}x{} flags {:#x} shading {} format {:?}",
                l.flags, l.color, l.slots, d.name, d.width, d.height, d.flags, l.material.shading, d.frames.first().map(|f| f.format)
            );
            if let Some(dir) = &out {
                for (tag, rec) in [("base", &mat.diffuse), ("layer", d)] {
                    if let Ok(img) = rec.decode_frame(&data, 0) {
                        let file = format!("{dir}/{}_{tag}.png", mat.diffuse.name);
                        image::save_buffer(&file, &img.rgba[..(img.width * img.height * 4) as usize], img.width, img.height, image::ColorType::Rgba8).unwrap();
                    }
                }
            }
        }
    }
}
