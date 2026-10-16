//! Collision triangle count and extent of every track `.map`.
use mxbikes_formats::TrackMap;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    for path in vfs.list_files("tracks/").into_iter().filter(|p| p.ends_with(".map")) {
        let Ok(data) = vfs.read(&path) else { continue };
        match TrackMap::parse(&data, None) {
            Ok(map) => {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for t in &map.collision {
                    for v in t.v {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                }
                println!("{path}: {} triangles, extent {lo:.0?} .. {hi:.0?}", map.collision.len());
            }
            Err(e) => println!("{path}: {e}"),
        }
    }
}
