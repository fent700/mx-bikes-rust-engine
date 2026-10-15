//! For control-layer clips (throttle, brakes, clutch, gears) lists which bones move across the
//! clip and how far the static bones are from the centre frame of `leaning sitting`.
use mxbikes_formats::{AnimationClip, AnimationFile};
use mxbikes_vfs::MxBikesVfs;
use std::collections::HashMap;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut clips: HashMap<String, AnimationClip> = HashMap::new();
    for i in 1..=42 {
        let file = if i == 1 { "rider.anm".to_string() } else { format!("rider{i}.anm") };
        if let Ok(d) = vfs.read(&format!("rider/animations/mx/{file}")) {
            for c in AnimationFile::parse(&d).unwrap().clips {
                clips.insert(c.name.to_ascii_lowercase(), c);
            }
        }
    }
    let mut names: Vec<_> = clips.keys().cloned().collect();
    names.sort();
    println!("clips: {names:?}");
    let base = &clips["leaning sitting"];
    let mid = base.frame_count as usize / 2;
    for name in std::env::args().skip(1) {
        let Some(c) = clips.get(&name.to_ascii_lowercase()) else {
            println!("{name}: not found");
            continue;
        };
        let mut moving = Vec::new();
        let mut static_diff = 0.0f32;
        for b in &c.bones {
            let k0 = &b.keys[0];
            let range = b.keys.iter().map(|k| k.rotation.angle_between(k0.rotation).to_degrees()).fold(0.0, f32::max);
            if range > 0.5 {
                moving.push(format!("{} {:.0}deg", b.name.trim_start_matches("riderRIG_"), range));
            } else if let Some(bb) = base.bones.iter().find(|x| x.name == b.name) {
                static_diff = static_diff.max(k0.rotation.angle_between(bb.keys[mid].rotation).to_degrees());
            }
        }
        println!("{name} ({} frames): moving {:?}\n   static bones max diff from leaning sitting centre: {static_diff:.1} deg", c.frame_count, moving);
        let on: Vec<_> = c
            .bones
            .iter()
            .filter(|b| b.trailer != 0.0)
            .map(|b| format!("{}={}", b.name.trim_start_matches("riderRIG_"), b.trailer))
            .collect();
        println!("   bones with non-zero trailer ({} of {}): {:?}", on.len(), c.bones.len(), on);
    }
}
