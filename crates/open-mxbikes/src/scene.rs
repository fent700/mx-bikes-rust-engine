//! Converts decoded PiBoSo assets into Bevy meshes, images and materials.

use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use mxbikes_formats::edf::{EdfObject, EdfVertex, Submesh};
use mxbikes_formats::texture::{DecodedImage, ImageRecord, Material as PbMaterial};
use mxbikes_formats::{EdfModel, MorphSet, TrackTerrain};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Builds a full mip chain (box filter) and returns the concatenated level data and level count.
fn build_mips(width: u32, height: u32, rgba: &[u8], channels: usize) -> (Vec<u8>, u32) {
    let mut data = rgba.to_vec();
    let mut levels = 1;
    let (mut w, mut h) = (width as usize, height as usize);
    let mut prev = rgba.to_vec();
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * channels];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..channels {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += prev[(sy * w + sx) * channels + c] as u32;
                    }
                    next[(y * nw + x) * channels + c] = (sum / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        prev = next;
        w = nw;
        h = nh;
        levels += 1;
    }
    (data, levels)
}

pub fn sampler(repeat: bool) -> ImageSampler {
    let mode = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    })
}

/// Uploads a decoded RGBA image with mipmaps.
pub fn rgba_image(img: &DecodedImage, srgb: bool, repeat: bool) -> Image {
    let layer = (img.width * img.height * 4) as usize;
    let (data, levels) = build_mips(img.width, img.height, &img.rgba[..layer], 4);
    let format = if srgb {
        TextureFormat::Rgba8UnormSrgb
    } else {
        TextureFormat::Rgba8Unorm
    };
    let mut image = Image {
        data,
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        ..Default::default()
    };
    image.texture_descriptor.dimension = TextureDimension::D2;
    image.texture_descriptor.size = Extent3d {
        width: img.width,
        height: img.height,
        depth_or_array_layers: 1,
    };
    image.texture_descriptor.format = format;
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = sampler(repeat);
    image
}

/// Uploads a single-channel coverage mask (red channel of the decoded image) with mipmaps.
pub fn mask_image(img: &DecodedImage) -> Image {
    let r: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
    let (data, levels) = build_mips(img.width, img.height, &r, 1);
    let mut image = Image {
        data,
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        ..Default::default()
    };
    image.texture_descriptor.dimension = TextureDimension::D2;
    image.texture_descriptor.size = Extent3d {
        width: img.width,
        height: img.height,
        depth_or_array_layers: 1,
    };
    image.texture_descriptor.format = TextureFormat::R8Unorm;
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = sampler(false);
    image
}

/// Decodes many image records in parallel; failures are logged and skipped.
pub fn decode_parallel(file: &[u8], records: &[&ImageRecord]) -> Vec<Option<DecodedImage>> {
    std::thread::scope(|s| {
        let handles: Vec<_> = records
            .iter()
            .map(|r| s.spawn(move || r.decode_frame(file, 0)))
            .collect();
        handles
            .into_iter()
            .zip(records)
            .map(|(h, r)| match h.join() {
                Ok(Ok(img)) => Some(img),
                Ok(Err(e)) => {
                    warn!("texture '{}' not decoded: {e}", r.name);
                    None
                }
                Err(_) => None,
            })
            .collect()
    })
}

/// Materials for one asset file, indexed by the 1-based texture reference used in object slots.
pub struct MaterialSet {
    pub by_ref: HashMap<u32, Handle<StandardMaterial>>,
    pub untextured: Handle<StandardMaterial>,
}

