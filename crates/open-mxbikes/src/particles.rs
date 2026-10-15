//! Tyre roost from the stock particle effects (`effects/particles/<surface>.cfg`).
//!
//! From the game:
//! - effect per surface and the per-track tint (track `gfx.cfg` `particles/<effect>/red|green|blue`,
//!   built-in defaults when any channel is missing; `FUN_14004ebc0`)
//! - emission per wheel (`FUN_140050480`): speed effects fire when the tyre rim speed v reaches
//!   `min_speed`; exit velocity = -t v exit_speed_scale cos(exit_angle) + n v exit_speed_scale
//!   sin(exit_angle) + wheel velocity * momentum_scale, with t the rolling direction in the ground
//!   plane and n the ground normal; the engine is given `frequency / (v / size_speed_scale)` and the
//!   size factor `min(v / size_speed_scale, 1)`
//! - `texture_slots` is log2 of the number of square cells in the texture (engine `FUN_140247d50`)
//! - particle life, gravity, start / end size, colour and alpha keys from the effect file
//!
//! INFERRED (the engine's particle update is not decoded): the first engine value is used as
//! particles per second, `exit_variance` as a cone half-angle in degrees, `friction` as a linear
//! velocity decay per second. Ground collision is not applied (not decoded).

use crate::bike_entity::BikeSim;
use crate::plate::decode_tga_rgba;
use crate::scene;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::view::NoFrustumCulling;
use mxbikes_formats::assets::read_cfg;
use mxbikes_formats::{ConfigNode, Surface};
use mxbikes_physics::WheelTelemetry;
use mxbikes_vfs::MxBikesVfs;

/// The engine's particle budget per emitter (`0xbf` with 1000).
const MAX_PARTICLES: usize = 1000;

/// Roost effects by surface, with the game's default tint (`FUN_14004ebc0`).
const SURFACE_EFFECTS: [(Surface, &str, [f32; 3]); 5] = [
    (Surface::Soil, "soil", [0.392_157, 0.294_118, 0.2]),
    (Surface::CompactSoil, "soilcompact", [0.270_996, 0.203_999, 0.148_999]),
    (Surface::SoftSoil, "soilsoft", [0.517_999, 0.404_000, 0.290_000]),
    (Surface::Sand, "sand", [0.490_000, 0.411_999, 0.278_000]),
    (Surface::Gravel, "gravel", [0.549_000, 0.517_999, 0.459_000]),
];

#[derive(Debug, Clone)]
struct EffectSpec {
    max_life: f32,
    gravity: f32,
    friction: f32,
    start_size: f32,
    end_size: f32,
    color_keys: Vec<(f32, Vec3)>,
    alpha_keys: Vec<(f32, f32)>,
    texture: String,
    cells: u32,
    min_speed: f32,
    exit_speed_scale: f32,
    exit_angle: f32,
    exit_variance: f32,
    frequency: f32,
    size_speed_scale: f32,
    momentum_scale: f32,
}

impl EffectSpec {
    fn from_cfg(c: &ConfigNode) -> Self {
        let f = |k: &str, d: f32| c.get_f32(k).unwrap_or(d);
        let keys = |count: &str, prefix: &'static str| {
            (0..c.get_i64(count).unwrap_or(0).max(0)).map(move |i| format!("{prefix}{i}")).collect::<Vec<_>>()
        };
        let color_keys = keys("num_color_keys", "color_key")
            .into_iter()
            .filter_map(|k| {
                let n = c.get_child(&k)?;
                let g = |x: &str| n.get_f32(x).unwrap_or(1.0);
                Some((n.get_f32("time").unwrap_or(0.0), Vec3::new(g("red"), g("green"), g("blue"))))
            })
            .collect();
        let alpha_keys = keys("num_alpha_keys", "alpha_key")
            .into_iter()
            .filter_map(|k| {
                let n = c.get_child(&k)?;
                Some((n.get_f32("time").unwrap_or(0.0), n.get_f32("alpha").unwrap_or(1.0)))
            })
            .collect();
        Self {
            max_life: f("max_life", 1.0),
            gravity: f("gravity", 0.0),
            friction: f("friction", 0.0),
            start_size: f("start_size", 1.0),
            end_size: f("end_size", 1.0),
            color_keys,
            alpha_keys,
            texture: c.get_str("texture").unwrap_or("").to_string(),
            cells: 1 << (c.get_i64("texture_slots").unwrap_or(0).clamp(0, 8) as u32),
            min_speed: f("min_speed", 0.0),
            exit_speed_scale: f("exit_speed_scale", 0.0),
            exit_angle: f("exit_angle", 0.0),
            exit_variance: f("exit_variance", 0.0),
            frequency: f("frequency", 0.0),
            size_speed_scale: f("size_speed_scale", 1.0),
            momentum_scale: f("momentum_scale", 0.0),
        }
    }

    fn color(&self, t: f32) -> Vec3 {
        piecewise(&self.color_keys, t, Vec3::ONE, |a, b, u| a.lerp(b, u))
    }

    fn alpha(&self, t: f32) -> f32 {
        piecewise(&self.alpha_keys, t, 1.0, |a, b, u| a + (b - a) * u)
    }
}

