//! Loads a track and a bike from the mounted game files and spawns them.

use crate::bike_entity::{BikeParts, BikeSim};
use crate::setup::{BikeSetup, TyreChoice};
use crate::rider;
use crate::scene::{self, MaterialSet};
use crate::terrain_material::{TerrainExtension, TerrainMaterial, TerrainParams};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::mesh::skinning::SkinnedMeshInverseBindposes;
use mxbikes_formats::assets::{self, read_cfg, resolve_relative};
use mxbikes_formats::map::TrackMap;
use mxbikes_formats::reader::lh_to_rh;
use mxbikes_formats::{ConfigNode, DecodedImage, EdfModel, PaintFile, TrackTerrain};
use mxbikes_physics::{Motorcycle, SurfaceMask, TerrainGround};
use mxbikes_vfs::MxBikesVfs;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Resource, Clone)]
pub struct LaunchOptions {
    pub track: String,
    pub bike: String,
    pub screenshot: Option<(String, f32)>,
    pub camera: String,
    /// Scripted input for unattended runs: first gear and part throttle after 2 s.
    pub autopilot: bool,
    /// Steering held by the autopilot (-1 left .. 1 right).
    pub autopilot_steer: f32,
    /// Throttle held by the autopilot (default 0.6).
    pub autopilot_throttle: f32,
    /// Rider aid `autoshift` (game `profile.ini`): shift from the bike's `autoshift` rpm table.
    pub autoshift: bool,
    /// Menu shown at start: `main`, `pit` or `garage`.
    pub menu: Option<String>,
    /// Paint per part (`bike`, `rider`, `gloves`, `helmet`, `goggles`, `boots`): `.pnt` file stem.
    /// Parts without one keep the textures embedded in their models.
    pub paints: HashMap<String, String>,
    /// Race number for the plates and suit (from the profile in the game; empty draws none).
    pub number: String,
    /// Rider name for the suit back lettering (from the profile in the game; empty draws none).
    pub rider_name: String,
    /// Font set for numbers and letters ("default_black" or "default_white").
    pub font: String,
    /// Mud build-up accumulation factor override (0.0 clean .. 1.0 caked).
    pub dirt: Option<f32>,
}

/// Marker component attached to all in-game 3D world entities (track meshes, terrain chunks,
/// directional lights, skybox, bike parts, and audio voices). Enables clean in-place teardown.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WorldEntity;

/// Event sent to reload the track and bike in-place without process restart.
#[derive(Event, Debug, Clone)]
pub struct ReloadSession {
    pub track: String,
    pub bike: String,
    pub paints: HashMap<String, String>,
}

#[derive(Resource, Clone, Copy)]
pub struct TrackDirtColor(pub Vec3);

#[derive(Debug, Clone)]
pub struct TrackGate {
    pub name: String,
    pub left: Vec2,
    pub right: Vec2,
    pub long: f32,
    /// Direction of travel along the centreline at the gate (engine x, z).
    pub forward: Vec2,
}

#[derive(Resource, Clone, Default)]
pub struct TrackTiming {
    pub finish_line: Option<TrackGate>,
    pub splits: Vec<TrackGate>,
    pub track_length: f32,
}

#[derive(Resource, Default)]
pub struct LapTimer {
    pub lap: u32,
    /// Timing starts at the first finish crossing (the out-lap from the pits is not timed).
    pub started: bool,
    pub current_time: f32,
    pub best_time: Option<f32>,
    pub last_lap_time: Option<f32>,
    pub split_times: Vec<Option<f32>>,
    pub prev_pos: Option<Vec2>,
    /// Per gate (finish, then splits): crossed backwards; the next forward crossing only clears it.
    pub wrong_way: Vec<bool>,
    /// Timed laps: lap time, split times from the lap start, top speed (m/s).
    pub laps: Vec<LapRecord>,
    /// Top speed of the lap in progress (m/s).
    pub lap_top_speed: f32,
}

#[derive(Debug, Clone)]
pub struct LapRecord {
    pub time: f32,
    pub splits: Vec<Option<f32>>,
    pub top_speed: f32,
}

impl LapTimer {
    /// Ideal lap: the sum of the best time of each sector (lap start, splits, finish).
    pub fn ideal(&self) -> Option<f32> {
        let sectors = self.laps.first()?.splits.len() + 1;
        let mut total = 0.0;
        for k in 0..sectors {
            let best = self
                .laps
                .iter()
                .filter_map(|l| {
                    let start = if k == 0 { Some(0.0) } else { l.splits.get(k - 1).copied().flatten() };
                    let end = if k + 1 == sectors { Some(l.time) } else { l.splits.get(k).copied().flatten() };
                    Some(end? - start?)
                })
                .fold(f32::INFINITY, f32::min);
            if !best.is_finite() {
                return None;
            }
            total += best;
        }
        Some(total)
    }
}

/// Gate crossing as the game tests it (FUN_14010c7a0): the movement segment against the gate
/// segment, returning the fraction of the step at the crossing and whether it was forward.
fn crossing(prev: Vec2, curr: Vec2, gate: &TrackGate) -> Option<(f32, bool)> {
    let (r, s) = (curr - prev, gate.right - gate.left);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-9 {
        return None;
    }
    let q = gate.left - prev;
    let (t, u) = (q.perp_dot(s) / denom, q.perp_dot(r) / denom);
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| (t, r.dot(gate.forward) > 0.0))
}

/// Track direction at `long`, from the centreline (engine x, z).
fn gate_forward(terrain: &TrackTerrain, long: f32) -> Vec2 {
    match (terrain.track_to_world(long - 0.5, 0.0), terrain.track_to_world(long + 0.5, 0.0)) {
        (Some(a), Some(b)) => Vec2::new(b.x - a.x, b.z - a.z).normalize_or_zero(),
        _ => Vec2::ZERO,
    }
}

