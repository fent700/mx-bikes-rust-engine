//! Crops and scales a PNG region: crop <in> <out> <x> <y> <w> <h> <scale>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let img = image::open(&a[0]).unwrap().to_rgba8();
    let n = |i: usize| a[i].parse::<u32>().unwrap();
    let sub = image::imageops::crop_imm(&img, n(2), n(3), n(4), n(5)).to_image();
    let s = n(6);
    let out = image::imageops::resize(&sub, sub.width() * s, sub.height() * s, image::imageops::FilterType::Nearest);
    out.save(&a[1]).unwrap();
}
