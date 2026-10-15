use mxbikes_formats::EdfModel;
use mxbikes_vfs::MxBikesVfs;
fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let mut multi = 0; let mut nodes = 0; let mut skinned = 0;
    for p in vfs.list_files("") {
        if !p.ends_with(".edf") { continue; }
        let Ok(m) = EdfModel::parse(&vfs.read(&p).unwrap()) else { continue };
        for g in &m.groups { for o in &g.objects { for n in &o.nodes {
            nodes += 1;
            if n.submeshes.len() > 1 { multi += 1; if multi < 5 { println!("multi-sub node {p} {} {}", n.name, n.submeshes.len()); } }
            if n.skinning.is_some() { skinned += 1; }
        }}}
    }
    println!("nodes {nodes} multi-submesh {multi} skinned {skinned}");
}