pub fn update_lap_timing(
    time: Res<Time>,
    timing: Option<Res<TrackTiming>>,
    sim: Option<Res<crate::bike_entity::BikeSim>>,
    timer: Option<ResMut<LapTimer>>,
) {
    let (Some(timing), Some(sim), Some(mut timer)) = (timing, sim, timer) else { return };
    let dt = time.delta_secs();
    let p = sim.0.chassis_transform().0;
    let curr = Vec2::new(p.x, p.z);
    let gates = timing.finish_line.iter().chain(timing.splits.iter()).count();
    if timer.wrong_way.len() != gates {
        timer.wrong_way = vec![false; gates];
        timer.split_times = vec![None; timing.splits.len()];
    }
    let Some(prev) = timer.prev_pos.replace(curr) else { return };
    let t0 = timer.current_time;

    // A forward crossing counts unless the gate was crossed backwards before.
    let pass = |timer: &mut LapTimer, idx: usize, gate: &TrackGate| -> Option<f32> {
        let (frac, forward) = crossing(prev, curr, gate)?;
        if !forward {
            timer.wrong_way[idx] = true;
            return None;
        }
        if std::mem::take(&mut timer.wrong_way[idx]) {
            return None;
        }
        Some(t0 + frac * dt)
    };
    for (i, split) in timing.splits.iter().enumerate() {
        if let Some(t) = pass(&mut timer, i + timing.finish_line.is_some() as usize, split) {
            if timer.started {
                timer.split_times[i] = Some(t);
            }
        }
    }
    timer.current_time += dt;
    let speed = sim.0.speed();
    timer.lap_top_speed = timer.lap_top_speed.max(speed);
    if let Some(finish) = &timing.finish_line {
        if let Some(t) = pass(&mut timer, 0, finish) {
            if timer.started {
                timer.last_lap_time = Some(t);
                timer.best_time = Some(timer.best_time.map_or(t, |b| b.min(t)));
                timer.lap += 1;
                let record = LapRecord { time: t, splits: timer.split_times.clone(), top_speed: timer.lap_top_speed };
                timer.laps.push(record);
            }
            timer.lap_top_speed = 0.0;
            timer.started = true;
            timer.current_time -= t;
            timer.split_times.fill(None);
        }
    }
}

#[derive(Resource)]
pub struct Vfs(pub Arc<MxBikesVfs>);

#[derive(Resource)]
pub struct TrackGround(pub Arc<TerrainGround>);

/// What the track info dialog shows (`FUN_140118940`): the track `.ini` `[ui]` author, location
/// and map picture, `[info]` length and altitude, and the pit stall count.
#[derive(Resource, Debug, Clone, Default)]
pub struct TrackInfo {
    pub name: String,
    pub author: String,
    pub location: String,
    pub length: f32,
    pub altitude: f32,
    pub pits: usize,
    /// VFS paths of the `[ui]` pictures (`pic`, `pic_info`).
    pub picture: Option<String>,
    pub map: Option<String>,
}

impl TrackInfo {
    pub fn load(vfs: &MxBikesVfs, stem: &str) -> Self {
        let folder = stem.rsplit_once('/').map(|(f, _)| f).unwrap_or("");
        let ini = read_cfg(vfs, &format!("{stem}.ini")).unwrap_or_default();
        let ui = ini.get_child("ui");
        let info = ini.get_child("info");
        let s = |n: Option<&ConfigNode>, k: &str| n.and_then(|n| n.get_str(k)).unwrap_or("").to_string();
        let pic = |k: &str| ui.and_then(|u| u.get_str(k)).filter(|p| !p.is_empty()).map(|p| format!("{folder}/{p}"));
        // INFERRED: the pit count is the number of `pit_lane/start_stall<n>` entries of the .rdf.
        let pits = read_cfg(vfs, &format!("{stem}.rdf"))
            .and_then(|r| r.get_child("pit_lane").map(|p| (0..).take_while(|i| p.get_child(&format!("start_stall{i}")).is_some()).count()))
            .unwrap_or(0);
        Self {
            name: s(info, "name"),
            author: s(ui, "author"),
            location: s(ui, "location"),
            length: info.and_then(|i| i.get_f32("length")).unwrap_or(0.0),
            altitude: info.and_then(|i| i.get_f32("altitude")).unwrap_or(0.0),
            pits,
            picture: pic("pic"),
            map: pic("pic_info"),
        }
    }
}

#[derive(Resource, Clone, Copy)]
pub struct SpawnPoint {
    pub position: Vec3,
    pub heading: f32,
}

/// Sky entity that follows the camera.
#[derive(Component)]
pub struct Sky;

/// Terrain chunk grid coordinates for dynamic rut mesh updates.
#[derive(Component, Debug, Clone, Copy)]
pub struct TerrainChunk {
    pub cx: u32,
    pub cz: u32,
    pub nx: u32,
    pub nz: u32,
    pub step: u32,
}

#[derive(Resource, Clone, Copy)]
pub struct Atmosphere {
    pub fog_color: Color,
    pub fog_density: f32,
}

fn rgb(node: Option<&ConfigNode>, fallback: [f32; 3]) -> [f32; 3] {
    node.map(|n| {
        [
            n.get_f32("red").unwrap_or(fallback[0]),
            n.get_f32("green").unwrap_or(fallback[1]),
            n.get_f32("blue").unwrap_or(fallback[2]),
        ]
    })
    .unwrap_or(fallback)
}

#[derive(Debug, Clone)]
pub struct LodLevel {
    pub scene: String,
    pub name: String,
    pub switch_dist: f32,
}

pub(crate) fn parse_hrc(vfs: &MxBikesVfs, folder: &str, file: &str) -> Vec<LodLevel> {
    let Some(node) = read_cfg(vfs, &format!("{folder}/{file}")) else {
        return Vec::new();
    };
    let mut levels = Vec::new();
    let mut i = 0;
    while let Some(lvl) = node.get_child(&format!("level{i}")) {
        if let Some(scene) = lvl.get_str("scene") {
            let name = lvl.get_str("name").unwrap_or("").to_string();
            let switch_dist = lvl.get_f32("switch").unwrap_or(0.0);
            levels.push(LodLevel {
                scene: scene.to_string(),
                name,
                switch_dist,
            });
        }
        i += 1;
    }
    levels
}

/// `.hrc` level 0: (scene file, group name).
pub(crate) fn hrc_level0(vfs: &MxBikesVfs, folder: &str, file: &str) -> Option<(String, String)> {
    parse_hrc(vfs, folder, file).first().map(|l| (l.scene.clone(), l.name.clone()))
}

