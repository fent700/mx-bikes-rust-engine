//! Full brakes from speed on flat ground: prints front/rear tyre state every 50 ms.
use glam::Vec3;
use mxbikes_formats::assets;
use mxbikes_physics::{FlatGround, Motorcycle, RiderInput};
use mxbikes_vfs::MxBikesVfs;

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let id = std::env::args().nth(1).unwrap_or_else(|| "250xf".into());
    let front: f32 = std::env::args().nth(2).and_then(|t| t.parse().ok()).unwrap_or(1.0);
    let rear: f32 = std::env::args().nth(3).and_then(|t| t.parse().ok()).unwrap_or(1.0);
    let def = assets::load_bike(&vfs, &id).unwrap();
    let tyres = assets::load_tyres(&vfs, &assets::tyre_set_for(&vfs, &def.tyres_type).unwrap()).unwrap();
    let mut bike = Motorcycle::new(def, tyres, Vec3::ZERO, 0.0);
    bike.place_on(Vec3::ZERO, 0.0);
    bike.set_speed(25.0);
    bike.brake_help = std::env::args().nth(4).map(|a| a == "help").unwrap_or(false);
    let dt = 1.0 / 200.0;
    for step in 0..(6.0 / dt) as usize {
        let t = step as f32 * dt;
        let braking = t > 1.0;
        bike.input = RiderInput {
            front_brake: if braking { front } else { 0.0 },
            rear_brake: if braking { rear } else { 0.0 },
            clutch: 1.0,
            ..Default::default()
        };
        bike.step(dt, &FlatGround);
        if step % 10 == 0 {
            let f = bike.front_telemetry();
            let r = bike.rear_telemetry();
            let fwd = bike.rotation * Vec3::NEG_Z;
            println!(
                "t {t:4.2} v {:6.1} pitch {:6.1}  F: fz {:6.0} fx {:6.0} k {:+.3} om {:6.1}  R: fz {:6.0} fx {:6.0} k {:+.3}",
                bike.speed() * 3.6,
                fwd.y.asin().to_degrees(),
                f.load, f.fx, f.slip_ratio, bike.front_omega(),
                r.load, r.fx, r.slip_ratio,
            );
        }
        if braking && bike.speed() < 0.3 {
            break;
        }
    }
}
