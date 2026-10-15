//! Rider: the skinned `rider.edf` body posed from the `.anm` lean clips, with helmet, boots and
//! protections attached to the link bones named in the rider `gfx.cfg`.
//!
//! The rig is spawned in the chassis model frame: the lean clips place the pelvis about 1.04 m
//! above and 0.16 m ahead of the clip origin, which is the seated position on a bike modelled with
//! the ground at y = 0. Joint indices in the skin weights are EDF node indices, and `.anm` keys
//! are full parent-relative transforms (see `docs/FORMATS.md`).

use crate::bike_entity::BikeSim;
use crate::scene::{self, MaterialSet};
use crate::world::{hrc_level0, load_edf, load_paint, parse_hrc, PartLod};
use bevy::prelude::*;
use bevy::render::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::render::mesh::VertexAttributeValues;
use bevy::render::view::NoFrustumCulling;
use mxbikes_formats::assets::read_cfg;
use mxbikes_formats::{AnimationClip, AnimationFile, DecodedImage, EdfModel, EdfObject};
use mxbikes_vfs::MxBikesVfs;
use std::collections::HashMap;

/// Rider, gear and animation set; the stock profile's choices. `protection` is picked per player
/// profile in the game (the rider `.ini` value `default` names no folder), `full` is the set
/// with both neckbrace and armour.
const RIDER: &str = "default_mx";
const HELMET: &str = "default";
const BOOTS: &str = "default";
const PROTECTION: &str = "full";

/// Seated-on-the-ground lean clip sets by fore/aft weight (back, centre, forward): the body set
/// and the head set the game's rider update drives with the bike lean phase.
const BODY_SITTING: [&str; 3] = ["leaning sitting back", "leaning sitting", "leaning sitting forward"];
const HEAD_SITTING: [&str; 3] = ["head lean sitting back", "head lean sitting", "head lean sitting forward"];
const BODY_STANDING_GROUND: [&str; 3] = ["leaning standing back ground", "leaning standing ground", "leaning standing forward ground"];
const HEAD_STANDING_GROUND: [&str; 3] = ["head lean standing back ground", "head lean standing ground", "head lean standing forward ground"];
const BODY_SITTING_JUMP: [&str; 3] = ["leaning sitting back jump", "leaning sitting jump", "leaning sitting forward jump"];
const HEAD_SITTING_JUMP: [&str; 3] = ["head lean sitting back jump", "head lean sitting jump", "head lean sitting forward jump"];
const BODY_STANDING_AIR: [&str; 3] = ["leaning standing back", "leaning standing", "leaning standing forward"];
const HEAD_STANDING_AIR: [&str; 3] = ["head lean standing back", "head lean standing", "head lean standing forward"];
const BODY_GROUNDSIT: [&str; 3] = ["leaning standing back groundsit", "leaning standing groundsit", "leaning standing forward groundsit"];
const TORSO_SITTING: &str = "torso lean sitting";
const TORSO_STANDING: &str = "torso lean standing";
const THROTTLE: &str = "throttle";

/// Per-joint key track of one clip (rotation, translation per frame) and the per-bone weight the
/// `.anm` stores after each track; joints the clip does not animate have no track.
struct PoseClip {
    frames: usize,
    tracks: Vec<Option<Vec<(Quat, Vec3)>>>,
    mask: Vec<f32>,
}

impl PoseClip {
    fn bind(clip: &AnimationClip, object: &EdfObject) -> Self {
        let by_name: HashMap<String, usize> =
            clip.bones.iter().enumerate().map(|(i, b)| (b.name.to_ascii_lowercase(), i)).collect();
        let bone = |n: &mxbikes_formats::EdfNode| by_name.get(&n.name.to_ascii_lowercase()).map(|&b| &clip.bones[b]);
        Self {
            frames: clip.frame_count.max(1) as usize,
            tracks: object
                .nodes
                .iter()
                .map(|n| bone(n).map(|b| b.keys.iter().map(|k| (k.rotation, k.translation)).collect()))
                .collect(),
            mask: object.nodes.iter().map(|n| bone(n).map_or(0.0, |b| b.trailer)).collect(),
        }
    }

