//! Prints the rider pose across the lean clip: the head's lateral offset from the pelvis in
//! rider space, which shows how frames map to lean angle.
use glam::{Affine3A, Vec3};
use mxbikes_formats::{AnimationFile, EdfModel};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let model = EdfModel::parse(&vfs.read("rider/riders/default_mx/rider.edf").unwrap()).unwrap();
    let obj = &model.groups[0].objects[0];
    let file = std::env::args().nth(1).unwrap_or_else(|| "rider.anm".into());
    let anim = AnimationFile::parse(&vfs.read(&format!("rider/animations/mx/{file}")).unwrap()).unwrap();
    let clip = &anim.clips[0];
    let track = |name: &str| clip.bones.iter().position(|b| b.name == name);
    let world = |frame: usize, node: usize| -> Vec3 {
        let mut chain = vec![node];
        while let Some(p) = obj.nodes[*chain.last().unwrap()].parent {
            chain.push(p);
        }
        let mut m = Affine3A::IDENTITY;
        for &n in chain.iter().rev() {
            let local = match track(&obj.nodes[n].name) {
                Some(b) => {
                    let k = &clip.bones[b].keys[frame];
                    Affine3A::from_rotation_translation(k.rotation, k.translation)
                }
                None => Affine3A::from_mat4(obj.nodes[n].transform),
            };
            m = m * local;
        }
        m.translation.into()
    };
    let head = obj.find_node("riderRIG_Head").unwrap();
    let pelvis = obj.find_node("riderRIG_Pelvis").unwrap();
    println!("clip '{}' {} frames", clip.name, clip.frame_count);
    for f in (0..clip.frame_count as usize).step_by(4).chain([clip.frame_count as usize - 1]) {
        let d = world(f, head) - world(f, pelvis);
        let roll = d.x.atan2(d.y).to_degrees();
        println!("  frame {f:2}: head-pelvis {:>6.3?} roll {roll:6.1} deg", d.to_array());
    }
}
