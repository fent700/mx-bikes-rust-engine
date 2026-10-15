//! Compares `.anm` keys with the rider EDF rest pose to establish the key space.
use mxbikes_formats::{AnimationFile, EdfModel};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let model = EdfModel::parse(&vfs.read("rider/riders/default_mx/rider.edf").unwrap()).unwrap();
    let obj = &model.groups[0].objects[0];
    let anim = AnimationFile::parse(&vfs.read("rider/animations/mx/rider7.anm").unwrap()).unwrap();
    let clip = &anim.clips[0];
    for name in ["riderRIG_Root", "riderRIG_Pelvis", "riderRIG_LeftHip", "riderRIG_LeftKnee", "riderRIG_Spine2", "riderRIG_Head", "riderRIG_LeftElbow"] {
        let Some(n) = obj.find_node(name) else { continue };
        let (_, rest_rot, rest_pos) = obj.nodes[n].transform.to_scale_rotation_translation();
        let bone = clip.bones.iter().find(|b| b.name == name).unwrap();
        let k = &bone.keys[0];
        println!(
            "{name:22} rest t {:>7.3?} q {:>6.3?}\n{:22} key0 t {:>7.3?} q {:>6.3?}  trailer {}",
            rest_pos.to_array(),
            rest_rot.to_array(),
            "",
            k.translation.to_array(),
            k.rotation.to_array(),
            bone.trailer
        );
    }
}