    /// Joint pose at `phase` (0..1 over the clip), interpolated between neighbouring keys.
    fn sample(&self, joint: usize, phase: f32) -> Option<(Quat, Vec3)> {
        let keys = self.tracks.get(joint)?.as_ref()?;
        let f = phase.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let (i, t) = (f.floor() as usize, f.fract());
        let (a, b) = (keys[i], keys[(i + 1).min(keys.len() - 1)]);
        Some((a.0.slerp(b.0, t), a.1.lerp(b.1, t)))
    }
}

/// One playing clip: the game's per-clip phase, weight and layer.
struct Playing<'a> {
    clip: &'a PoseClip,
    phase: f32,
    weight: f32,
    layer: i32,
}

/// The game's pose evaluator. Clips are taken highest layer first; a lower layer only gets the
/// weight the layers above left over. Within that, each clip's sample is folded into a running
/// weighted average (slerp / lerp by `w / (w + accumulated)`) with `w = weight * bone mask`. The
/// first clip that animates a joint always lands, so a joint no clip weights keeps that sample.
/// Joints no playing clip animates return `None`.
fn evaluate(playing: &mut [Playing], joints: usize) -> Vec<Option<(Quat, Vec3)>> {
    playing.sort_by(|a, b| b.layer.cmp(&a.layer));
    (0..joints)
        .map(|j| {
            let (mut acc, mut acc_w, mut remaining) = (None::<(Quat, Vec3)>, 0.0f32, 1.0f32);
            let mut layer = playing.first().map_or(0, |p| p.layer);
            for p in playing.iter() {
                if p.clip.tracks[j].is_none() {
                    continue;
                }
                if p.layer < layer {
                    remaining *= 1.0 - acc_w;
                    layer = p.layer;
                }
                let w = remaining * p.weight * p.clip.mask[j];
                if w < 1e-5 && acc.is_some() {
                    continue;
                }
                let Some(s) = p.clip.sample(j, p.phase) else { continue };
                acc = Some(match acc {
                    Some(a) if acc_w >= 1e-5 => {
                        let t = w / (w + acc_w);
                        (a.0.slerp(s.0, t), a.1.lerp(s.1, t))
                    }
                    _ => s,
                });
                acc_w += w;
            }
            acc
        })
        .collect()
}

/// A `lefthand` / `righthand` block of the rider `gfx.cfg`: the palm point (`pos` in the `refobj`
/// frame) is placed on the bike grip by turning `root` (shoulder) and `endeffector` (elbow).
struct HandIk {
    shoulder: usize,
    elbow: usize,
    wrist: usize,
    palm: Vec3,
    /// Grip position in the steer model frame (bike `gfx.cfg` `steer/<side>grip/pos`).
    grip: Vec3,
}

#[derive(Resource)]
pub struct RiderRig {
    joints: Vec<Entity>,
    parents: Vec<Option<usize>>,
    rest: Vec<(Quat, Vec3)>,
    body_sitting: [PoseClip; 3],
    head_sitting: [PoseClip; 3],
    body_standing_ground: [PoseClip; 3],
    head_standing_ground: [PoseClip; 3],
    body_sitting_jump: [PoseClip; 3],
    head_sitting_jump: [PoseClip; 3],
    body_standing_air: [PoseClip; 3],
    head_standing_air: [PoseClip; 3],
    body_groundsit: [PoseClip; 3],
    torso_sitting: PoseClip,
    torso_standing: PoseClip,
    throttle: PoseClip,
    hands: Vec<HandIk>,
    /// `maxlean` of the animation set (degrees): the bike lean at the ends of the lean clips.
    max_lean: f32,
}

