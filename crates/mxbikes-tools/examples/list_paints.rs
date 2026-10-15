//! Lists every `.pnt` with its replacement texture names and sizes, and the matching diffuse
//! names in the models next to it.
use mxbikes_formats::{EdfModel, PaintFile};
use mxbikes_vfs::MxBikesVfs;
use std::collections::BTreeSet;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut files = vfs.list_files("");
    files.sort();
    for path in files.iter().filter(|p| p.ends_with(".pnt")) {
        let p = PaintFile::parse(&vfs.read(path).unwrap()).unwrap();
        let tex: Vec<String> = p.textures.iter().map(|t| format!("{} {}x{}", t.name, t.width, t.height)).collect();
        println!("{path:55} '{}' [{}]", p.name, tex.join(", "));
    }
    for owner in ["bikes/250xf/", "rider/riders/default_mx/", "rider/helmets/default/", "rider/boots/default/"] {
        let mut names = BTreeSet::new();
        for path in files.iter().filter(|p| p.starts_with(owner) && p.ends_with(".edf")) {
            if let Ok(m) = EdfModel::parse(&vfs.read(path).unwrap()) {
                for mat in &m.materials {
                    names.insert(format!("{} ({} paint layers)", mat.diffuse.name, mat.paint_layers.len()));
                }
            }
        }
        println!("{owner} diffuse names: {names:?}");
    }
}
