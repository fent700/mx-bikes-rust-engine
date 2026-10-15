//! PiBoSo `.edf` scene files (bikes, wheels, rider gear, skies, track props).
//!
//! Layout (little endian):
//!   "EDF\0" bbox_min:vec3 bbox_max:vec3
//!   root group objects ... (until `root_end`)
//!   material table (see `texture.rs`)
//!   named groups: bbox:vec3*2 objects ...
//!   optional footer: root_end:u32 count:u32 { name[100] offset:u32 } * count  "EXT\0" footer_size:u32
//!
//! Object:
//!   material_count:u32 { flags:u32 color:f32*6 slots:u32*7 } * material_count
//!   vertex_count:u32 then structure-of-arrays:
//!     position:f32*3, uv0:f32*2, uv1:f32*2, color:f32*4, normal:f32*3, tangent:f32*4
//!   triangle_count:u32 indices:u32*3*triangle_count
//!   node_count:u32 nodes...
//!
//! Node:
//!   name[100] flags:u32 transform:f32*16 (row major, parent relative) pivot:f32*16
//!   parent_plus_one:u32 child_count:u32 children:u32*child_count
//!   submesh_count:u32 { kind material first_tri tri_count first_vertex vertex_count } * n
//!   node_vertex_count:u32 reserved:u32
//!   flags & 0x2000: skinning, 124 B per node vertex
//!   flags & 0x10000: morph_count:u32 { frames:u32 position:vec3[v*frames] normal:vec3[v*frames]
//!                    tangent:vec4[v*frames] time:f32[frames] } * morph_count   (v = node_vertex_count)
//!   bbox:vec3*2

use crate::reader::{lh_to_rh, lh_to_rh_mat, ReadError, Reader};
use crate::texture::{read_material_table, Material};
use glam::{Mat4, Vec2, Vec3, Vec4};

pub type EdfError = ReadError;

