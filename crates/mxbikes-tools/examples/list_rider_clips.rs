//! Lists every clip in the rider animation set referenced by `gfx.cfg`.
use mxbikes_formats::{AnimationFile, ConfigNode};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let set = std::env::args().nth(1).unwrap_or_else(|| "mx".into());
    let dir = format!("rider/animations/{set}");
    let cfg = ConfigNode::parse(&String::from_utf8_lossy(&vfs.read(&format!("{dir}/gfx.cfg")).unwrap())).unwrap();
    for i in 1..=64 {
        let key = if i == 1 { "anim".to_string() } else { format!("anim{i}") };
        let Some(file) = cfg.get_str(&key) else { continue };
        let a = AnimationFile::parse(&vfs.read(&format!("{dir}/{file}")).unwrap()).unwrap();
        let clips: Vec<String> = a.clips.iter().map(|c| format!("'{}' ({} f)", c.name, c.frame_count)).collect();
        println!("{key:7} {file:16} {}", clips.join(", "));
    }
}
