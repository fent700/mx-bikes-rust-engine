//! Inspects scenery submeshes, vertices and UV coordinates of a .map file (defaults to misc/bg/bg.map).
use mxbikes_formats::TrackMap;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let path = std::env::args().nth(1).unwrap_or_else(|| "misc/bg/bg.map".to_string());
    let data = vfs.read(&path).unwrap();
    let map = TrackMap::parse(&data, None).unwrap();
    for (i, sm) in map.scenery.submeshes.iter().enumerate() {
        let start = (sm.first_triangle * 3) as usize;
        let count = (sm.triangle_count * 3) as usize;
        let indices = &map.scenery.indices[start..start + count];
        println!("SM {i}: mat {}, triangles {}", sm.material, count / 3);
        for &idx in indices.iter().take(5) {
            let v = &map.scenery.vertices[idx as usize];
            println!("  pos [{:.2}, {:.2}, {:.2}], uv [{:.2}, {:.2}]",
                v.position[0], v.position[1], v.position[2], v.uv0[0], v.uv0[1]);
        }
    }
}
