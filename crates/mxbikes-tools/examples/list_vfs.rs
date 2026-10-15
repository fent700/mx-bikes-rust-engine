//! Lists files in the game's virtual file system under a prefix.
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let prefix = std::env::args().nth(1).unwrap_or_default();
    for f in vfs.list_files(&prefix) {
        println!("{f}");
    }
}