fn piecewise<T: Copy>(keys: &[(f32, T)], t: f32, default: T, lerp: impl Fn(T, T, f32) -> T) -> T {
    match keys {
        [] => default,
        [only] => only.1,
        _ => {
            if t <= keys[0].0 {
                return keys[0].1;
            }
            for w in keys.windows(2) {
                if t <= w[1].0 {
                    let u = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                    return lerp(w[0].1, w[1].1, u);
                }
            }
            keys[keys.len() - 1].1
        }
    }
}

#[derive(Debug, Clone)]
struct Particle {
    pos0: Vec3,
    vel0: Vec3,
    pos: Vec3,
    age: f32,
    size_scale: f32,
    cell: u32,
}

struct Effect {
    surface: Surface,
    spec: EffectSpec,
    tint: Vec3,
    particles: Vec<Particle>,
    /// Fractional particles owed per wheel.
    accum: [f32; 2],
    mesh: Handle<Mesh>,
}

#[derive(Resource)]
pub struct ParticleSystem {
    effects: Vec<Effect>,
    rng: u32,
}

impl ParticleSystem {
    fn rand(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / 16_777_216.0
    }
}

/// Track tint for an effect: all three channels from the track `gfx.cfg`, else the default.
fn track_tint(track_gfx: Option<&ConfigNode>, effect: &str, default: [f32; 3]) -> Vec3 {
    let block = track_gfx.and_then(|g| g.get_child("particles")).and_then(|p| p.get_child(effect));
    match block.map(|b| (b.get_f32("red"), b.get_f32("green"), b.get_f32("blue"))) {
        Some((Some(r), Some(g), Some(b))) => Vec3::new(r, g, b),
        _ => Vec3::from(default),
    }
}

fn load_texture(vfs: &MxBikesVfs, name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let bytes = vfs.read(&format!("effects/particles/{name}")).ok()?;
    let (width, height, rgba) = decode_tga_rgba(&bytes)?;
    let img = mxbikes_formats::texture::DecodedImage { width, height, layers: 1, rgba };
    Some(images.add(scene::rgba_image(&img, true, false)))
}

/// Builds the roost effects. `track_gfx` is the track's `gfx.cfg`.
pub fn spawn_particles(
    commands: &mut Commands,
    vfs: &MxBikesVfs,
    track_gfx: Option<&ConfigNode>,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mut effects = Vec::new();
    for (surface, name, default_tint) in SURFACE_EFFECTS {
        let Some(cfg) = read_cfg(vfs, &format!("effects/particles/{name}.cfg")) else {
            warn!("particle effect {name}.cfg not found");
            continue;
        };
        let spec = EffectSpec::from_cfg(&cfg);
        let texture = load_texture(vfs, &spec.texture, images);
        let material = materials.add(StandardMaterial {
            base_color_texture: texture,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
        mesh.insert_indices(Indices::U32(Vec::new()));
        let mesh = meshes.add(mesh);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::default(),
            NoFrustumCulling,
            crate::world::WorldEntity,
        ));
        effects.push(Effect {
            surface,
            tint: track_tint(track_gfx, name, default_tint),
            spec,
            particles: Vec::new(),
            accum: [0.0; 2],
            mesh,
        });
    }
    commands.insert_resource(ParticleSystem { effects, rng: 0x1234_5678 });
}

fn emit(sys: &mut ParticleSystem, effect: usize, wheel: usize, w: &WheelTelemetry, dt: f32) {
    let (Some(contact), Some(surface)) = (w.contact, w.surface) else {
        sys.effects[effect].accum[wheel] = 0.0;
        return;
    };
    let spec = &sys.effects[effect].spec;
    let v = w.rim_speed.abs();
    if surface != sys.effects[effect].surface || v < spec.min_speed || v <= 0.0 {
        sys.effects[effect].accum[wheel] = 0.0;
        return;
    }
    let n = w.normal;
    let t = (w.rolling - n * w.rolling.dot(n)).normalize_or_zero();
    let exit = spec.exit_speed_scale * v;
    let angle = spec.exit_angle.to_radians();
    let base = -t * exit * angle.cos() + n * exit * angle.sin() + w.velocity * spec.momentum_scale;
    let ratio = v / spec.size_speed_scale.max(1e-3);
    let rate = spec.frequency / ratio.max(1e-3);
    let size_scale = ratio.min(1.0);
    let variance = spec.exit_variance.to_radians();
    let cells = spec.cells.max(1);

    sys.effects[effect].accum[wheel] += rate * dt;
    while sys.effects[effect].accum[wheel] >= 1.0 {
        sys.effects[effect].accum[wheel] -= 1.0;
        if sys.effects[effect].particles.len() >= MAX_PARTICLES {
            continue;
        }
        // Cone distribution from FUN_140129a20 lines 51-64:
        // Polar angle theta = rand * exit_variance (radians)
        // Azimuth phi = rand * 2 * pi
        let dir = base.normalize_or_zero();
        let side = dir.any_orthonormal_vector();
        let up = dir.cross(side);
        let theta = sys.rand() * variance;
        let phi = sys.rand() * std::f32::consts::TAU;
        let vel = (dir * theta.cos() + side * (phi.cos() * theta.sin()) + up * (phi.sin() * theta.sin())) * base.length();
        let cell = ((sys.rand() * cells as f32) as u32).min(cells - 1);
        sys.effects[effect].particles.push(Particle { pos0: contact, vel0: vel, pos: contact, age: 0.0, size_scale, cell });
    }
}