/// Lean clip phase (0..1) for a bike lean angle in degrees, as the game computes it: angles past
/// +-90 fold back, positive (right) lean runs towards frame 0, where the rider's body sits left of
/// the bike, so the rider stays more upright than the bike.
fn lean_phase(lean_deg: f32, max_lean: f32) -> f32 {
    let l = if lean_deg > 90.0 {
        180.0 - lean_deg
    } else if lean_deg < -90.0 {
        -180.0 - lean_deg
    } else {
        lean_deg
    };
    (1.0 - (l / max_lean.max(1.0)).clamp(-1.0, 1.0)) * 0.5
}

fn chain_world(locals: &[(Quat, Vec3)], parents: &[Option<usize>], root: Mat4, joint: usize) -> Mat4 {
    let mut m = Mat4::from_rotation_translation(locals[joint].0, locals[joint].1);
    let mut p = parents[joint];
    while let Some(j) = p {
        m = Mat4::from_rotation_translation(locals[j].0, locals[j].1) * m;
        p = parents[j];
    }
    root * m
}

/// Applies a world-space rotation `delta` to `joint`, rewriting its local rotation.
fn rotate_world(locals: &mut [(Quat, Vec3)], parents: &[Option<usize>], root: Mat4, joint: usize, delta: Quat) {
    let (_, world, _) = chain_world(locals, parents, root, joint).to_scale_rotation_translation();
    let parent = match parents[joint] {
        Some(p) => chain_world(locals, parents, root, p).to_scale_rotation_translation().1,
        None => root.to_scale_rotation_translation().1,
    };
    locals[joint].0 = (parent.inverse() * delta * world).normalize();
}

/// Two-bone solve: bend the elbow to the shoulder-target distance, then swing the shoulder onto
/// the target. The wrist keeps its animated local rotation.
fn solve_hand(locals: &mut [(Quat, Vec3)], parents: &[Option<usize>], root: Mat4, h: &HandIk, target: Vec3) {
    let pos = |l: &[(Quat, Vec3)], j: usize| chain_world(l, parents, root, j).w_axis.truncate();
    let (s, e, w) = (pos(locals, h.shoulder), pos(locals, h.elbow), pos(locals, h.wrist));
    let palm = chain_world(locals, parents, root, h.wrist).transform_point3(h.palm);
    let t = target - (palm - w);
    let (a, b) = ((e - s).length(), (w - e).length());
    if a < 1e-4 || b < 1e-4 {
        return;
    }
    let c = (t - s).length().clamp((a - b).abs() + 1e-3, a + b - 1e-3);
    let Some(n) = (s - e).cross(w - e).try_normalize() else { return };
    let bend = ((a * a + b * b - c * c) / (2.0 * a * b)).clamp(-1.0, 1.0).acos();
    let new_dir = Quat::from_axis_angle(n, bend) * (s - e).normalize();
    rotate_world(locals, parents, root, h.elbow, Quat::from_rotation_arc((w - e).normalize(), new_dir));
    let w2 = pos(locals, h.wrist);
    if let (Some(from), Some(to)) = ((w2 - s).try_normalize(), (t - s).try_normalize()) {
        rotate_world(locals, parents, root, h.shoulder, Quat::from_rotation_arc(from, to));
    }
}

fn load_clips(vfs: &MxBikesVfs, set: &str) -> HashMap<String, AnimationClip> {
    let dir = format!("rider/animations/{set}");
    let mut clips = HashMap::new();
    let Some(cfg) = read_cfg(vfs, &format!("{dir}/gfx.cfg")) else {
        warn!("rider animation set '{set}' not found");
        return clips;
    };
    // `anim`, `anim2`, `anim3`, ... until the first missing key, as the game reads them.
    for i in 1.. {
        let key = if i == 1 { "anim".to_string() } else { format!("anim{i}") };
        let Some(file) = cfg.get_str(&key) else { break };
        match vfs.read(&format!("{dir}/{file}")).map_err(|e| e.to_string()).and_then(|d| AnimationFile::parse(&d).map_err(|e| e.to_string())) {
            Ok(a) => {
                for c in a.clips {
                    clips.insert(c.name.to_ascii_lowercase(), c);
                }
            }
            Err(e) => warn!("{dir}/{file}: {e}"),
        }
    }
    clips
}

