//! Prints the frame count of every paint (mud) layer texture in a model.
use mxbikes_formats::EdfModel;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    for path in std::env::args().skip(1) {
        let m = EdfModel::parse(&vfs.read(&path).unwrap()).unwrap();
        for mat in &m.materials {
            for l in &mat.paint_layers {
                let d = &l.material.diffuse;
                println!("{path}: '{}' layer frames {} sizes {:?}", mat.diffuse.name, d.frames.len(), d.frames.iter().map(|f| f.size).collect::<Vec<_>>());
            }
        }
    }
}
