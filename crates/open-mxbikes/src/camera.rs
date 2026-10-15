//! Chase and onboard (helmet) cameras.

use crate::bike_entity::BikeSim;
use crate::input::ControlState;
use bevy::prelude::*;

#[derive(Component)]
pub struct FollowCamera {
    pub mode: CameraMode,
    /// Smoothed heading used by the chase camera (radians).
    heading: f32,
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
        Self { mode, heading: 0.0 }
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
    mut cams: Query<(&mut Transform, &mut FollowCamera)>,
    parts: Query<(&Name, &GlobalTransform), Without<FollowCamera>>,
) {
    let Ok((mut t, mut cam)) = cams.get_single_mut() else { return };
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
            let fwd = rot * Vec3::NEG_Z;
            let target_heading = fwd.x.atan2(-fwd.z);
            let mut d = target_heading - cam.heading;
            d = d.sin().atan2(d.cos());
            cam.heading += d * (dt * 5.0).min(1.0);
            let dir = Vec3::new(cam.heading.sin(), 0.0, -cam.heading.cos());
            let focus = origin + Vec3::Y * 1.0;
            let desired = focus - dir * 4.0 + Vec3::Y * 1.2;
            t.translation = t.translation.lerp(desired, (dt * 10.0).min(1.0));
            t.look_at(focus + dir * 2.0, Vec3::Y);
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
            let pitch = profile.as_ref().map(|p| p.f("view", "pitch")).unwrap_or(0.5);
            let helmet = parts.iter().find(|(n, _)| n.as_str() == "helmet").map(|(_, g)| *g);
            t.translation = match helmet {
                Some(g) => g.transform_point(Vec3::new(0.209, 0.0, 0.046)),
                None => bike.rider_view_world() + rot * Vec3::Y * (bike.rider.stand * 0.18),
            };
            t.rotation = rot * Quat::from_rotation_x(-(15.0 + 20.0 * pitch).to_radians());
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
            // In the bike's heading frame. Angles and offset are given in the game's left-handed
            // frame, where the bike faces +Z and the camera sits `distance` ahead of it; mirroring
            // into the right-handed frame negates both angles and the offset's Z.
            let (_, yaw, _) = rot.to_euler(EulerRot::YXZ);
            let heading = Quat::from_rotation_y(yaw);
            let focus = origin + heading * Vec3::new(cfg.shift.x, 0.65 + cfg.shift.y, 0.0);
            let orbit = Quat::from_euler(EulerRot::YXZ, -cfg.angley_deg.to_radians(), -cfg.anglex_deg.to_radians(), 0.0);
            t.translation = focus + heading * rot_bike_frame(orbit, cfg.distance);
            t.look_at(focus, Vec3::Y);
        }
    }
}

/// Camera offset `distance` in front of the bike (asset +Z, engine -Z) after the orbit rotation.
fn rot_bike_frame(rot: Quat, distance: f32) -> Vec3 {
    rot * Vec3::new(0.0, 0.0, -distance)
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