pub fn update_particles(
    time: Res<Time>,
    sim: Res<BikeSim>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    sys: Option<ResMut<ParticleSystem>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut sys) = sys else { return };
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let wheels = [sim.0.front_telemetry(), sim.0.rear_telemetry()];
    for e in 0..sys.effects.len() {
        for (i, w) in wheels.iter().enumerate() {
            emit(&mut sys, e, i, w, dt);
        }
    }

    let Ok(cam) = camera.get_single() else { return };
    let (right, up) = (cam.rotation() * Vec3::X, cam.rotation() * Vec3::Y);
    for effect in &mut sys.effects {
        let spec = effect.spec.clone();
        // Analytical closed-form linear drag and gravity ballistics from FUN_140129a20 lines 85-97:
        // v(t) = v0 * exp(-friction * t), with downward terminal acceleration g / friction.
        // x(t) = x0 + (v0.x / k) * (1 - exp(-k * t))
        // z(t) = z0 + (v0.z / k) * (1 - exp(-k * t))
        // y(t) = y0 + (v0.y - g / k) * (1 / k) * (1 - exp(-k * t)) + (g / k) * t
        let k = spec.friction;
        let g = spec.gravity;
        for p in &mut effect.particles {
            p.age += dt;
            let t = p.age;
            if k > 1e-4 {
                let exp_kt = (-k * t).exp();
                let f = 1.0 - exp_kt;
                let gk = g / k;
                p.pos.x = p.pos0.x + (p.vel0.x / k) * f;
                p.pos.z = p.pos0.z + (p.vel0.z / k) * f;
                p.pos.y = p.pos0.y + (p.vel0.y - gk) * (1.0 / k) * f + gk * t;
            } else {
                p.pos.x = p.pos0.x + p.vel0.x * t;
                p.pos.z = p.pos0.z + p.vel0.z * t;
                p.pos.y = p.pos0.y + p.vel0.y * t + 0.5 * g * t * t;
            }
        }
        effect.particles.retain(|p| p.age < spec.max_life);

        let n = effect.particles.len();
        let (mut pos, mut uv, mut col, mut idx) =
            (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
        for (i, p) in effect.particles.iter().enumerate() {
            let t = (p.age / spec.max_life).clamp(0.0, 1.0);
            let half = 0.5 * p.size_scale * (spec.start_size + (spec.end_size - spec.start_size) * t);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                pos.push((p.pos + right * sx * half + up * sy * half).to_array());
            }
            let (u0, u1) = (p.cell as f32 / spec.cells as f32, (p.cell + 1) as f32 / spec.cells as f32);
            uv.extend_from_slice(&[[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]]);
            let c = spec.color(t) * effect.tint;
            let rgba = [c.x, c.y, c.z, spec.alpha(t)];
            col.extend_from_slice(&[rgba; 4]);
            let b = i as u32 * 4;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        if let Some(mesh) = meshes.get_mut(&effect.mesh) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            mesh.insert_indices(Indices::U32(idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_soil_spec_reads_from_text() {
        let cfg = ConfigNode::parse(
            "max_life = 6\ngravity = -7\nfriction = 0.6\nstart_size = 0.5\nend_size = 2.3\nnum_alpha_keys = 2\nalpha_key0\n{\ntime = 0\nalpha = 0\n}\nalpha_key1\n{\ntime = 0.05\nalpha = 1\n}\ntexture = soil.tga\ntexture_slots = 3\nmin_speed = 3\nexit_speed_scale = 0.6\nexit_angle = 10\nexit_variance = 20\nfrequency = 30\nsize_speed_scale = 60\nmomentum_scale = 0.6\n",
        )
        .unwrap();
        let s = EffectSpec::from_cfg(&cfg);
        assert_eq!(s.cells, 8);
        assert!((s.alpha(0.025) - 0.5).abs() < 1e-5);
        assert_eq!(s.alpha(0.5), 1.0);
    }
}