/// Spawns the meshes of a bone-local gear model with full `.hrc` LOD levels under `joint`.
#[allow(clippy::too_many_arguments)]
fn attach(
    commands: &mut Commands,
    vfs: &MxBikesVfs,
    folder: &str,
    hrc: &str,
    paint: &HashMap<String, DecodedImage>,
    mud: Option<scene::MudConfig>,
    joint: Entity,
    name: &str,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
) {
    let levels = parse_hrc(vfs, folder, hrc);
    if levels.is_empty() {
        warn!("{folder}/{hrc}: no LOD levels");
        return;
    }
    let parent = commands
        .spawn((Transform::default(), Visibility::default(), Name::new(name.to_string())))
        .id();
    let mut lod_entities = Vec::new();

    for (idx, lvl) in levels.into_iter().enumerate() {
        let path = format!("{folder}/{}", lvl.scene);
        if let Some((data, model)) = load_edf(vfs, &path) {
            let set = scene::build_materials_with_mud(&data, &model.materials, images, std_materials, paint, mud);
            let initial_vis = if idx == 0 { Visibility::Inherited } else { Visibility::Hidden };
            let child = scene::spawn_group(commands, &model, &lvl.name, &set, meshes, &format!("{name}_lod{idx}"));
            commands.entity(child).insert(initial_vis);
            commands.entity(parent).add_child(child);
            lod_entities.push((child, lvl.switch_dist));
        }
    }

    if lod_entities.len() > 1 {
        commands.entity(parent).insert(PartLod { levels: lod_entities });
    }
    commands.entity(joint).add_child(parent);
}