/// Dynamic LOD switching container for bike and gear parts.
#[derive(Component, Debug, Clone)]
pub struct PartLod {
    pub levels: Vec<(Entity, f32)>,
}

/// Diffuse replacements from a `.pnt` file, keyed by lower-case texture name.
pub(crate) fn load_paint(vfs: &MxBikesVfs, path: &str) -> HashMap<String, DecodedImage> {
    let mut out = HashMap::new();
    let data = match vfs.read(path) {
        Ok(d) => d,
        Err(e) => {
            warn!("paint {path}: {e}");
            return out;
        }
    };
    match PaintFile::parse(&data) {
        Ok(p) => {
            for t in &p.textures {
                match t.decode_frame(&data, 0) {
                    Ok(img) => {
                        out.insert(t.name.to_ascii_lowercase(), img);
                    }
                    Err(e) => warn!("paint {path} texture '{}': {e}", t.name),
                }
            }
        }
        Err(e) => warn!("paint {path}: {e}"),
    }
    out
}

pub(crate) fn load_edf(vfs: &MxBikesVfs, path: &str) -> Option<(Arc<Vec<u8>>, EdfModel)> {
    let data = vfs.read(path).map_err(|e| warn!("{path}: {e}")).ok()?;
    let model = EdfModel::parse(&data).map_err(|e| warn!("{path}: {e}")).ok()?;
    Some((Arc::new(data), model))
}