impl MaterialSet {
    pub fn get(&self, texture_ref: u32) -> Handle<StandardMaterial> {
        self.by_ref.get(&texture_ref).cloned().unwrap_or_else(|| self.untextured.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MudConfig {
    /// Mud stage 0..3 (`Motorcycle::dirt_level`); 0 is clean.
    pub level: u32,
    /// Track `dirt_color` for parts flagged `dirt_color` in their gfx.cfg, else white.
    pub color: Vec3,
}

/// Draws a mud stage (RGBA with alpha) over a base diffuse image, tinted by `tint`.
pub fn blend_mud(base: &mut DecodedImage, mud: &DecodedImage, factor: f32, tint: Vec3) {
    if factor <= 0.0 || mud.rgba.is_empty() {
        return;
    }
    let (bw, bh) = (base.width as usize, base.height as usize);
    let (mw, mh) = (mud.width as usize, mud.height as usize);
    if bw == 0 || bh == 0 || mw == 0 || mh == 0 {
        return;
    }
    for y in 0..bh {
        let my = (y * mh / bh).min(mh - 1);
        let base_row = y * bw * 4;
        let mud_row = my * mw * 4;
        for x in 0..bw {
            let mx = (x * mw / bw).min(mw - 1);
            let b_idx = base_row + x * 4;
            let m_idx = mud_row + mx * 4;

            if b_idx + 3 >= base.rgba.len() || m_idx + 3 >= mud.rgba.len() {
                continue;
            }

            let ma = mud.rgba[m_idx + 3] as f32 / 255.0;
            if ma <= 0.0 {
                continue;
            }
            let alpha = (ma * factor).clamp(0.0, 1.0);
            if alpha <= 0.001 {
                continue;
            }

            let mr = (mud.rgba[m_idx] as f32 / 255.0 * tint.x).clamp(0.0, 1.0);
            let mg = (mud.rgba[m_idx + 1] as f32 / 255.0 * tint.y).clamp(0.0, 1.0);
            let mb = (mud.rgba[m_idx + 2] as f32 / 255.0 * tint.z).clamp(0.0, 1.0);

            let br = base.rgba[b_idx] as f32 / 255.0;
            let bg = base.rgba[b_idx + 1] as f32 / 255.0;
            let bb = base.rgba[b_idx + 2] as f32 / 255.0;

            base.rgba[b_idx] = ((br * (1.0 - alpha) + mr * alpha) * 255.0).round() as u8;
            base.rgba[b_idx + 1] = ((bg * (1.0 - alpha) + mg * alpha) * 255.0).round() as u8;
            base.rgba[b_idx + 2] = ((bb * (1.0 - alpha) + mb * alpha) * 255.0).round() as u8;
        }
    }
}

pub fn build_materials(
    file: &[u8],
    materials: &[PbMaterial],
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    override_diffuse: &HashMap<String, DecodedImage>,
) -> MaterialSet {
    build(file, materials, images, std_materials, override_diffuse, None)
}

/// A material whose diffuse shows the mud stage; recomposited when the stage changes.
struct MudTarget {
    source: Arc<Vec<u8>>,
    layer: ImageRecord,
    clean: DecodedImage,
    color: Vec3,
    image: Handle<Image>,
}

static MUD_TARGETS: Mutex<Vec<MudTarget>> = Mutex::new(Vec::new());

/// Mud stage `level` composited over `clean`: stage n is stored frame n - 1 of the paint layer
/// (the empty base frame is dropped by the reader).
fn with_mud(source: &[u8], layer: &ImageRecord, clean: &DecodedImage, level: u32, color: Vec3) -> DecodedImage {
    let mut img = clean.clone();
    let frames = layer.frames.len() as u32;
    if level > 0 && frames > 0 {
        if let Ok(mud) = layer.decode_frame(source, (level.min(frames) - 1) as usize) {
            blend_mud(&mut img, &mud, 1.0, color);
        }
    }
    img
}

/// Re-composites every registered mud material when the bike's mud stage changes.
pub fn update_mud(sim: Res<crate::bike_entity::BikeSim>, mut images: ResMut<Assets<Image>>, mut shown: Local<Option<u32>>) {
    let level = sim.0.dirt_level();
    if *shown == Some(level) {
        return;
    }
    let first = shown.is_none();
    *shown = Some(level);
    if first {
        return; // Built with the starting stage already.
    }
    let targets = MUD_TARGETS.lock().unwrap();
    for t in targets.iter() {
        let img = with_mud(&t.source, &t.layer, &t.clean, level, t.color);
        images.insert(&t.image, rgba_image(&img, true, true));
    }
}

/// Materials with mud: the stage starts at `mud.level` and follows the bike's dirt afterwards.
pub fn build_materials_with_mud(
    file: &Arc<Vec<u8>>,
    materials: &[PbMaterial],
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    override_diffuse: &HashMap<String, DecodedImage>,
    mud: Option<MudConfig>,
) -> MaterialSet {
    build(file, materials, images, std_materials, override_diffuse, mud.map(|m| (m, file.clone())))
}

/// Converts a single-channel specular / gloss map into a Bevy PBR metallic-roughness image (green = roughness, blue = metallic).
fn metallic_roughness_image(spec: &DecodedImage, base_roughness: f32, is_metallic: bool) -> DecodedImage {
    let mut rgba = Vec::with_capacity((spec.width * spec.height * 4) as usize);
    let metal_byte = if is_metallic { 180u8 } else { 0u8 };
    for chunk in spec.rgba.chunks_exact(4) {
        let factor = chunk[0] as f32 / 255.0;
        let r = (base_roughness * (1.1 - 0.75 * factor)).clamp(0.08, 1.0);
        let roughness_byte = (r * 255.0).round() as u8;
        rgba.extend_from_slice(&[255, roughness_byte, metal_byte, 255]);
    }
    DecodedImage {
        width: spec.width,
        height: spec.height,
        layers: 1,
        rgba,
    }
}

fn build(
    file: &[u8],
    materials: &[PbMaterial],
    images: &mut Assets<Image>,
    std_materials: &mut Assets<StandardMaterial>,
    override_diffuse: &HashMap<String, DecodedImage>,
    mud: Option<(MudConfig, Arc<Vec<u8>>)>,
) -> MaterialSet {
    let records: Vec<&ImageRecord> = materials.iter().map(|m| &m.diffuse).collect();
    let decoded = decode_parallel(file, &records);
    let normals: Vec<&ImageRecord> = materials.iter().filter_map(|m| m.normal.as_ref().map(|n| &n.map)).collect();
    let decoded_normals = decode_parallel(file, &normals);
    let mut normal_iter = decoded_normals.into_iter();
    let specs: Vec<&ImageRecord> = materials.iter().filter_map(|m| m.specular.as_ref().and_then(|s| s.map.as_ref())).collect();
    let decoded_specs = decode_parallel(file, &specs);
    let mut spec_iter = decoded_specs.into_iter();

    let mut by_ref = HashMap::new();
    for (i, (m, img)) in materials.iter().zip(decoded).enumerate() {
        let mut diffuse = override_diffuse.get(&m.diffuse.name.to_ascii_lowercase()).cloned().or(img);
        let mut mud_target = None;
        if let (Some((cfg, source)), Some(layer), Some(clean)) = (&mud, m.paint_layers.first(), diffuse.as_ref()) {
            mud_target = Some((source.clone(), layer.material.diffuse.clone(), clean.clone(), cfg.color));
            diffuse = Some(with_mud(file, &layer.material.diffuse, clean, cfg.level, cfg.color));
        }

        let normal = if m.normal.is_some() { normal_iter.next().flatten() } else { None };
        let spec_map = if m.specular.as_ref().is_some_and(|s| s.map.is_some()) {
            spec_iter.next().flatten()
        } else {
            None
        };
        let alpha = m.diffuse.has_alpha_test();
        let roughness = match m.specular.as_ref() {
            Some(s) => (1.0 - (s.power / 60.0)).clamp(0.35, 0.9),
            None => 0.85,
        };
        let base_color_texture = diffuse.map(|d| images.add(rgba_image(&d, true, true)));
        let metallic_roughness_texture = spec_map.map(|sm| {
            let mr = metallic_roughness_image(&sm, roughness, m.reflection.is_some());
            images.add(rgba_image(&mr, false, true))
        });
        if let (Some((source, layer, clean, color)), Some(image)) = (mud_target, base_color_texture.clone()) {
            MUD_TARGETS.lock().unwrap().push(MudTarget { source, layer, clean, color, image });
        }
        let mat = StandardMaterial {
            base_color_texture,
            normal_map_texture: normal.map(|n| images.add(rgba_image(&n, false, true))),
            metallic_roughness_texture,
            perceptual_roughness: roughness,
            reflectance: if m.reflection.is_some() { 0.5 } else { 0.3 },
            alpha_mode: if alpha { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
            double_sided: alpha,
            cull_mode: if alpha { None } else { Some(bevy::render::render_resource::Face::Back) },
            ..default()
        };
        by_ref.insert(i as u32 + 1, std_materials.add(mat));
    }
    MaterialSet {
        by_ref,
        untextured: std_materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.6),
            perceptual_roughness: 0.8,
            ..default()
        }),
    }
}

pub fn mesh_from(vertices: &[EdfVertex], indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vertices.iter().map(|v| v.position.to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vertices
            .iter()
            .map(|v| v.normal.try_normalize().unwrap_or(Vec3::Y).to_array())
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vertices.iter().map(|v| v.uv0.to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_TANGENT,
        vertices
            .iter()
            .map(|v| {
                let t = v.tangent.truncate().try_normalize().unwrap_or(Vec3::X);
                [t.x, t.y, t.z, if v.tangent.w < 0.0 { -1.0 } else { 1.0 }]
            })
            .collect::<Vec<_>>(),
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Extracts one submesh as a compact mesh (vertices re-based to the submesh range).
pub fn submesh_mesh(vertices: &[EdfVertex], indices: &[u32], s: &Submesh) -> Option<Mesh> {
    let t0 = s.first_triangle as usize * 3;
    let t1 = (t0 + s.triangle_count as usize * 3).min(indices.len());
    if t0 >= t1 {
        return None;
    }
    let tris = &indices[t0..t1];
    let lo = *tris.iter().min()? as usize;
    let hi = *tris.iter().max()? as usize;
    let verts = &vertices[lo..=hi];
    Some(mesh_from(verts, tris.iter().map(|i| i - lo as u32).collect()))
}

#[derive(Component)]
pub struct ChainScroll {
    pub base_uvs: Vec<[f32; 2]>,
    pub ratio: f32,
}

/// Spawns nodes of an object that satisfy `predicate` as children of `parent`.
pub fn spawn_object_filtered<F: Fn(&str) -> bool>(
    commands: &mut Commands,
    parent: Entity,
    object: &EdfObject,
    materials: &MaterialSet,
    meshes: &mut Assets<Mesh>,
    predicate: &F,
) {
    for (ni, node) in object.nodes.iter().enumerate() {
        if node.submeshes.is_empty() || !predicate(&node.name) {
            continue;
        }
        let transform = if node.name.eq_ignore_ascii_case("chain") {
            Transform::IDENTITY
        } else {
            Transform::from_matrix(object.node_world(ni))
        };

        for s in &node.submeshes {
            let Some(mesh) = submesh_mesh(&object.vertices, &object.indices, s) else {
                continue;
            };
            let is_chain = node.name.eq_ignore_ascii_case("chain");
            let base_uvs = if is_chain {
                if let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(uvs)) =
                    mesh.attribute(Mesh::ATTRIBUTE_UV_0)
                {
                    Some(uvs.clone())
                } else {
                    None
                }
            } else {
                None
            };
            let slot = object.materials.get(s.material as usize).map(|m| m.texture).unwrap_or(0);
            let mesh_handle = meshes.add(mesh);
            let mut cmd = commands.spawn((
                Mesh3d(mesh_handle.clone()),
                MeshMaterial3d(materials.get(slot)),
                transform,
                Name::new(node.name.clone()),
            ));
            if let Some(base_uvs) = base_uvs {
                cmd.insert(ChainScroll {
                    base_uvs,
                    ratio: 0.035,
                });
            }
            if !node.morphs.is_empty() {
                let morph = node.morphs[0].clone();
                let duration = morph.times.last().copied().unwrap_or(1.0);
                let node_vertex_count = if morph.frames > 0 {
                    morph.positions.len() / morph.frames
                } else {
                    0
                };
                cmd.insert(MorphAnimation {
                    mesh: mesh_handle,
                    morph,
                    node_vertex_count,
                    submesh_first_vertex: s.first_vertex as usize,
                    submesh_vertex_count: s.vertex_count as usize,
                    time: 0.0,
                    duration,
                });
            }
            let child = cmd.id();
            commands.entity(parent).add_child(child);
        }
    }
}

#[derive(Component)]
pub struct MorphAnimation {
    pub mesh: Handle<Mesh>,
    pub morph: MorphSet,
    pub node_vertex_count: usize,
    pub submesh_first_vertex: usize,
    pub submesh_vertex_count: usize,
    pub time: f32,
    pub duration: f32,
}

/// Keyframe vertex morph animation (e.g. waving cloth flags) interpolated at stored frame times.
pub fn animate_morphs(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<&mut MorphAnimation>,
) {
    let dt = time.delta_secs();
    for mut anim in &mut q {
        if anim.morph.frames < 2 || anim.duration <= 0.0 {
            continue;
        }
        anim.time = (anim.time + dt) % anim.duration;
        let t = anim.time;
        let times = &anim.morph.times;
        let mut f0 = 0;
        let mut f1 = (times.len() - 1).min(1);
        for i in 0..times.len().saturating_sub(1) {
            if t >= times[i] && t < times[i + 1] {
                f0 = i;
                f1 = i + 1;
                break;
            }
        }
        let t0 = times[f0];
        let t1 = times[f1];
        let alpha = if (t1 - t0).abs() > 1e-5 {
            ((t - t0) / (t1 - t0)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if let Some(mesh) = meshes.get_mut(&anim.mesh) {
            let nverts = anim.submesh_vertex_count;
            let offset = anim.submesh_first_vertex;
            let total_node = anim.node_vertex_count;
            if total_node == 0 {
                continue;
            }
            let p0_base = f0 * total_node + offset;
            let p1_base = f1 * total_node + offset;

            if p0_base + nverts <= anim.morph.positions.len() && p1_base + nverts <= anim.morph.positions.len() {
                let mut new_pos = Vec::with_capacity(nverts);
                for i in 0..nverts {
                    let p0 = anim.morph.positions[p0_base + i];
                    let p1 = anim.morph.positions[p1_base + i];
                    new_pos.push((p0.lerp(p1, alpha)).to_array());
                }
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, new_pos);
            }
            if p0_base + nverts <= anim.morph.normals.len() && p1_base + nverts <= anim.morph.normals.len() {
                let mut new_norm = Vec::with_capacity(nverts);
                for i in 0..nverts {
                    let n0 = anim.morph.normals[p0_base + i];
                    let n1 = anim.morph.normals[p1_base + i];
                    new_norm.push((n0.lerp(n1, alpha).try_normalize().unwrap_or(Vec3::Y)).to_array());
                }
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, new_norm);
            }
        }
    }
}

/// Spawns every node of an object as children of `parent` with its object-space transform.
pub fn spawn_object(
    commands: &mut Commands,
    parent: Entity,
    object: &EdfObject,
    materials: &MaterialSet,
    meshes: &mut Assets<Mesh>,
) {
    spawn_object_filtered(commands, parent, object, materials, meshes, &|_| true);
}

/// Spawns a named group (or the root group for an empty name) of a model under a new entity, filtering node names.
pub fn spawn_group_filtered<F: Fn(&str) -> bool>(
    commands: &mut Commands,
    model: &EdfModel,
    group: &str,
    materials: &MaterialSet,
    meshes: &mut Assets<Mesh>,
    name: &str,
    predicate: F,
) -> Entity {
    let root = commands
        .spawn((Transform::default(), Visibility::default(), Name::new(name.to_string())))
        .id();
    match model.group(group) {
        Some(g) => {
            for o in &g.objects {
                spawn_object_filtered(commands, root, o, materials, meshes, &predicate);
            }
        }
        None => warn!("group '{group}' not found for {name}"),
    }
    root
}

/// Spawns a named group (or the root group for an empty name) of a model under a new entity.
pub fn spawn_group(
    commands: &mut Commands,
    model: &EdfModel,
    group: &str,
    materials: &MaterialSet,
    meshes: &mut Assets<Mesh>,
    name: &str,
) -> Entity {
    spawn_group_filtered(commands, model, group, materials, meshes, name, |_| true)
}

/// Builds terrain meshes in square chunks, sampling every `step` heightfield samples.
pub fn terrain_chunks(t: &TrackTerrain, step: u32, chunk_quads: u32) -> Vec<(u32, u32, u32, u32, Mesh)> {
    let cols = (t.width - 1) / step;
    let rows = (t.depth - 1) / step;
    let mut out = Vec::new();
    let mut cz = 0;
    while cz < rows {
        let mut cx = 0;
        while cx < cols {
            let nx = chunk_quads.min(cols - cx);
            let nz = chunk_quads.min(rows - cz);
            let mut pos = Vec::with_capacity(((nx + 1) * (nz + 1)) as usize);
            let mut nrm = Vec::with_capacity(pos.capacity());
            for j in 0..=nz {
                for i in 0..=nx {
                    let (gi, gj) = ((cx + i) * step, (cz + j) * step);
                    let p = t.vertex(gi, gj);
                    pos.push(p.to_array());
                    nrm.push(t.normal_at(p.x, p.z).to_array());
                }
            }
            let mut idx = Vec::with_capacity((nx * nz * 6) as usize);
            for j in 0..nz {
                for i in 0..nx {
                    let a = j * (nx + 1) + i;
                    let b = a + 1;
                    let c = a + nx + 1;
                    let d = c + 1;
                    // Diagonal b-c matches TrackTerrain::height_at's cell split; winding is CCW
                    // seen from above in the right-handed frame (asset Z is mirrored).
                    idx.extend_from_slice(&[a, b, c, b, d, c]);
                }
            }
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
            mesh.insert_indices(Indices::U32(idx));
            out.push((cx, cz, nx, nz, mesh));
            cx += chunk_quads;
        }
        cz += chunk_quads;
    }
    out
}

/// Groups scenery runs by material slot into one mesh each.
pub fn scenery_meshes(
    vertices: &[EdfVertex],
    indices: &[u32],
    runs: &[Submesh],
) -> Vec<(u32, Mesh)> {
    let mut by_mat: HashMap<u32, Vec<&Submesh>> = HashMap::new();
    for r in runs {
        by_mat.entry(r.material).or_default().push(r);
    }
    let mut out = Vec::new();
    for (mat, list) in by_mat {
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut verts = Vec::new();
        let mut idx = Vec::new();
        for r in list {
            let t0 = r.first_triangle as usize * 3;
            let t1 = (t0 + r.triangle_count as usize * 3).min(indices.len());
            for &i in &indices[t0..t1] {
                let n = *remap.entry(i).or_insert_with(|| {
                    verts.push(vertices[i as usize]);
                    verts.len() as u32 - 1
                });
                idx.push(n);
            }
        }
        if !idx.is_empty() {
            out.push((mat, mesh_from(&verts, idx)));
        }
    }
    out
}