#[derive(Debug, Clone, Copy)]
pub struct EdfVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv0: Vec2,
    pub uv1: Vec2,
    pub color: Vec4,
    pub tangent: Vec4,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectMaterialSlot {
    pub flags: u32,
    pub color: [f32; 6],
    /// 1-based index into the file material table; 0 means untextured.
    pub texture: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Submesh {
    /// Index into the owning object's `materials`.
    pub material: u32,
    pub first_triangle: u32,
    pub triangle_count: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VertexSkinning {
    pub bones: [u16; 4],
    pub weights: [f32; 4],
}

/// Vertex animation baked per frame (cloth flags). Frame-major, `node_vertex_count` per frame,
/// in the right-handed engine frame.
#[derive(Debug, Clone)]
pub struct MorphSet {
    pub frames: usize,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub times: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct EdfNode {
    pub name: String,
    pub flags: u32,
    /// Parent-relative transform, converted to the right-handed engine frame.
    pub transform: Mat4,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub submeshes: Vec<Submesh>,
    pub bbox: (Vec3, Vec3),
    pub skinning: Option<Vec<VertexSkinning>>,
    pub morphs: Vec<MorphSet>,
}

#[derive(Debug, Clone)]
pub struct EdfObject {
    pub materials: Vec<ObjectMaterialSlot>,
    /// Vertices in node-local space, right-handed frame.
    pub vertices: Vec<EdfVertex>,
    /// Triangle list, winding already flipped for the right-handed frame.
    pub indices: Vec<u32>,
    pub nodes: Vec<EdfNode>,
}

impl EdfObject {
    /// Accumulated object-space transform for a node.
    pub fn node_world(&self, index: usize) -> Mat4 {
        let mut m = self.nodes[index].transform;
        let mut cur = self.nodes[index].parent;
        let mut guard = 0;
        while let Some(p) = cur {
            m = self.nodes[p].transform * m;
            cur = self.nodes[p].parent;
            guard += 1;
            if guard > 256 {
                break;
            }
        }
        m
    }

    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
pub struct EdfGroup {
    /// Empty for the root group.
    pub name: String,
    pub bbox: (Vec3, Vec3),
    pub objects: Vec<EdfObject>,
}

#[derive(Debug, Clone)]
pub struct EdfModel {
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
    pub groups: Vec<EdfGroup>,
    pub materials: Vec<Material>,
}

impl EdfModel {
    pub fn group(&self, name: &str) -> Option<&EdfGroup> {
        if name.is_empty() {
            return self.groups.first();
        }
        self.groups.iter().find(|g| g.name.eq_ignore_ascii_case(name))
    }

    /// Looks up a material by the 1-based reference stored in object material slots.
    pub fn material(&self, texture_ref: u32) -> Option<&Material> {
        if texture_ref == 0 {
            None
        } else {
            self.materials.get(texture_ref as usize - 1)
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self, EdfError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"EDF\0" {
            return Err(ReadError::Magic {
                expected: *b"EDF\0",
                found: magic,
            });
        }
        let bmin = r.vec3()?;
        let bmax = r.vec3()?;
        let (bbox_min, bbox_max) = rh_bbox(bmin, bmax);

        let footer = parse_footer(data)?;
        let mut groups = Vec::new();
        let mut materials = Vec::new();

        match &footer {
            Some(f) => {
                let objects = read_objects_until(&mut r, f.root_end, 72)?;
                groups.push(EdfGroup {
                    name: String::new(),
                    bbox: (bbox_min, bbox_max),
                    objects,
                });
                r.seek(f.root_end);
                let table_end = f.groups.first().map(|g| g.1).unwrap_or(f.start);
                if table_end > f.root_end {
                    materials = read_material_table(&mut r)?;
                }
                for (i, (name, off)) in f.groups.iter().enumerate() {
                    let end = f.groups.get(i + 1).map(|g| g.1).unwrap_or(f.start);
                    r.seek(*off);
                    let a = r.vec3()?;
                    let b = r.vec3()?;
                    let objects = read_objects_until(&mut r, end, 72)?;
                    groups.push(EdfGroup {
                        name: name.clone(),
                        bbox: rh_bbox(a, b),
                        objects,
                    });
                }
            }
            None => {
                let object = read_object(&mut r, 72)?;
                groups.push(EdfGroup {
                    name: String::new(),
                    bbox: (bbox_min, bbox_max),
                    objects: vec![object],
                });
                if r.remaining() >= 4 {
                    materials = read_material_table(&mut r)?;
                }
            }
        }

        Ok(Self {
            bbox_min,
            bbox_max,
            groups,
            materials,
        })
    }
}

struct Footer {
    start: usize,
    root_end: usize,
    groups: Vec<(String, usize)>,
}

fn parse_footer(data: &[u8]) -> Result<Option<Footer>, ReadError> {
    if data.len() < 16 || &data[data.len() - 8..data.len() - 4] != b"EXT\0" {
        return Ok(None);
    }
    let size = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap()) as usize;
    if size < 16 || size > data.len() {
        return Ok(None);
    }
    let start = data.len() - size;
    let mut r = Reader::at(data, start);
    let root_end = r.u32()? as usize;
    let count = r.bounded_count("edf group count", 1024)?;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.name(100)?;
        let off = r.u32()? as usize;
        groups.push((name, off));
    }
    Ok(Some(Footer {
        start,
        root_end,
        groups,
    }))
}

fn rh_bbox(a: Vec3, b: Vec3) -> (Vec3, Vec3) {
    let (a, b) = (lh_to_rh(a), lh_to_rh(b));
    (a.min(b), a.max(b))
}

fn read_objects_until(r: &mut Reader, end: usize, stride: usize) -> Result<Vec<EdfObject>, ReadError> {
    let mut objects = Vec::new();
    // Group bodies may carry a 4-byte terminator before the next section.
    while r.pos() + 8 <= end {
        objects.push(read_object(r, stride)?);
    }
    Ok(objects)
}

/// Reads per-vertex structure-of-arrays blocks. `stride` is 72 for `.edf`, 80 for `.map`
/// (which carries an extra uv2 channel).
pub(crate) fn read_vertices(r: &mut Reader, count: usize, stride: usize) -> Result<Vec<EdfVertex>, ReadError> {
    let pos = r.f32s(count * 3)?;
    let uv0 = r.f32s(count * 2)?;
    let uv1 = r.f32s(count * 2)?;
    if stride == 80 {
        r.skip(count * 8)?;
    }
    let col = r.f32s(count * 4)?;
    let nrm = r.f32s(count * 3)?;
    let tan = r.f32s(count * 4)?;
    Ok((0..count)
        .map(|i| EdfVertex {
            position: lh_to_rh(Vec3::new(pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2])),
            uv0: Vec2::new(uv0[i * 2], uv0[i * 2 + 1]),
            uv1: Vec2::new(uv1[i * 2], uv1[i * 2 + 1]),
            color: Vec4::new(col[i * 4], col[i * 4 + 1], col[i * 4 + 2], col[i * 4 + 3]),
            normal: lh_to_rh(Vec3::new(nrm[i * 3], nrm[i * 3 + 1], nrm[i * 3 + 2])),
            tangent: Vec4::new(tan[i * 4], tan[i * 4 + 1], -tan[i * 4 + 2], -tan[i * 4 + 3]),
        })
        .collect())
}

pub(crate) fn read_indices(r: &mut Reader, vertex_count: usize) -> Result<Vec<u32>, ReadError> {
    let tri_count = r.bounded_count("triangle count", 50_000_000)?;
    let at = r.pos();
    let mut idx = r.u32s(tri_count * 3)?;
    for tri in idx.chunks_exact_mut(3) {
        tri.swap(1, 2);
    }
    if let Some(&bad) = idx.iter().find(|&&i| i as usize >= vertex_count) {
        return Err(ReadError::Implausible {
            what: "vertex index",
            value: bad as u64,
            offset: at,
        });
    }
    Ok(idx)
}

pub(crate) fn read_object_materials(r: &mut Reader) -> Result<Vec<ObjectMaterialSlot>, ReadError> {
    let count = r.bounded_count("object material count", 4096)?;
    (0..count)
        .map(|_| {
            let flags = r.u32()?;
            let c = r.f32s(6)?;
            let slots = r.u32s(7)?;
            Ok(ObjectMaterialSlot {
                flags,
                color: [c[0], c[1], c[2], c[3], c[4], c[5]],
                texture: slots[4],
            })
        })
        .collect()
}

fn read_object(r: &mut Reader, stride: usize) -> Result<EdfObject, ReadError> {
    let materials = read_object_materials(r)?;
    let vertex_count = r.bounded_count("vertex count", 20_000_000)?;
    let vertices = read_vertices(r, vertex_count, stride)?;
    let indices = read_indices(r, vertex_count)?;
    let node_count = r.bounded_count("node count", 65536)?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_node(r)?);
    }
    Ok(EdfObject {
        materials,
        vertices,
        indices,
        nodes,
    })
}

