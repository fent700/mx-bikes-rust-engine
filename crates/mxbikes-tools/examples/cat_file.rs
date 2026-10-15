//! Prints a (decrypted) file from the game's virtual file system.
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let path = std::env::args().nth(1).expect("path");
    let data = vfs.read(&path).unwrap();
    print!("{}", String::from_utf8_lossy(&data));
}
