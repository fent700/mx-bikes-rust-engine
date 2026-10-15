use mxbikes_physics::Motorcycle;
use mxbikes_vfs::MxBikesVfs;
use mxbikes_formats::assets;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let def = assets::load_bike(&vfs, "250xf").unwrap();
    let tyre_set = assets::tyre_set_for(&vfs, &def.tyres_type).unwrap();
    let tyres = assets::load_tyres(&vfs, &tyre_set).unwrap();
    let stem = assets::find_track(&vfs, "club").unwrap();
    let terrain = mxbikes_formats::TrackTerrain::parse(&vfs.read(&format!("{stem}.trh")).unwrap()).unwrap();
    let track_def = assets::load_track_definition(&vfs, &stem).unwrap();
    let start = track_def.start.unwrap();
    let stall = start.stalls.first().unwrap();
    let line = terrain.centerline.as_ref().unwrap();
    let pos = terrain.track_to_world(stall.long, stall.lat).unwrap();
    let heading_asset = line.heading_deg(stall.long) + stall.angle_deg;
    let heading = -heading_asset.to_radians();

    let mut bike = Motorcycle::new(def, tyres, pos, heading);
    bike.place_on(pos, heading);
    let mut ground = mxbikes_physics::TerrainGround::new(terrain);

    let dt = 0.002;
    let total_steps = (15.0 / dt) as usize;
    let mut shifted = false;
    for step in 0..total_steps {
        let t = step as f32 * dt;
        let mut inp = mxbikes_physics::RiderInput::default();
        if t >= 2.0 {
            if !shifted {
                bike.powertrain.shift_up();
                shifted = true;
            }
            inp.throttle = 0.4;
            inp.steer = 0.3;
        }
        bike.input = inp;
        bike.step(dt, &mut ground);
        if step % 500 == 0 {
            let (p, _) = bike.chassis_transform();
            println!("t={:.1}s pos=({:.1}, {:.1}, {:.1}) speed={:.1} km/h", t, p.x, p.y, p.z, bike.speed() * 3.6);
        }
    }
}
