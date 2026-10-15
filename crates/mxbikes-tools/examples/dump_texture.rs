//! Saves one named diffuse texture from a model as PNG and prints its alpha coverage.
use mxbikes_formats::EdfModel;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let mut args = std::env::args().skip(1);
    let (path, name, out) = (args.next().unwrap(), args.next().unwrap(), args.next().unwrap());
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let data = vfs.read(&path).unwrap();
    let m = EdfModel::parse(&data).unwrap();
    let mat = m.materials.iter().find(|x| x.diffuse.name.eq_ignore_ascii_case(&name)).expect("texture");
    let img = mat.diffuse.decode_frame(&data, 0).unwrap();
    let px = &img.rgba[..(img.width * img.height * 4) as usize];
    let opaque = px.chunks_exact(4).filter(|p| p[3] > 8).count();
    println!("{name}: {}x{} opaque {opaque} of {}", img.width, img.height, img.width * img.height);
    image::save_buffer(&out, px, img.width, img.height, image::ColorType::Rgba8).unwrap();
}