#[allow(clippy::too_many_arguments)]
pub fn setup_world(
    mut commands: Commands,
    options: Res<LaunchOptions>,
    vfs: Res<Vfs>,
    profile: Option<Res<crate::profile::Profile>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    // Gamepad bindings: the game's XInput controller profile.
    let pad_cfg = std::fs::read_to_string(vfs.0.game_dir().join("controllers").join("xinput.cfg"))
        .ok()
        .and_then(|t| ConfigNode::parse(&t).ok())
        .unwrap_or_default();
    commands.insert_resource(crate::input::PadProfile::from_cfg(&pad_cfg));

    spawn_world_internal(
        &mut commands,
        &options,
        &vfs.0,
        profile.as_deref(),
        &mut meshes,
        &mut images,
        &mut std_materials,
        &mut terrain_materials,
        &mut bindposes,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_world_internal(
    commands: &mut Commands,
    options: &LaunchOptions,
    vfs: &MxBikesVfs,
    profile: Option<&crate::profile::Profile>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    terrain_materials: &mut Assets<TerrainMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) {
    let stem = assets::find_track(vfs, &options.track).unwrap_or_else(|| {
        error!("track '{}' not found; available: {:?}", options.track, assets::track_paths(vfs));
        std::process::exit(1);
    });
    let folder = stem.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default();
    info!("loading track {stem}");

    // --- Terrain heightfield.
    let terrain = if stem == "misc/bg/bg" {
        TrackTerrain::flat(200.0, 200.0)
    } else {
        TrackTerrain::parse(&vfs.read(&format!("{stem}.trh")).expect("track .trh"))
            .expect("valid .trh")
    };

    // --- Scenery + terrain layers.
    let map = assets::track_map_path(&vfs, &stem).and_then(|p| {
        let data = vfs.read(&p).ok()?;
        match TrackMap::parse(&data, Some((terrain.size_x, terrain.size_z, terrain.height_range))) {
            Ok(m) => Some((Arc::new(data), m)),
            Err(e) => {
                warn!("{p}: {e}");
                None
            }
        }
    });

    let mut ground = TerrainGround::new(terrain.clone());
    let white = images.add(scene::mask_image(&mxbikes_formats::DecodedImage {
        width: 1,
        height: 1,
        layers: 1,
        rgba: vec![0, 0, 0, 255],
    }));
    let mut layer_images = vec![white.clone(); 4];
    let mut mask_images = vec![white.clone(); 3];
    let mut tiling_u = Vec4::splat(40.0);
    let mut tiling_v = Vec4::splat(40.0);
    let mut layer_count = 1;

    if let Some((data, map)) = &map {
        if let Some(t) = &map.terrain {
            layer_count = t.layers.len().min(4) as u32;
            let diffuse: Vec<_> = t.layers.iter().take(4).map(|l| &l.variants[0].diffuse).collect();
            for (i, img) in scene::decode_parallel(data, &diffuse).into_iter().enumerate() {
                if let Some(img) = img {
                    layer_images[i] = images.add(scene::rgba_image(&img, true, true));
                }
                tiling_u[i] = t.layers[i].tiling[0];
                tiling_v[i] = t.layers[i].tiling[1];
            }
            for (i, layer) in t.layers.iter().enumerate().skip(1).take(3) {
                let Some(mask) = &layer.mask else { continue };
                match mask.blob.decode(data, mask.width, mask.height) {
                    Ok(img) => {
                        if let Some(l) = ground.layers.get_mut(i).filter(|l| l.1.is_none()) {
                            l.1 = Some(SurfaceMask {
                                width: img.width,
                                height: img.height,
                                coverage: img.rgba.chunks_exact(4).map(|p| p[0]).collect(),
                            });
                        }
                        mask_images[i - 1] = images.add(scene::mask_image(&img));
                    }
                    Err(e) => warn!("terrain mask {i}: {e}"),
                }
            }
        }

        let set = scene::build_materials(data, &map.materials, images, std_materials, &HashMap::new());
        // INFERRED: in the old "MAP" format (the menu background) a material without flag 0x200
        // is drawn without lighting; that is the backdrop gradient (0x500), while the lit ground
        // has 0x700. No other stock map uses these bits.
        if map.version == 0 {
            for (i, m) in map.materials.iter().enumerate() {
                let Some(mat) = std_materials.get_mut(&set.get(i as u32 + 1)) else { continue };
                if m.diffuse.flags & 0x200 == 0 {
                    mat.unlit = true;
                    mat.alpha_mode = AlphaMode::Opaque;
                    mat.cull_mode = None;
                } else if m.diffuse.flags & 0x400 != 0 {
                    // The ground patch fades out through its texture alpha.
                    mat.alpha_mode = AlphaMode::Blend;
                }
            }
        }
        let s = &map.scenery;
        for (slot, mesh) in scene::scenery_meshes(&s.vertices, &s.indices, &s.submeshes) {
            let tex = s.materials.get(slot as usize).map(|m| m.texture).unwrap_or(0);
            commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(set.get(tex)), Transform::default(), WorldEntity));
        }
    }

    let terrain_mat = terrain_materials.add(TerrainMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.95,
            reflectance: 0.2,
            ..default()
        },
        extension: TerrainExtension {
            params: TerrainParams {
                tiling_u,
                tiling_v,
                extent: Vec4::new(terrain.origin.x, terrain.origin.z, terrain.size_x, terrain.size_z),
                layers: layer_count,
            },
            layer0: layer_images[0].clone(),
            layer1: layer_images[1].clone(),
            layer2: layer_images[2].clone(),
            layer3: layer_images[3].clone(),
            mask1: mask_images[0].clone(),
            mask2: mask_images[1].clone(),
            mask3: mask_images[2].clone(),
        },
    });
    if map.as_ref().and_then(|(_, m)| m.terrain.as_ref()).is_some() {
        let step = if terrain.width > 2049 { 4 } else { 2 };
        for (cx, cz, nx, nz, mesh) in scene::terrain_chunks(&terrain, step, 128) {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(terrain_mat.clone()),
                Transform::default(),
                TerrainChunk { cx, cz, nx, nz, step },
                WorldEntity,
            ));
        }
    }

    // --- Atmosphere: sun, ambient, fog, sky from `<track>.amb` (clear weather).
    let amb = read_cfg(vfs, &format!("{stem}.amb")).or_else(|| {
        let name = folder.rsplit('/').next().unwrap_or("");
        read_cfg(vfs, &format!("{folder}/{name}.amb"))
    });
    let clear = amb.as_ref().and_then(|a| a.get_child("clear"));
    let light_node = amb.as_ref().and_then(|a| a.get_child("light"));
    let sun_pos = amb
        .as_ref()
        .and_then(|a| a.get_child("sun_position"))
        .map(|n| Vec3::new(n.get_f32("x").unwrap_or(2.0), n.get_f32("y").unwrap_or(8.0), -n.get_f32("z").unwrap_or(-7.0)))
        .or_else(|| {
            light_node.and_then(|l| l.get_child("direction")).map(|d| {
                Vec3::new(d.get_f32("x").unwrap_or(-0.6), d.get_f32("y").unwrap_or(3.0), -d.get_f32("z").unwrap_or(-0.4))
            })
        })
        .unwrap_or(Vec3::new(2.0, 8.0, 7.0))
        .normalize();

    let sun = light_node
        .and_then(|l| l.get_child("color"))
        .map(|c| rgb(Some(&c), [0.6, 0.58, 0.57]))
        .unwrap_or_else(|| rgb(clear.and_then(|c| c.get_child("sun_color")), [1.1, 0.95, 0.7]));

    let ambient = amb
        .as_ref()
        .and_then(|a| a.get_child("ambient"))
        .map(|c| rgb(Some(&c), [0.7, 0.7, 0.8]))
        .unwrap_or_else(|| rgb(clear.and_then(|c| c.get_child("ambient")), [0.4, 0.45, 0.55]));

    let fog = clear.and_then(|c| c.get_child("fog"));
    let fog_rgb = rgb(fog, [0.7, 0.7, 0.85]);
    commands.insert_resource(Atmosphere {
        fog_color: Color::linear_rgb(fog_rgb[0], fog_rgb[1], fog_rgb[2]),
        fog_density: fog.and_then(|f| f.get_f32("density")).unwrap_or(0.0008),
    });
    // The game lights with the fixed-function model, texture * (ambient + light * N.L) clamped,
    // using the `.amb` colours directly. With the camera at exposure 1 and no tonemapping
    // (main.rs), a directional illuminance of pi (Bevy's Lambert term divides by pi) gives the
    // light multiplier; an ambient brightness of pi matched the game's menu picture (orange
    // fender texture 233 / 168 drawn at 207 / 170 in the game, 228 / 163 here).
    commands.insert_resource(AmbientLight {
        color: Color::linear_rgb(ambient[0], ambient[1], ambient[2]),
        brightness: std::f32::consts::PI,
    });
    let shadows_enabled = profile.map_or(true, |p| p.i("gfx", "shadow_disable") == 0);
    commands.spawn((
        DirectionalLight {
            color: Color::linear_rgb(sun[0], sun[1], sun[2]),
            illuminance: std::f32::consts::PI,
            shadows_enabled,
            ..default()
        },
        Transform::default().looking_to(-sun_pos, Vec3::Y),
        WorldEntity,
    ));

    if let Some(sky_file) = clear.and_then(|c| c.get_str("sky")).or_else(|| (stem != "misc/bg/bg").then_some("*clearsky.edf")) {
        let sky_rot = clear.and_then(|c| c.get_f32("sky_rot")).unwrap_or(0.0);
        if let Some(sky_path) = resolve_relative(vfs, &folder, sky_file) {
            if let Some((data, model)) = load_edf(vfs, &sky_path) {
                let set = scene::build_materials(&data, &model.materials, images, std_materials, &HashMap::new());
                let unlit = unlit_copy(&set, std_materials);
                let e = scene::spawn_group(commands, &model, "", &unlit, meshes, "sky");
                commands
                    .entity(e)
                    .insert((Sky, Transform::from_rotation(Quat::from_rotation_y(-sky_rot.to_radians())), WorldEntity));
            }
        }
    }
    if let Some(bg) = amb.as_ref().and_then(|a| a.get_str("background")) {
        if let Some(p) = resolve_relative(vfs, &folder, bg) {
            if let Some((data, model)) = load_edf(vfs, &p) {
                let set = scene::build_materials(&data, &model.materials, images, std_materials, &HashMap::new());
                let e = scene::spawn_group(commands, &model, "", &set, meshes, "background");
                commands.entity(e).insert(WorldEntity);
            }
        }
    }

    // --- Spawn: like the game, pit stall 0 in track coordinates through the TRH centreline.
    let def = assets::load_track_definition(&vfs, &stem);
    commands.insert_resource(TrackInfo::load(&vfs, &stem));
    let track_gfx = read_cfg(&vfs, &format!("{folder}/gfx.cfg"));
    // As the game (FUN_14004ebc0): all three channels from the track, else all three defaults.
    let track_dirt_color = match track_gfx.as_ref().and_then(|g| g.get_child("dirt_color")).map(|dc| {
        (dc.get_f32("red"), dc.get_f32("green"), dc.get_f32("blue"))
    }) {
        Some((Some(r), Some(g), Some(b))) => Vec3::new(r, g, b),
        _ => Vec3::new(0.161_000, 0.122_000, 0.090_000),
    };
    commands.insert_resource(TrackDirtColor(track_dirt_color));
    crate::particles::spawn_particles(commands, &vfs, track_gfx.as_ref(), images, meshes, std_materials);

    let timing = def.as_ref().map(|d| {
        let finish_line = d.finish_line.as_ref().and_then(|g| {
            let left = terrain.track_to_world(g.longitudinal_pos, g.left_bound)?;
            let right = terrain.track_to_world(g.longitudinal_pos, g.right_bound)?;
            Some(TrackGate {
                name: g.name.clone(),
                left: Vec2::new(left.x, left.z),
                right: Vec2::new(right.x, right.z),
                long: g.longitudinal_pos,
                forward: gate_forward(&terrain, g.longitudinal_pos),
            })
        });
        let splits = d.splits.iter().filter_map(|g| {
            let left = terrain.track_to_world(g.longitudinal_pos, g.left_bound)?;
            let right = terrain.track_to_world(g.longitudinal_pos, g.right_bound)?;
            Some(TrackGate {
                name: g.name.clone(),
                left: Vec2::new(left.x, left.z),
                right: Vec2::new(right.x, right.z),
                long: g.longitudinal_pos,
                forward: gate_forward(&terrain, g.longitudinal_pos),
            })
        }).collect();
        TrackTiming {
            finish_line,
            splits,
            track_length: d.length,
        }
    }).unwrap_or_default();
    commands.insert_resource(timing);
    commands.insert_resource(LapTimer::default());

    let start = def.as_ref().and_then(|d| d.start.clone());
    let from_stall = start.as_ref().and_then(|g| {
        let stall = g.stalls.first()?;
        let line = terrain.centerline.as_ref()?;
        let pos = terrain.track_to_world(stall.long, stall.lat)?;
        let heading_asset = line.heading_deg(stall.long) + stall.angle_deg;
        Some((pos, -heading_asset.to_radians()))
    });
    let (position, heading) = from_stall
        .or_else(|| {
            start.map(|g| {
                let (x, z) = g.slot(0);
                (Vec3::new(x, terrain.height_at(x, -z), -z), -g.angle_deg.to_radians())
            })
        })
        .unwrap_or((terrain.center(), 0.0));
    let spawn = SpawnPoint { position, heading };
    commands.insert_resource(spawn);
    if let Some(d) = &def {
        info!("{} ({} m)", d.name, d.length);
    }

    // --- Bike.
    spawn_bike(commands, &vfs, &options, profile.as_deref(), track_dirt_color, spawn, meshes, images, std_materials, bindposes);
    commands.insert_resource(TrackGround(Arc::new(ground)));
}