fn skinned_meshes(object: &EdfObject, node: usize) -> Vec<(u32, Mesh)> {
    let n = &object.nodes[node];
    let Some(skin) = n.skinning.as_ref() else { return Vec::new() };
    let base = n.submeshes.iter().map(|s| s.first_vertex).min().unwrap_or(0) as usize;
    let mut out = Vec::new();
    for s in &n.submeshes {
        let t0 = s.first_triangle as usize * 3;
        let t1 = (t0 + s.triangle_count as usize * 3).min(object.indices.len());
        let tris = &object.indices[t0..t1];
        let (Some(&lo), Some(&hi)) = (tris.iter().min(), tris.iter().max()) else { continue };
        let (lo, hi) = (lo as usize, hi as usize);
        if hi - base >= skin.len() {
            warn!("node '{}': skin covers {} vertices, submesh reaches {}", n.name, skin.len(), hi - base);
            continue;
        }
        let mut mesh = scene::mesh_from(&object.vertices[lo..=hi], tris.iter().map(|i| i - lo as u32).collect());
        let range = &skin[lo - base..=hi - base];
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(range.iter().map(|v| v.bones).collect()),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, range.iter().map(|v| v.weights).collect::<Vec<_>>());
        let slot = object.materials.get(s.material as usize).map(|m| m.texture).unwrap_or(0);
        out.push((slot, mesh));
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_rider(
    commands: &mut Commands,
    vfs: &MxBikesVfs,
    parent: Entity,
    options: &crate::world::LaunchOptions,
    dirt_color: Vec3,
    grips: [Option<Vec3>; 2],
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<RiderRig> {
    let folder = format!("rider/riders/{RIDER}");
    let gfx = read_cfg(vfs, &format!("{folder}/gfx.cfg"))?;
    let model_file = gfx.get_child("model").and_then(|m| m.get_str("file")).unwrap_or("rider.hrc");
    let (file, group) = hrc_level0(vfs, &folder, model_file)?;
    let (data, model): (_, EdfModel) = load_edf(vfs, &format!("{folder}/{file}"))?;
    let object = model.group(&group)?.objects.first()?;
    // Paint selections resolve the way the game builds its paint paths; suit and gloves both
    // repaint the rider model, helmet paint and goggles the helmet.
    let paint = |pairs: &[(&str, String)]| {
        let mut out = HashMap::new();
        for (part, dir) in pairs {
            if let Some(name) = options.paints.get(*part) {
                out.extend(load_paint(vfs, &format!("{dir}/{name}.pnt")));
            }
        }
        out
    };
    let helmet_dir = format!("rider/helmets/{HELMET}");
    let boots_dir = format!("rider/boots/{BOOTS}");
    let mut suit = paint(&[("rider", format!("{folder}/paints")), ("gloves", format!("{folder}/gloves"))]);
    let plates = crate::plate::render_plates(vfs, &options.font, &options.number, &options.rider_name);
    suit.extend(plates);
    let helmet_paint = paint(&[("helmet", format!("{helmet_dir}/paints")), ("goggles", format!("{helmet_dir}/goggles"))]);
    let boots_paint = paint(&[("boots", format!("{boots_dir}/paints"))]);

    let dirt_factor = options.dirt.unwrap_or(0.0);
    let suit_dirt_tint = if gfx.get_i64("dirt_color").unwrap_or(0) != 0 { dirt_color } else { Vec3::ONE };
    let suit_mud = Some(scene::MudConfig { level: mxbikes_physics::motorcycle::dirt_level(dirt_factor), color: suit_dirt_tint });
    let materials: MaterialSet = scene::build_materials_with_mud(&data, &model.materials, images, std_materials, &suit, suit_mud);

    let root = commands.spawn((Transform::IDENTITY, Visibility::default(), Name::new("rider"))).id();
    commands.entity(parent).add_child(root);

    let joints: Vec<Entity> = object
        .nodes
        .iter()
        .map(|n| commands.spawn((Transform::from_matrix(n.transform), Visibility::default(), Name::new(n.name.clone()))).id())
        .collect();
    for (i, n) in object.nodes.iter().enumerate() {
        let p = n.parent.and_then(|p| joints.get(p).copied()).unwrap_or(root);
        commands.entity(p).add_child(joints[i]);
    }

    let rest: Vec<Mat4> = (0..object.nodes.len()).map(|i| object.node_world(i)).collect();
    for (ni, n) in object.nodes.iter().enumerate() {
        if n.submeshes.is_empty() {
            continue;
        }
        if n.skinning.is_none() {
            scene::spawn_object_filtered(commands, joints[ni], object, &materials, meshes, &|name: &str| name == n.name);
            continue;
        }
        let inverse = bindposes.add(SkinnedMeshInverseBindposes::from(
            rest.iter().map(|j| j.inverse() * rest[ni]).collect::<Vec<_>>(),
        ));
        for (slot, mesh) in skinned_meshes(object, ni) {
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.get(slot)),
                    SkinnedMesh {
                        inverse_bindposes: inverse.clone(),
                        joints: joints.clone(),
                    },
                    NoFrustumCulling,
                    Transform::IDENTITY,
                    Name::new(n.name.clone()),
                ))
                .id();
            commands.entity(root).add_child(e);
        }
    }

    let joint = |key: &str| gfx.get_str(key).and_then(|name| object.find_node(name)).map(|i| joints[i]);
    let helmet_cfg = read_cfg(vfs, &format!("{helmet_dir}/gfx.cfg"));
    let helmet_tint = if helmet_cfg.as_ref().and_then(|h| h.get_i64("dirt_color")).unwrap_or(0) != 0 { dirt_color } else { Vec3::ONE };
    let helmet_mud = Some(scene::MudConfig { level: mxbikes_physics::motorcycle::dirt_level(dirt_factor), color: helmet_tint });
    if let (Some(j), Some(h)) = (joint("helmetlinkobj"), helmet_cfg) {
        let hrc = h.get_str("model").unwrap_or("helmet.hrc").to_string();
        attach(commands, vfs, &helmet_dir, &hrc, &helmet_paint, helmet_mud, j, "helmet", meshes, images, std_materials);
    }

    let boots_cfg = read_cfg(vfs, &format!("{boots_dir}/gfx.cfg"));
    let boots_tint = if boots_cfg.as_ref().and_then(|b| b.get_i64("dirt_color")).unwrap_or(0) != 0 { dirt_color } else { Vec3::ONE };
    let boots_mud = Some(scene::MudConfig { level: mxbikes_physics::motorcycle::dirt_level(dirt_factor), color: boots_tint });
    if let Some(b) = boots_cfg {
        for side in ["left", "right"] {
            let hrc = b.get_child(side).and_then(|s| s.get_child("model")).and_then(|m| m.get_str("file"));
            if let (Some(j), Some(hrc)) = (joint(&format!("{side}bootlinkobj")), hrc) {
                attach(commands, vfs, &boots_dir, hrc, &boots_paint, boots_mud, j, &format!("{side}_boot"), meshes, images, std_materials);
            }
        }
    }
    let prot_dir = format!("rider/protections/{PROTECTION}");
    let prot_cfg = read_cfg(vfs, &format!("{prot_dir}/gfx.cfg"));
    let prot_tint = if prot_cfg.as_ref().and_then(|p| p.get_i64("dirt_color")).unwrap_or(0) != 0 { dirt_color } else { Vec3::ONE };
    let prot_mud = Some(scene::MudConfig { level: mxbikes_physics::motorcycle::dirt_level(dirt_factor), color: prot_tint });
    if let Some(p) = prot_cfg {
        for part in ["neckbrace", "armour"] {
            let hrc = p.get_child(part).and_then(|c| c.get_str("model"));
            if let (Some(j), Some(hrc)) = (joint(&format!("{part}linkobj")), hrc) {
                attach(commands, vfs, &prot_dir, hrc, &HashMap::new(), prot_mud, j, part, meshes, images, std_materials);
            }
        }
    }

    let set = gfx.get_str("animations").unwrap_or("mx");
    let clips = load_clips(vfs, set);
    let anim_cfg = read_cfg(vfs, &format!("rider/animations/{set}/gfx.cfg"));
    let anim_cfg = anim_cfg.as_ref();
    let pose = |name: &str| match clips.get(name) {
        Some(c) => PoseClip::bind(c, object),
        None => {
            warn!("rider clip '{name}' not found in set '{set}'");
            PoseClip {
                frames: 1,
                tracks: vec![None; object.nodes.len()],
                mask: vec![0.0; object.nodes.len()],
            }
        }
    };
    let hands = ["lefthand", "righthand"]
        .iter()
        .zip(grips)
        .filter_map(|(side, grip)| {
            let h = gfx.get_child(side)?;
            let p = h.get_child("pos");
            let palm = Vec3::new(
                p.and_then(|p| p.get_f32("x")).unwrap_or(0.0),
                p.and_then(|p| p.get_f32("y")).unwrap_or(0.0),
                -p.and_then(|p| p.get_f32("z")).unwrap_or(0.0),
            );
            let find = |k: &str| h.get_str(k).and_then(|n| object.find_node(n));
            Some(HandIk {
                shoulder: find("root")?,
                elbow: find("endeffector")?,
                wrist: find("refobj")?,
                palm,
                grip: grip?,
            })
        })
        .collect();
    Some(RiderRig {
        joints,
        parents: object.nodes.iter().map(|n| n.parent).collect(),
        rest: object
            .nodes
            .iter()
            .map(|n| {
                let (_, r, t) = n.transform.to_scale_rotation_translation();
                (r, t)
            })
            .collect(),
        body_sitting: BODY_SITTING.map(pose),
        head_sitting: HEAD_SITTING.map(pose),
        body_standing_ground: BODY_STANDING_GROUND.map(pose),
        head_standing_ground: HEAD_STANDING_GROUND.map(pose),
        body_sitting_jump: BODY_SITTING_JUMP.map(pose),
        head_sitting_jump: HEAD_SITTING_JUMP.map(pose),
        body_standing_air: BODY_STANDING_AIR.map(pose),
        head_standing_air: HEAD_STANDING_AIR.map(pose),
        body_groundsit: BODY_GROUNDSIT.map(pose),
        torso_sitting: pose(TORSO_SITTING),
        torso_standing: pose(TORSO_STANDING),
        throttle: pose(THROTTLE),
        hands,
        max_lean: anim_cfg.and_then(|c| c.get_f32("maxlean")).unwrap_or(55.0),
    })
}

