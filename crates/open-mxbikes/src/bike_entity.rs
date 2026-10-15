//! Drives the physics bike and mirrors its part transforms onto the rendered meshes.

use crate::input::ControlState;
use crate::world::{SpawnPoint, TrackGround};
use bevy::prelude::*;
use mxbikes_physics::Motorcycle;

#[derive(Resource)]
pub struct BikeSim(pub Motorcycle);

#[derive(Resource)]
pub struct BikeParts {
    pub chassis: Entity,
    pub steer: Entity,
    pub fork: Entity,
    pub swingarm: Entity,
    pub front_wheel: Entity,
    pub rear_wheel: Entity,
    pub rear_axle: Entity,
}

pub fn step_physics(
    time: Res<Time>,
    controls: Res<ControlState>,
    ground: Option<Res<TrackGround>>,
    spawn: Option<Res<SpawnPoint>>,
    ui_mode: Option<Res<crate::ui::UiMode>>,
    mut to_pits: Option<ResMut<crate::ui::ToPits>>,
    mut sim: ResMut<BikeSim>,
    mut crash_timer: Local<f32>,
) {
    let Some(ground) = ground else { return };
    let bike = &mut sim.0;

    // STAND-IN (not from the game): reset to the spawn when the bike leaves the heightfield,
    // where there is no ground mesh, and 3 s after the rider falls. The game's reset behaviour
    // follows the profile `reset_type` setting (FUN_1401126c0), which is not decoded.
    let (min_x, max_x) = (ground.0.terrain.origin.x, ground.0.terrain.origin.x + ground.0.terrain.size_x);
    let (min_z, max_z) = (-ground.0.terrain.origin.z - ground.0.terrain.size_z, -ground.0.terrain.origin.z);
    let out_of_bounds = bike.com.x < min_x || bike.com.x > max_x || bike.com.z < min_z || bike.com.z > max_z;

    if bike.crashed {
        *crash_timer += time.delta_secs();
    } else {
        *crash_timer = 0.0;
    }

    let pit = to_pits.as_mut().is_some_and(|t| std::mem::take(&mut t.0));
    if pit || controls.reset || out_of_bounds || *crash_timer > 3.0 {
        *crash_timer = 0.0;
        if let Some(sp) = spawn {
            bike.place_on(sp.position, sp.heading);
        } else {
            bike.place_on(bike.com, 0.0);
        }
    }
    if let Some(mode) = ui_mode {
        // The simulation waits while a menu is open.
        if *mode != crate::ui::UiMode::None {
            return;
        }
    }
    bike.input = controls.rider;
    bike.step(time.delta_secs(), ground.0.as_ref());
}

pub fn sync_parts(sim: Res<BikeSim>, parts: Res<BikeParts>, mut q: Query<&mut Transform>) {
    let bike = &sim.0;
    let (ch_pos, ch_rot, ch_scale) = bike.chain_transform();
    let pairs = [
        (parts.chassis, bike.chassis_transform().0, bike.chassis_transform().1, Vec3::ONE),
        (parts.steer, bike.steer_transform().0, bike.steer_transform().1, Vec3::ONE),
        (parts.fork, bike.fork_transform().0, bike.fork_transform().1, Vec3::ONE),
        (parts.swingarm, bike.swingarm_transform().0, bike.swingarm_transform().1, Vec3::ONE),
        (parts.front_wheel, bike.front_wheel_transform().0, bike.front_wheel_transform().1, Vec3::ONE),
        (parts.rear_wheel, bike.rear_wheel_transform().0, bike.rear_wheel_transform().1, Vec3::ONE),
        (parts.rear_axle, ch_pos, ch_rot, ch_scale),
    ];
    for (e, pos, rot, scale) in pairs {
        if let Ok(mut t) = q.get_mut(e) {
            t.translation = pos;
            t.rotation = rot;
            t.scale = scale;
        }
    }
}

pub fn update_chain_uv(
    sim: Res<BikeSim>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<(&Mesh3d, &crate::scene::ChainScroll)>,
) {
    let wheel_angle = sim.0.rear_wheel_angle();
    for (mesh3d, scroll) in &q {
        let Some(mesh) = meshes.get_mut(&mesh3d.0) else { continue };
        let offset = -wheel_angle * scroll.ratio;
        let new_uvs: Vec<[f32; 2]> = scroll.base_uvs.iter().map(|&[u, v]| [u + offset, v]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, new_uvs);
    }
}
