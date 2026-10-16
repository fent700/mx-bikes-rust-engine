//! Chase and onboard (helmet) cameras.

use crate::bike_entity::BikeSim;
use crate::input::ControlState;
use bevy::prelude::*;

#[derive(Component)]
pub struct FollowCamera {
    pub mode: CameraMode,
    /// Smoothed heading used by the chase camera (radians).
    heading: f32,
    /// Onboard head yaw (deg), slewed at 600 deg/s.
    head_yaw: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShowroomCameraConfig {
    pub distance: f32,
    pub shift: Vec2,
    pub anglex_deg: f32,
    pub angley_deg: f32,
}

/// The game's defaults for the profile.ini `home_camera*` keys (FUN_1400e7080): angle x -30,
/// angle y 10 (constant at 0x1403953bc), distance 2.75, shift x -0.2, shift y 0. INFERRED: angle x
/// as the rotation about X (pitch) and angle y about Y (yaw), and the 0.65 m focus height; the
/// engine's camera placement is not decoded.
impl Default for ShowroomCameraConfig {
    fn default() -> Self {
        Self {
            distance: 2.75,
            shift: Vec2::new(-0.2, 0.0),
            anglex_deg: -30.0,
            angley_deg: 10.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CameraMode {
    Chase,
    Onboard,
    /// Fixed side view of the bike (asset inspection).
    Side,
    /// Fixed left side view of the bike (chain and drive inspection).
    Left,
    /// Showroom / menu camera orbit ported from FUN_1400e7080.
    Showroom(ShowroomCameraConfig),
}

impl FollowCamera {
    pub fn new(mode: CameraMode) -> Self {
        Self { mode, heading: 0.0, head_yaw: 0.0 }
    }
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self::new(CameraMode::Chase)
    }
}

pub fn update_camera(
    time: Res<Time>,
    sim: Res<BikeSim>,
    controls: Res<ControlState>,
    profile: Option<Res<crate::profile::Profile>>,
    mut cams: Query<(&mut Transform, &mut FollowCamera, &mut Projection)>,
    parts: Query<(&Name, &GlobalTransform), Without<FollowCamera>>,
) {
    let Ok((mut t, mut cam, mut projection)) = cams.get_single_mut() else { return };
    let pf = |s: &str, k: &str, d: f32| profile.as_ref().map(|p| p.f(s, k)).unwrap_or(d);
    let pi = |s: &str, k: &str, d: i64| profile.as_ref().map(|p| p.i(s, k)).unwrap_or(d);
    // The helmet view uses the profile field of view; the chase view is fixed at 75 degrees
    // (FUN_14000b7e0 writes 75.0 into the view for the external camera).
    let fov_deg = match cam.mode {
        CameraMode::Onboard => pf("view", "fov", 90.0).clamp(30.0, 150.0),
        CameraMode::Chase => 75.0,
        // The menu scene's camera is set to 60 degrees (`FUN_1400e7080`, op 0xb0).
        CameraMode::Showroom(_) => 60.0,
        _ => pf("view", "fov", 90.0).clamp(30.0, 150.0),
    };
    if let Projection::Perspective(p) = projection.as_mut() {
        // INFERRED from the menu picture: the game's field of view is vertical at a 4:3 aspect
        // and the horizontal extent stays fixed at wider aspects; Bevy's FOV is vertical.
        let aspect = p.aspect_ratio.max(0.1);
        let vertical = 2.0 * ((fov_deg.to_radians() * 0.5).tan() * (4.0 / 3.0) / aspect).atan();
        if (p.fov - vertical).abs() > 1e-4 {
            p.fov = vertical;
        }
        let far = pf("gfx", "drawdistance", 1000.0).max(500.0);
        if (p.far - far).abs() > 0.5 {
            p.far = far;
        }
    }
    if controls.toggle_camera {
        cam.mode = match cam.mode {
            CameraMode::Chase => CameraMode::Onboard,
            CameraMode::Onboard => CameraMode::Side,
            CameraMode::Side => CameraMode::Left,
            CameraMode::Left => CameraMode::Showroom(ShowroomCameraConfig::default()),
            CameraMode::Showroom(_) => CameraMode::Chase,
        };
    }
    let bike = &sim.0;
    let (origin, rot) = bike.chassis_transform();
    let dt = time.delta_secs();
    match cam.mode {
        CameraMode::Chase => {
            // External view (FUN_14000b7e0): a level frame along the bike's heading, or with
            // `ext_view/mode` 1 along the direction of travel (blended in from 1 to 5 m/s). The
            // camera sits 3.6 m + `distance` behind and 1.5 m + `height` above the midpoint of the
            // axles (moved 70 % towards the bike and 0.7 m down with `follow_bike`), `latoffset`
            // to the side, pitched down atan(height / (3.6 + distance)). `dynamic` adds
            // 0.3 - speed / 30 m of distance. INFERRED: the heading is smoothed (the game reads an
            // already filtered heading whose filter is not decoded), and the dynamic term's
            // second input (camera struct + 0x5ec) is taken as zero.
            let fwd = rot * Vec3::NEG_Z;
            let mut heading_dir = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
            if pi("ext_view", "mode", 1) != 0 {
                let vel = Vec3::new(bike.velocity.x, 0.0, bike.velocity.z);
                let v = vel.length();
                if v >= 5.0 {
                    heading_dir = vel / v;
                } else if v > 1.0 {
                    let w = (v - 1.0) * 0.25;
                    heading_dir = (vel / v * w + heading_dir * (1.0 - w)).normalize_or(heading_dir);
                }
            }
            let target_heading = heading_dir.x.atan2(-heading_dir.z);
            let mut d = target_heading - cam.heading;
            d = d.sin().atan2(d.cos());
            cam.heading += d * (dt * 5.0).min(1.0);
            let dir = Vec3::new(cam.heading.sin(), 0.0, -cam.heading.cos());
            let right = dir.cross(Vec3::Y);
            let (front_axle, _) = bike.front_wheel_transform();
            let (rear_axle, _) = bike.rear_axle_transform();
            let mut pivot = (front_axle + rear_axle) * 0.5;
            let follow = pi("ext_view", "follow_bike", 0) != 0;
            if follow {
                pivot = pivot.lerp(origin, 0.7);
            }
            let back = 3.6 + pf("ext_view", "distance", 0.0);
            let height = pf("ext_view", "height", 0.3);
            let dynamic = if pi("ext_view", "dynamic", 1) != 0 { 0.3 - bike.speed() / 30.0 } else { 0.0 };
            let up = 1.5 + height - if follow { 0.7 } else { 0.0 };
            t.translation = pivot + right * pf("ext_view", "latoffset", 0.0) + Vec3::Y * up - dir * (back - dynamic);
            let pitch = height.atan2(back);
            t.rotation = Quat::from_rotation_y(-cam.heading) * Quat::from_rotation_x(-pitch);
        }
        CameraMode::Onboard => {
            // Helmet view (FUN_14000b7e0): at the rider's head, raised by stand * 0.18 m, in the
            // rider's frame, looking down by 15 + 20 * profile `view/pitch` degrees (25 at the
            // default 0.5). INFERRED: the rider frame is the chassis frame (the rider body is not
            // a separate simulated body), and the head look-into-turn yaw (up to +-60 deg) is not
            // applied.
            // The camera links to the helmet's `cam` object (`gfx.cfg` camlinkobj, at
            // (0.209, 0, -0.046) in the left-handed helmet frame); the helmet itself is hidden
            // (its cockpit model `c_helmet.edf` is empty).
            let pitch = pf("view", "pitch", 0.5);
            let helmet = parts.iter().find(|(n, _)| n.as_str() == "helmet").map(|(_, g)| *g);
            t.translation = match helmet {
                Some(g) => g.transform_point(Vec3::new(0.209, 0.0, 0.046)),
                None => bike.rider_view_world() + rot * Vec3::Y * (bike.rider.stand * 0.18),
            };
            // Corner anticipation:
            // 1 = steer angle x corner_anticipation_scale
            // 2 = lean heading: clamp(lean / 50, -1, 1) x lean_heading_scale
            // Both slewed at 600 deg/s up to +-60 deg (FUN_14000b7e0 lines 200-244).
            let yaw_target = match pi("view", "corner_anticipation", 0) {
                1 => bike.steer_angle.to_degrees().clamp(-45.0, 45.0) * pf("view", "corner_anticipation_scale", 0.5),
                2 => (bike.lean_angle_deg / 50.0).clamp(-1.0, 1.0) * pf("view", "lean_heading_scale", 35.0),
                _ => 0.0,
            };
            let step = 600.0 * dt;
            cam.head_yaw += (yaw_target.clamp(-60.0, 60.0) - cam.head_yaw).clamp(-step, step);
            // `view/mode`: 0 keeps horizon level, 1 blends by `view/tilt` (0 horizon .. 1 bike roll).
            let f = rot * Vec3::NEG_Z;
            let horizon = Quat::from_rotation_y(f.x.atan2(-f.z) * -1.0) * Quat::from_rotation_x(f.y.clamp(-1.0, 1.0).asin());
            let tilt = if pi("view", "mode", 1) == 0 { 0.0 } else { pf("view", "tilt", 0.5) };
            let frame = horizon.slerp(rot, tilt);
            t.rotation = frame * Quat::from_rotation_y(-cam.head_yaw.to_radians()) * Quat::from_rotation_x(-(15.0 + 20.0 * pitch).to_radians());
        }
        CameraMode::Side => {
            let right = rot * Vec3::X;
            let focus = origin + Vec3::Y * 0.55;
            t.translation = focus + right * 2.6 + Vec3::Y * 0.25;
            t.look_at(focus, Vec3::Y);
        }
        CameraMode::Left => {
            let right = rot * Vec3::X;
            let focus = origin + Vec3::Y * 0.55;
            t.translation = focus - right * 2.6 + Vec3::Y * 0.25;
            t.look_at(focus, Vec3::Y);
        }
        CameraMode::Showroom(cfg) => {
            // Menu camera (`FUN_14001e710`, mode 0), in the game's left-handed world frame: the
            // focus is the midpoint of the two wheels raised 0.5 m and moved 0.35 m along x; the
            // camera turns by `home_cameraanglex` about Y, then `home_cameraangley` about X, and
            // sits at (shiftx, shifty, -distance) in that frame, looking along its +Z. The angles
            // are world angles, not relative to the bike.
            let (front, _) = bike.front_wheel_transform();
            let (rear, _) = bike.rear_wheel_transform();
            let lh = |v: Vec3| Vec3::new(v.x, v.y, -v.z);
            let focus = lh((front + rear) * 0.5) + Vec3::new(0.35, 0.5, 0.0);
            let (sy, cy) = cfg.anglex_deg.to_radians().sin_cos();
            let (sx, cx) = cfg.angley_deg.to_radians().sin_cos();
            let rot = |v: Vec3| {
                let v = Vec3::new(v.x, cx * v.y - sx * v.z, sx * v.y + cx * v.z);
                Vec3::new(cy * v.x + sy * v.z, v.y, -sy * v.x + cy * v.z)
            };
            let pos = focus + rot(Vec3::new(cfg.shift.x, cfg.shift.y, -cfg.distance));
            let forward = rot(Vec3::Z);
            let up = rot(Vec3::Y);
            t.translation = lh(pos);
            t.look_to(lh(forward), lh(up));
        }
    }
}

/// The helmet view hides the rider's helmet (the camera sits inside it); other views show it.
pub fn onboard_visibility(cams: Query<&FollowCamera>, mut parts: Query<(&Name, &mut Visibility)>) {
    let Ok(cam) = cams.get_single() else { return };
    let hidden = matches!(cam.mode, CameraMode::Onboard);
    for (name, mut vis) in &mut parts {
        if name.as_str() == "helmet" {
            let want = if hidden { Visibility::Hidden } else { Visibility::Inherited };
            if *vis != want {
                *vis = want;
            }
        }
    }
}