/// Poses the rig as the game's rider update does (`FUN_1400523e0`), from the rider states the
/// physics keeps (`RiderState`). With s = standing, j = airborne and c = commanded stand, the lean
/// sets weigh: sitting (1-s)(1-j)(1-c), standing groundsit (1-s)(1-j)c, sitting jump (1-s)j,
/// standing ground s(1-j), standing s j; the head sets the same without the groundsit split.
/// Within a set, fore/aft blends the centre clip with the back or forward one. All lean sets play
/// at the bike lean phase.
pub fn animate_rider(sim: Res<BikeSim>, rig: Option<Res<RiderRig>>, mut q: Query<&mut Transform>) {
    let Some(rig) = rig else { return };
    let r = &sim.0.rider;
    let (s, j, c) = (r.stand, r.airborne, r.stand_cmd);
    let f = r.fore_aft.clamp(-1.0, 1.0);
    let side = if f < 0.0 { 0 } else { 2 };
    let phase = lean_phase(sim.0.lean_angle_deg, rig.max_lean);

    let sets: [(&[PoseClip; 3], f32); 9] = [
        (&rig.body_sitting, (1.0 - s) * (1.0 - j) * (1.0 - c)),
        (&rig.body_groundsit, (1.0 - s) * (1.0 - j) * c),
        (&rig.body_sitting_jump, (1.0 - s) * j),
        (&rig.body_standing_ground, s * (1.0 - j)),
        (&rig.body_standing_air, s * j),
        (&rig.head_sitting, (1.0 - s) * (1.0 - j)),
        (&rig.head_sitting_jump, (1.0 - s) * j),
        (&rig.head_standing_ground, s * (1.0 - j)),
        (&rig.head_standing_air, s * j),
    ];
    let mut playing = Vec::with_capacity(24);
    for (set, w) in sets {
        playing.push(Playing { clip: &set[1], phase, weight: w * (1.0 - f.abs()), layer: 0 });
        playing.push(Playing { clip: &set[side], phase, weight: w * f.abs(), layer: 0 });
    }

    // Torso lean on layer 2 (FUN_14003f070 sets the layer): phase is the signed curve
    // sign(t) |t|^0.75, which the engine clamps to 0..1, so a left lean plays frame 0 (full left)
    // at its weight, exactly as the original does.
    let t = r.torso;
    let curve = t.signum() * t.abs().powf(0.75);
    playing.push(Playing { clip: &rig.torso_sitting, phase: curve, weight: (1.0 - s) * curve.abs(), layer: 2 });
    playing.push(Playing { clip: &rig.torso_standing, phase: curve, weight: s * curve.abs(), layer: 2 });

    // Throttle (Layer 0)
    playing.push(Playing {
        clip: &rig.throttle,
        phase: sim.0.powertrain.throttle,
        weight: 1.0,
        layer: 0,
    });
    let mut locals: Vec<(Quat, Vec3)> = evaluate(&mut playing, rig.joints.len())
        .into_iter()
        .zip(&rig.rest)
        .map(|(p, r)| p.unwrap_or(*r))
        .collect();

    let (cp, cr) = sim.0.chassis_transform();
    let (sp, sr) = sim.0.steer_transform();
    let root = Mat4::from_rotation_translation(cr, cp);
    let steer = Mat4::from_rotation_translation(sr, sp);
    for h in &rig.hands {
        let target = steer.transform_point3(h.grip);
        // The palm offset moves with the wrist, so repeated passes converge on the grip (the
        // animated pose starts about 1 cm away; at full lock the far arm runs out of reach).
        for _ in 0..4 {
            solve_hand(&mut locals, &rig.parents, root, h, target);
        }
    }

    for (&e, (r, t)) in rig.joints.iter().zip(locals) {
        if let Ok(mut tf) = q.get_mut(e) {
            tf.rotation = r;
            tf.translation = t;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(angle: f32, mask: [f32; 2]) -> PoseClip {
        let key = (Quat::from_rotation_z(angle), Vec3::ZERO);
        PoseClip {
            frames: 1,
            tracks: vec![Some(vec![key]), Some(vec![key])],
            mask: mask.to_vec(),
        }
    }

    #[test]
    fn evaluator_follows_masks_weights_and_layers() {
        let (a, b, top) = (clip(0.0, [1.0, 1.0]), clip(1.0, [1.0, 0.0]), clip(-1.0, [0.0, 1.0]));
        let angle = |p: Option<(Quat, Vec3)>| p.unwrap().0.to_axis_angle().1 * p.unwrap().0.to_axis_angle().0.z;
        let mut playing = vec![
            Playing { clip: &a, phase: 0.0, weight: 0.75, layer: 0 },
            Playing { clip: &b, phase: 0.0, weight: 0.25, layer: 0 },
        ];
        let out = evaluate(&mut playing, 2);
        // Joint 0: weighted blend 0.75 * 0 + 0.25 * 1. Joint 1: b is masked out.
        assert!((angle(out[0]) - 0.25).abs() < 1e-4);
        assert!(angle(out[1]).abs() < 1e-4);
        // A full-weight higher layer leaves nothing for layer 0 on the joints it masks in.
        let mut playing = vec![
            Playing { clip: &a, phase: 0.0, weight: 1.0, layer: 0 },
            Playing { clip: &top, phase: 0.0, weight: 1.0, layer: 2 },
        ];
        let out = evaluate(&mut playing, 2);
        assert!(angle(out[0]).abs() < 1e-4, "top layer masks joint 0 out");
        assert!((angle(out[1]) + 1.0).abs() < 1e-4);
    }

    #[test]
    fn lean_phase_matches_game_mapping() {
        assert_eq!(lean_phase(0.0, 55.0), 0.5);
        assert_eq!(lean_phase(55.0, 55.0), 0.0);
        assert_eq!(lean_phase(-80.0, 55.0), 1.0);
        // Past 90 degrees the angle folds back: 150 behaves like 30.
        assert!((lean_phase(150.0, 55.0) - lean_phase(30.0, 55.0)).abs() < 1e-6);
    }

    #[test]
    fn two_bone_solve_reaches_target() {
        // Shoulder at the origin, upper arm and forearm 0.3 m each along +x, slightly bent.
        let parents = vec![None, Some(0), Some(1)];
        let mut locals = vec![
            (Quat::IDENTITY, Vec3::ZERO),
            (Quat::from_rotation_z(0.2), Vec3::new(0.3, 0.0, 0.0)),
            (Quat::IDENTITY, Vec3::new(0.3, 0.0, 0.0)),
        ];
        let hand = HandIk {
            shoulder: 0,
            elbow: 1,
            wrist: 2,
            palm: Vec3::new(0.05, 0.0, 0.0),
            grip: Vec3::ZERO,
        };
        let target = Vec3::new(0.2, -0.3, 0.2);
        for _ in 0..4 {
            solve_hand(&mut locals, &parents, Mat4::IDENTITY, &hand, target);
        }
        let palm = chain_world(&locals, &parents, Mat4::IDENTITY, 2).transform_point3(hand.palm);
        assert!(palm.distance(target) < 1e-3, "palm {palm:?}");
        // Bone lengths are preserved.
        assert!((locals[1].1.length() - 0.3).abs() < 1e-6 && (locals[2].1.length() - 0.3).abs() < 1e-6);
    }
}