const NODE_MORPH: u32 = 0x10000;
const NODE_SKINNED: u32 = 0x2000;

fn read_node(r: &mut Reader) -> Result<EdfNode, ReadError> {
    let name = r.name(100)?;
    let flags = r.u32()?;
    let transform = lh_to_rh_mat(r.mat4_row_major()?);
    r.skip(64)?;
    let parent_plus_one = r.u32()?;
    let child_count = r.bounded_count("node child count", 65536)?;
    let children = r.u32s(child_count)?.into_iter().map(|c| c as usize).collect();
    let sub_count = r.bounded_count("submesh count", 65536)?;
    let mut submeshes = Vec::with_capacity(sub_count);
    for _ in 0..sub_count {
        let w = r.u32s(6)?;
        submeshes.push(Submesh {
            material: w[1],
            first_triangle: w[2],
            triangle_count: w[3],
            first_vertex: w[4],
            vertex_count: w[5],
        });
    }
    let node_verts = r.bounded_count("node vertex count", 20_000_000)?;
    let _reserved = r.u32()?;
    let skinning = if flags & NODE_SKINNED != 0 {
        let mut weights_vec = Vec::with_capacity(node_verts);
        for _ in 0..node_verts {
            r.skip(40)?; // rest attributes (pos, normal, tangent)
            let bone_count = r.u32()? as usize;
            let bone_ids = r.u32s(10)?;
            let weights = r.f32s(10)?;
            let mut bones = [0u16; 4];
            let mut w = [0.0f32; 4];
            let take = bone_count.min(4);
            for i in 0..take {
                bones[i] = bone_ids[i] as u16;
                w[i] = weights[i];
            }
            let sum: f32 = w.iter().sum();
            if sum > 1e-6 {
                for x in &mut w {
                    *x /= sum;
                }
            }
            weights_vec.push(VertexSkinning { bones, weights: w });
        }
        Some(weights_vec)
    } else {
        None
    };
    let mut morphs = Vec::new();
    if flags & NODE_MORPH != 0 {
        let count = r.bounded_count("morph set count", 256)?;
        for _ in 0..count {
            let frames = r.bounded_count("morph frame count", 100_000)?;
            let n = node_verts * frames;
            let vec3s = |r: &mut Reader| -> Result<Vec<Vec3>, ReadError> {
                Ok(r.f32s(n * 3)?.chunks_exact(3).map(|c| lh_to_rh(Vec3::new(c[0], c[1], c[2]))).collect())
            };
            let positions = vec3s(r)?;
            let normals = vec3s(r)?;
            r.skip(n * 16)?;
            let times = r.f32s(frames)?;
            morphs.push(MorphSet {
                frames,
                positions,
                normals,
                times,
            });
        }
    }
    let a = r.vec3()?;
    let b = r.vec3()?;
    Ok(EdfNode {
        name,
        flags,
        transform,
        parent: parent_plus_one.checked_sub(1).map(|p| p as usize),
        children,
        submeshes,
        bbox: rh_bbox(a, b),
        skinning,
        morphs,
    })
}
