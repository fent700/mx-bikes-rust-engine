//! Rides into a track object (a near-vertical collision triangle close to the ground) on Club
//! and reports how far past its plane the bike gets.
use glam::Vec3;
use mxbikes_formats::{assets, TrackMap, TrackTerrain};
use mxbikes_physics::{CollisionMesh, Ground, Motorcycle, RiderInput, TerrainGround};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let stem = assets::find_track(&vfs, "club").unwrap();
    let terrain = TrackTerrain::parse(&vfs.read(&format!("{stem}.trh")).unwrap()).unwrap();
    let map = TrackMap::parse(&vfs.read(&format!("{stem}.map")).unwrap(), None).unwrap();
    let tris: Vec<[Vec3; 3]> = map.collision.iter().map(|t| t.v.map(|v| Vec3::new(v.x, v.y, -v.z))).collect();
    let mut ground = TerrainGround::new(terrain);
    // A wall-like triangle: near-horizontal normal, at least 1 m tall, starting near the ground.
    let wall = tris
        .iter()
        .find(|t| {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            let (lo, hi) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
            let c = (t[0] + t[1] + t[2]) / 3.0;
            n.y.abs() < 0.2 && hi - lo > 1.0 && (lo - ground.height(c.x, c.z)).abs() < 0.3 && (t[1] - t[0]).length() > 2.0
        })
        .copied()
        .expect("no wall triangle");
    ground.objects = CollisionMesh::new(tris);
    let c = (wall[0] + wall[1] + wall[2]) / 3.0;
    let n = (wall[1] - wall[0]).cross(wall[2] - wall[0]).normalize();
    let n = Vec3::new(n.x, 0.0, n.z).normalize();
    let start = c + n * 6.0;
    let start = Vec3::new(start.x, ground.height(start.x, start.z), start.z);
    // Face the wall: heading turns -Z towards -n.
    let dir = -n;
    let heading = (-dir.x).atan2(-dir.z);
    let def = assets::load_bike(&vfs, "250xf").unwrap();
    let tyres = assets::load_tyres(&vfs, &assets::tyre_set_for(&vfs, &def.tyres_type).unwrap()).unwrap();
    let mut bike = Motorcycle::new(def, tyres, start, heading);
    bike.place_on(start, heading);
    bike.powertrain.auto_shift = true;
    println!("wall centre {c:.2}, normal {n:.2}, start {start:.2}");
    let dt = 1.0 / 200.0;
    let mut deepest = f32::MAX;
    for step in 0..(6.0 / dt) as usize {
        let t = step as f32 * dt;
        bike.input = RiderInput { throttle: if t > 0.5 { 0.5 } else { 0.0 }, shift_up: step == 20, ..Default::default() };
        bike.step(dt, &ground);
        let dist = (bike.com - c).dot(n);
        deepest = deepest.min(dist);
        if step % 50 == 0 {
            println!("t {t:4.2} v {:5.1} km/h  distance to wall plane {dist:6.2} m  crashed {}", bike.speed() * 3.6, bike.crashed);
        }
    }
    println!("closest approach of the centre of mass to the wall plane: {deepest:.2} m");
}
