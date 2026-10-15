//! Full throttle from a standstill with the autoshift aid: prints gear, rpm, speed and slip.
use glam::Vec3;
use mxbikes_formats::{assets, Surface};
use mxbikes_physics::{Ground, Motorcycle, RiderInput};
use mxbikes_vfs::MxBikesVfs;

struct Flat;
impl Ground for Flat {
    fn height(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
    fn normal(&self, _x: f32, _z: f32) -> Vec3 {
        Vec3::Y
    }
    fn surface(&self, _x: f32, _z: f32) -> Surface {
        Surface::SoftSoil
    }
}

fn main() {
    let vfs = MxBikesVfs::auto_detect().unwrap();
    let id = std::env::args().nth(1).unwrap_or_else(|| "250xf".into());
    let throttle: f32 = std::env::args().nth(2).and_then(|t| t.parse().ok()).unwrap_or(1.0);
    let def = assets::load_bike(&vfs, &id).unwrap();
    let tyres = assets::load_tyres(&vfs, &assets::tyre_set_for(&vfs, &def.tyres_type).unwrap()).unwrap();
    let mut bike = Motorcycle::new(def, tyres, Vec3::ZERO, 0.0);
    bike.place_on(Vec3::ZERO, 0.0);
    bike.powertrain.auto_shift = true;
    let dt = 1.0 / 120.0;
    for step in 0..(20.0 / dt) as usize {
        let t = step as f32 * dt;
        bike.input = RiderInput { throttle: if t > 1.0 { throttle } else { 0.0 }, shift_up: step == 60, ..Default::default() };
        bike.step(dt, &Flat);
        if step % 30 == 0 {
            let r = bike.rear_telemetry();
            println!(
                "t {t:5.1}  gear {}  rpm {:6.0}  {:6.1} km/h  rear slip {:+.3}  wheel torque {:7.1}  clutch slip {:7.1}  omega {:6.1}  pending {}",
                bike.powertrain.gear,
                bike.powertrain.rpm(),
                bike.speed() * 3.6,
                r.slip_ratio,
                bike.powertrain.wheel_torque,
                bike.powertrain.clutch_slip,
                bike.rear_omega(),
                bike.powertrain.shift_in_progress()
            );
        }
    }
}
