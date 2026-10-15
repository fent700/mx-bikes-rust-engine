use mxbikes_formats::{AnimationFile, EdfModel};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    for p in ["rider/animations/mx/rider.anm", "rider/animations/mx/rider7.anm", "misc/marshals/flagman_left.anm"] {
        let a = AnimationFile::parse(&vfs.read(p).unwrap()).unwrap();
        println!("{p}: version {} clips {}", a.version, a.clips.len());
        for c in a.clips.iter().take(3) {
            let names: Vec<_> = c.bones.iter().take(6).map(|b| b.name.clone()).collect();
            let k = c.bones.first().and_then(|b| b.keys.first());
            println!("  clip '{}' frames {} bones {} first {:?} key0 {:?}", c.name, c.frame_count, c.bones.len(), names, k);
            let bad = c.bones.iter().flat_map(|b| &b.keys).filter(|k| (k.rotation.length() - 1.0).abs() > 0.01).count();
            println!("  non-unit quats: {bad} of {}", c.bones.iter().map(|b| b.keys.len()).sum::<usize>());
        }
    }
    let m = EdfModel::parse(&vfs.read("rider/riders/default_mx/rider.edf").unwrap()).unwrap();
    for g in m.groups.iter().take(2) {
        for o in &g.objects {
            let nodes = o.nodes.len();
            println!("group '{}' verts {} nodes {}", g.name, o.vertices.len(), nodes);
            for n in &o.nodes {
                if let Some(sk) = &n.skinning {
                    let sums: Vec<f32> = sk.iter().map(|s| s.weights.iter().sum()).collect();
                    let bad = sums.iter().filter(|s| (*s - 1.0).abs() > 0.02).count();
                    let maxb = sk.iter().flat_map(|s| s.bones).max().unwrap_or(0);
                    println!("  skinned node '{}' verts {} weight-sum!=1: {} max bone {} sample {:?}", n.name, sk.len(), bad, maxb, sk.get(0));
                }
            }
            println!("  node names: {:?}", o.nodes.iter().take(8).map(|n| n.name.clone()).collect::<Vec<_>>());
        }
    }
}
