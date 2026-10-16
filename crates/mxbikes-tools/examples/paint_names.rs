//! Prints the texture names a `.pnt` paint carries and the material names of an `.edf` model.
use mxbikes_formats::{EdfModel, PaintFile};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut args = std::env::args().skip(1);
    let pnt = args.next().expect("pnt path");
    let edf = args.next().expect("edf path");
    let p = PaintFile::parse(&vfs.read(&pnt).unwrap()).unwrap();
    for t in &p.textures {
        println!("paint texture '{}' {}x{} flags {:#x} frames {}", t.name, t.width, t.height, t.flags, t.frames.len());
    }
    let m = EdfModel::parse(&vfs.read(&edf).unwrap()).unwrap();
    for (i, mat) in m.materials.iter().enumerate() {
        let layers: Vec<_> = mat.paint_layers.iter().map(|l| l.material.diffuse.name.clone()).collect();
        println!("material {i}: '{}' flags {:#x} shading {} layers {:?}", mat.diffuse.name, mat.diffuse.flags, mat.shading, layers);
    }
}