fn unlit_copy(set: &MaterialSet, mats: &mut Assets<StandardMaterial>) -> MaterialSet {
    let mut by_ref = HashMap::new();
    for (k, h) in &set.by_ref {
        if let Some(m) = mats.get(h).cloned() {
            by_ref.insert(
                *k,
                mats.add(StandardMaterial {
                    unlit: true,
                    fog_enabled: false,
                    cull_mode: None,
                    ..m
                }),
            );
        }
    }
    MaterialSet {
        by_ref,
        untextured: set.untextured.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_bike(
    commands: &mut Commands,
    vfs: &MxBikesVfs,
    options: &LaunchOptions,
    profile: Option<&crate::profile::Profile>,
    track_dirt_color: Vec3,
    spawn: SpawnPoint,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) {
    let (id, autoshift) = (options.bike.as_str(), options.autoshift);
    // Without a chosen paint the model's own textures show, as in the game's menu.
    let mut bike_paint = match options.paints.get("bike") {
        Some(name) => load_paint(vfs, &format!("bikes/{id}/paints/{name}.pnt")),
        None => HashMap::new(),
    };
    let plates = crate::plate::render_plates(vfs, &options.font, &options.number, &options.rider_name);
    bike_paint.extend(plates.clone());
    let def = assets::load_bike(vfs, id).unwrap_or_else(|e| {
        error!("bike '{id}': {e}; available: {:?}", assets::bike_ids(vfs));
        std::process::exit(1);
    });
    let tyre_set = assets::tyre_set_for(vfs, &def.tyres_type).unwrap_or_else(|| "p_mx".into());
    let tyres = assets::load_tyres(vfs, &tyre_set).expect("tyre definitions");
    let bike_folder = format!("bikes/{id}");
    let gfx = read_cfg(vfs, &format!("{bike_folder}/gfx.cfg")).unwrap_or_default();
    let tyre_folder = format!("tyres/{tyre_set}");
    let tyre_gfx = read_cfg(vfs, &format!("{tyre_folder}/gfx.cfg")).unwrap_or_default();

    let bike_dirt_tint = if gfx.get_i64("dirt_color").unwrap_or(0) != 0 {
        track_dirt_color
    } else {
        Vec3::ONE
    };
    let tyre_dirt_tint = if tyre_gfx.get_i64("dirt_color").unwrap_or(0) != 0 {
        track_dirt_color
    } else {
        Vec3::ONE
    };
    let dirt_factor = options.dirt.unwrap_or(0.0);

    let part_levels = |block: &str, fallback: &str| -> Vec<LodLevel> {
        let file = gfx
            .get_child(block)
            .and_then(|b| b.get_child("model"))
            .and_then(|m| m.get_str("file"))
            .unwrap_or(fallback)
            .to_string();
        let levels = parse_hrc(vfs, &bike_folder, &file);
        if levels.is_empty() {
            vec![LodLevel { scene: format!("{id}.edf"), name: String::new(), switch_dist: 0.0 }]
        } else {
            levels
        }
    };

    let mut cache: HashMap<String, (EdfModel, MaterialSet)> = HashMap::new();
    let mut spawn_part_lod = |commands: &mut Commands,
                              folder: &str,
                              levels: Vec<LodLevel>,
                              name: &str,
                              pred: &dyn Fn(&str) -> bool|
     -> Entity {
        if levels.is_empty() {
            return commands.spawn((Transform::default(), Visibility::default(), WorldEntity)).id();
        }
        let parent = commands
            .spawn((Transform::default(), Visibility::default(), Name::new(name.to_string()), WorldEntity))
            .id();
        let mut lod_entities = Vec::new();

        for (idx, lvl) in levels.into_iter().enumerate() {
            let path = format!("{folder}/{}", lvl.scene);
            if !cache.contains_key(&path) {
                if let Some((data, model)) = load_edf(vfs, &path) {
                    let tint = if folder.starts_with("tyres") { tyre_dirt_tint } else { bike_dirt_tint };
                    let mud = Some(scene::MudConfig { level: mxbikes_physics::motorcycle::dirt_level(dirt_factor), color: tint });
                    let set = scene::build_materials_with_mud(&data, &model.materials, images, std_materials, &bike_paint, mud);
                    cache.insert(path.clone(), (model, set));
                }
            }
            if let Some((model, set)) = cache.get(&path) {
                let initial_vis = if idx == 0 { Visibility::Inherited } else { Visibility::Hidden };
                let child = scene::spawn_group_filtered(commands, model, &lvl.name, set, meshes, &format!("{name}_lod{idx}"), pred);
                commands.entity(child).insert(initial_vis);
                commands.entity(parent).add_child(child);
                lod_entities.push((child, lvl.switch_dist));
            }
        }

        if lod_entities.len() > 1 {
            commands.entity(parent).insert(PartLod { levels: lod_entities });
        }
        parent
    };

    let chassis = spawn_part_lod(commands, &bike_folder, part_levels("chassis", "chassis.hrc"), "chassis", &|_| true);
    let steer = spawn_part_lod(commands, &bike_folder, part_levels("steer", "steer.hrc"), "steer", &|_| true);
    let fork = spawn_part_lod(commands, &bike_folder, part_levels("front_susp", "fsusp.hrc"), "fork", &|_| true);
    let swingarm = spawn_part_lod(commands, &bike_folder, part_levels("rear_susp", "rsusp.hrc"), "swingarm", &|_| true);

    let tyre_folder = format!("tyres/{tyre_set}");
    let tyre_gfx = read_cfg(vfs, &format!("{tyre_folder}/gfx.cfg")).unwrap_or_default();
    let wheel_levels = |block: &str, fallback: &str| -> Vec<LodLevel> {
        let file = tyre_gfx
            .get_child(block)
            .and_then(|b| b.get_child("model"))
            .and_then(|m| m.get_str("file"))
            .unwrap_or(fallback)
            .to_string();
        let levels = parse_hrc(vfs, &tyre_folder, &file);
        if levels.is_empty() {
            vec![LodLevel { scene: format!("{tyre_set}.edf"), name: String::new(), switch_dist: 0.0 }]
        } else {
            levels
        }
    };
    let front_wheel = spawn_part_lod(commands, &tyre_folder, wheel_levels("front_wheel", "fwheel.hrc"), "front_wheel", &|_| true);
    let rear_wheel = spawn_part_lod(
        commands,
        &tyre_folder,
        wheel_levels("rear_wheel", "rwheel.hrc"),
        "rear_wheel",
        &|name| !name.eq_ignore_ascii_case("chain"),
    );
    let rear_axle = spawn_part_lod(
        commands,
        &tyre_folder,
        wheel_levels("rear_wheel", "rwheel.hrc"),
        "rear_axle",
        &|name| name.eq_ignore_ascii_case("chain"),
    );

    let grips = ["leftgrip", "rightgrip"].map(|g| {
        let p = gfx.get_child("steer")?.get_child(g)?.get_child("pos")?;
        Some(lh_to_rh(Vec3::new(p.get_f32("x")?, p.get_f32("y")?, p.get_f32("z")?)))
    });
    let menu = options.track == "bg" || options.track == "menu";
    let mut rig = rider::spawn_rider(commands, vfs, chassis, options, track_dirt_color, grips, menu, meshes, images, std_materials, bindposes);

    let mut bike = Motorcycle::new(def, tyres, spawn.position, spawn.heading);
    bike.place_on(spawn.position, spawn.heading);
    if menu {
        // Main menu (`FUN_1400e8790`, `FUN_1400455c0`): the bike stands on `misc/stand` with its
        // `.geom` `stand_linkfront` / `stand_linkrear` points on the stand top (`stand.ini`
        // [data] height), suspension extended; the rider stands beside it.
        let geom_name = read_cfg(vfs, &format!("bikes/{id}/{id}.cfg"))
            .and_then(|c| c.get_str("geom").map(str::to_owned))
            .unwrap_or(format!("{id}.geom"));
        let geom = read_cfg(vfs, &format!("bikes/{id}/{geom_name}"));
        let link = |k: &str| geom.as_ref().and_then(|g| g.get_vec3(k)).map(lh_to_rh);
        let stand_height = read_cfg(vfs, "misc/stand/stand.ini")
            .and_then(|c| c.get_child("data").and_then(|d| d.get_f32("height")))
            .unwrap_or(0.405);
        let mid = match (link("stand_linkfront"), link("stand_linkrear")) {
            (Some(f), Some(r)) => (f + r) * 0.5,
            _ => Vec3::new(0.0, 0.33, 0.0),
        };
        // INFERRED: the scene turns the bike to face the other way from the spawn heading (the
        // angle passed to the scene rotation in `FUN_1400e7cd0` is lost in the decompile; 180
        // degrees matches the game's menu picture).
        let heading = spawn.heading + std::f32::consts::PI;
        bike.place_on(spawn.position, heading);
        let rot = Quat::from_rotation_y(heading);
        bike.extend_suspension();
        let stand_pos = Vec3::new(spawn.position.x, spawn.position.y, spawn.position.z);
        let origin = stand_pos + Vec3::Y * (stand_height - mid.y) - rot * Vec3::new(mid.x, 0.0, mid.z);
        bike.set_chassis_origin(origin);
        if let Some((data, model)) = load_edf(vfs, "misc/stand/stand.edf") {
            let set = scene::build_materials(&data, &model.materials, images, std_materials, &bike_paint);
            let e = scene::spawn_group(commands, &model, "", &set, meshes, "stand");
            commands.entity(e).insert((Transform::from_translation(stand_pos).with_rotation(rot), WorldEntity));
        }
        // The rider root sits at (`ui_pos/lat` - 0.7, `ui_pos/vert`, 0) from the stand
        // (`FUN_1400e7cd0`), on the ground; the idle clip itself moves the body beside the bike.
        if let Some(r) = rig.as_mut() {
            let (lat, vert) = r.menu_ui_pos().unwrap_or((0.62, 0.0));
            r.set_menu_root(Vec3::new(lat - 0.7 + mid.x, vert - (stand_height - mid.y), mid.z));
        }
    }
    match rig {
        Some(rig) => commands.insert_resource(rig),
        None => warn!("rider model not loaded"),
    }
    // `--dirt` sets the starting dirt amount (0.1666 / 0.5 / 0.8333 are the mud stage cuts).
    bike.dirt_factor = dirt_factor;
    bike.rider_dirt_factor = dirt_factor;
    if let Some(p) = profile {
        p.apply_to_bike(&mut bike);
    }
    // `--autoshift` / `--manual-shift` override the profile; autoshift forces shift help on.
    bike.powertrain.auto_shift = autoshift;
    bike.powertrain.shift_help |= autoshift;
    // Rider body slew rates from the rider parameters named by rider/rider.cfg `params`
    // (`FUN_140192ec0`); the RiderState defaults are the stock values if the file is missing.
    let prm_name = read_cfg(vfs, "rider/rider.cfg").and_then(|c| c.get_str("params").map(str::to_owned));
    match prm_name.map(|n| vfs.read(&format!("rider/{n}"))) {
        Some(Ok(bytes)) => match mxbikes_formats::RiderPrm::parse(bytes.as_slice()) {
            Ok(prm) => {
                bike.rider.set_params(prm);
            }
            Err(e) => warn!("rider params: {e}"),
        },
        _ => warn!("rider params not found"),
    }
    commands.insert_resource(BikeSim(bike));
    if let Some(setup) = load_setup(vfs, id, &tyre_set) {
        commands.insert_resource(setup);
    }
    commands.insert_resource(BikeParts {
        chassis,
        steer,
        fork,
        swingarm,
        front_wheel,
        rear_wheel,
        rear_axle,
    });
}

/// The garage's view of the bike: its `.cfg` and `.geom` plus the compounds of its tyre set.
fn load_setup(vfs: &MxBikesVfs, id: &str, tyre_set: &str) -> Option<BikeSetup> {
    let cfg = read_cfg(vfs, &format!("bikes/{id}/{id}.cfg"))?;
    let geom_file = cfg.get_str("geom").map(str::to_string).unwrap_or(format!("{id}.geom"));
    let geom = read_cfg(vfs, &format!("bikes/{id}/{geom_file}")).unwrap_or_default();
    let tyre_cfg = read_cfg(vfs, &format!("tyres/{tyre_set}/{tyre_set}.cfg"))?;
    let wheel = |w: usize| -> Vec<TyreChoice> {
        let Some(node) = tyre_cfg.get_child(&format!("wheel{w}")) else { return Vec::new() };
        (0..10)
            .map_while(|i| node.get_child(&format!("tyre{i}")))
            .filter_map(|t| {
                let params = read_cfg(vfs, &format!("tyres/{tyre_set}/{}", t.get_str("params")?))?;
                // Compound name: refname, else id (`FUN_14016ba60`). INFERRED: sets per compound
                // default to 1 when `numsets` is absent, as in the stock tyre files.
                let name = t.get_str("refname").or_else(|| t.get_str("id")).unwrap_or("").to_string();
                let sets = t.get_i64("numsets").unwrap_or(1).max(1) as u32;
                Some(TyreChoice { name, params, sets })
            })
            .collect()
    };
    let strings = vfs
        .read("ui/english.str")
        .ok()
        .map(|d| mxbikes_formats::StringTable::parse(&String::from_utf8_lossy(&d)))
        .unwrap_or_default();
    let click = strings.get("cc_click").unwrap_or("click").to_string();
    let clicks = strings.get("cc_clicks").unwrap_or("clicks").to_string();
    Some(BikeSetup::new(id.to_string(), cfg, geom, [wheel(0), wheel(1)], click, clicks))
}

/// Applies the garage setup when the garage closes with Done (`FUN_14009c330`): the bike is
/// rebuilt from the edited nodes at the spawn, keeping dirt, aids and rider parameters.
pub fn apply_setup(
    mut apply: ResMut<crate::ui::ApplySetup>,
    setup: Option<Res<BikeSetup>>,
    vfs: Res<Vfs>,
    options: Res<LaunchOptions>,
    spawn: Option<Res<SpawnPoint>>,
    mut sim: Option<ResMut<BikeSim>>,
    profile: Option<Res<crate::profile::Profile>>,
) {
    if !apply.0 {
        return;
    }
    apply.0 = false;
    let (Some(setup), Some(sim)) = (setup, sim.as_mut()) else { return };
    let id = options.bike.as_str();
    let base = format!("bikes/{id}/");
    let engn_file = setup
        .cfg
        .get_child("engine")
        .and_then(|e| e.get_child(&format!("mapping{}", e.get_i64("mappingsetting").unwrap_or(0))))
        .and_then(|m| m.get_str("curves"))
        .map(str::to_string)
        .unwrap_or(format!("{id}.engn"));
    let engn = read_cfg(&vfs.0, &format!("{base}{engn_file}"));
    let ini = read_cfg(&vfs.0, &format!("{base}{id}.ini"));
    let def = mxbikes_formats::BikeDefinition::from_nodes(&setup.cfg, Some(&setup.geom), engn.as_ref(), ini.as_ref());
    let tyre = |w: usize| setup.tyres[w].get(setup.compound[w]).map(|t| mxbikes_formats::TyreDefinition::from_node(&t.params));
    let (Some(front), Some(rear)) = (tyre(0), tyre(1)) else { return };
    let old = &sim.0;
    let (position, heading) = match spawn {
        Some(sp) => (sp.position, sp.heading),
        None => (old.com, 0.0),
    };
    let mut bike = Motorcycle::new(def, (front, rear), position, heading);
    bike.place_on(position, heading);
    bike.dirt_factor = old.dirt_factor;
    bike.rider_dirt_factor = old.rider_dirt_factor;
    if let Some(p) = profile.as_ref() {
        p.apply_to_bike(&mut bike);
    }
    bike.powertrain.auto_shift = old.powertrain.auto_shift;
    bike.powertrain.shift_help = old.powertrain.shift_help;
    bike.rider.lean_speed = old.rider.lean_speed;
    bike.rider.pitch_speed = old.rider.pitch_speed;
    sim.0 = bike;
    info!("setup applied");
}

/// Keeps the sky centred on the camera and fog matching the track atmosphere.
pub fn follow_sky(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut sky: Query<&mut Transform, With<Sky>>,
) {
    let Ok(cam) = camera.get_single() else { return };
    for mut t in &mut sky {
        t.translation = cam.translation();
    }
}

pub fn fog_settings(atmosphere: &Atmosphere) -> DistanceFog {
    DistanceFog {
        color: atmosphere.fog_color,
        falloff: FogFalloff::Exponential {
            density: atmosphere.fog_density,
        },
        ..default()
    }
}

/// Rebuilds the terrain chunk meshes that overlap the area the tyres deformed this frame.
pub fn update_terrain_ruts(ground: Option<Res<TrackGround>>, mut meshes: ResMut<Assets<Mesh>>, q: Query<(&Mesh3d, &TerrainChunk)>) {
    let Some(ground) = ground else { return };
    let Some((i0, j0, i1, j1)) = ground.0.take_dirty() else { return };
    let t = &ground.0.terrain;
    for (mesh3d, chunk) in &q {
        // A vertex change also moves the normals of its neighbours, hence the one-sample margin.
        let (cx0, cz0) = (chunk.cx * chunk.step, chunk.cz * chunk.step);
        let (cx1, cz1) = ((chunk.cx + chunk.nx) * chunk.step, (chunk.cz + chunk.nz) * chunk.step);
        if i1 + 1 < cx0 || i0 > cx1 + 1 || j1 + 1 < cz0 || j0 > cz1 + 1 {
            continue;
        }
        let Some(mesh) = meshes.get_mut(&mesh3d.0) else { continue };
        let mut pos = Vec::with_capacity(((chunk.nx + 1) * (chunk.nz + 1)) as usize);
        let mut nrm = Vec::with_capacity(pos.capacity());
        for j in 0..=chunk.nz {
            for i in 0..=chunk.nx {
                let (gi, gj) = ((chunk.cx + i) * chunk.step, (chunk.cz + j) * chunk.step);
                let px = t.origin.x + gi as f32 * t.spacing_x();
                let pz = -(t.origin.z + gj as f32 * t.spacing_z());
                pos.push([px, ground.0.sample_height(gi, gj), pz]);
                nrm.push(ground.0.normal_at(px, pz).to_array());
            }
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    }
}

/// Dynamically updates active LOD levels on bike and gear parts based on camera distance.
pub fn update_lod_switching(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    lod_parents: Query<(&GlobalTransform, &PartLod)>,
    mut vis_q: Query<&mut Visibility>,
) {
    let Ok(cam) = camera.get_single() else { return };
    let cam_pos = cam.translation();
    for (transform, lod) in &lod_parents {
        if lod.levels.len() <= 1 {
            continue;
        }
        // `gfx/detail_bias` moves the switch distances in (settings.rs).
        let dist = transform.translation().distance(cam_pos) / crate::settings::lod_distance_scale().max(0.1);
        let mut active_idx = 0;
        for (i, (_, switch)) in lod.levels.iter().enumerate() {
            if dist >= *switch {
                active_idx = i;
            }
        }
        for (i, (entity, _)) in lod.levels.iter().enumerate() {
            if let Ok(mut vis) = vis_q.get_mut(*entity) {
                let target = if i == active_idx {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *vis != target {
                    *vis = target;
                }
            }
        }
    }
}

/// Handles in-place session reload when a new track/bike is selected.
/// Despawns all 3D world entities and audio voices, rebuilds the track and bike, and resets telemetry.
#[allow(clippy::too_many_arguments)]
pub fn handle_session_reload(
    mut commands: Commands,
    mut events: EventReader<ReloadSession>,
    world_entities: Query<Entity, With<WorldEntity>>,
    vfs: Res<Vfs>,
    profile: Option<Res<crate::profile::Profile>>,
    mut options: ResMut<LaunchOptions>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut audio: ResMut<Assets<bevy::audio::AudioSource>>,
    mut cam_q: Query<(&mut Transform, &mut crate::camera::FollowCamera), With<Camera3d>>,
) {
    let Some(event) = events.read().last().cloned() else { return };
    info!("in-place session reload: track '{}', bike '{}'", event.track, event.bike);

    // 1. Despawn all existing 3D world entities
    for entity in &world_entities {
        commands.entity(entity).despawn_recursive();
    }

    // 2. Update options with the newly chosen session params
    options.track = event.track;
    options.bike = event.bike;
    options.paints = event.paints;

    // 3. Respawn the world (track, terrain, lighting, atmosphere, bike, rider)
    spawn_world_internal(
        &mut commands,
        &options,
        &vfs.0,
        profile.as_deref(),
        &mut meshes,
        &mut images,
        &mut std_materials,
        &mut terrain_materials,
        &mut bindposes,
    );

    // 4. Respawn bike audio voices & state
    crate::engine_audio::setup_bike_sounds_internal(
        &mut commands,
        &vfs.0,
        &options.bike,
        &mut audio,
    );

    // 5. Reset camera to chase mode if it was in showroom
    if let Ok((_t, mut cam)) = cam_q.get_single_mut() {
        if matches!(cam.mode, crate::camera::CameraMode::Showroom(_)) {
            cam.mode = crate::camera::CameraMode::Chase;
        }
    }
}

