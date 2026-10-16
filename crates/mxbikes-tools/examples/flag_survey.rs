//! Lists material texture names with their flags across every `.map` in the game.
use mxbikes_formats::TrackMap;
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    for path in vfs.list_files("").into_iter().filter(|p| p.ends_with(".map")) {
        let Ok(data) = vfs.read(&path) else { continue };
        let Ok(map) = TrackMap::parse(&data, None) else { continue };
        for m in &map.materials {
            println!("{:#06x} {} {}", m.diffuse.flags, m.diffuse.name, path);
        }
    }
}
