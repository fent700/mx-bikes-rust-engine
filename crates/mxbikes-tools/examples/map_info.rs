//! Summarises a `.map` file: materials, textures and scenery mesh extents.
use mxbikes_formats::TrackMap;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let path = std::env::args().nth(1).expect("path");
    let data = vfs.read(&path).unwrap();
    let map = TrackMap::parse(&data, None).unwrap();
    println!("version {} materials {} vertices {} submeshes {}", map.version, map.materials.len(), map.scenery.vertices.len(), map.scenery.submeshes.len());
    for (i, m) in map.materials.iter().enumerate() {
        println!("material {i}: diffuse '{}' {}x{} flags {:#x}", m.diffuse.name, m.diffuse.width, m.diffuse.height, m.diffuse.flags);
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &map.scenery.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v.position[k]);
            hi[k] = hi[k].max(v.position[k]);
        }
    }
    println!("extent {lo:?} .. {hi:?}");
    println!("collision triangles {}", map.collision.len());
    let mut words: std::collections::BTreeMap<String, usize> = Default::default();
    for t in map.collision.iter().take(200000) {
        *words.entry(format!("{:?}", t.words)).or_default() += 1;
    }
    for (w, n) in words.iter().take(12) {
        println!("  words {w} x{n}");
    }
    if let Some(t) = &map.terrain {
        println!("terrain layers {}", t.layers.len());
    }
}
