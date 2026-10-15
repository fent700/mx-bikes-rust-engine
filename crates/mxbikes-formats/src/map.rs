//! PiBoSo `.map` track scenery (magic "MP2\0").
//!
//! The menu background `misc/bg/bg.map` uses the older "MAP\0" variant: no version word and no
//! terrain block, otherwise the same scenery mesh and material table. It is reported as version 0.
//!
//!   "MP2\0" version:u32
//!   scenery mesh:
//!     object materials (as `.edf`), vertex_count, SoA vertices (80-byte stride, extra uv2),
//!     triangles, AABB tree { bbox:f32*6 left+1:u32 right+1:u32 (inner: u32*3 | leaf: tri_count base runs { flags material first_tri tri_count first_vertex vertex_count }) }
//!   material table (as `.edf`)
//!   collision mesh + spatial tables (skipped)
//!   terrain block: size_x size_z height:f32 origin:vec3 layer_count
//!     layer: material slot (56 bytes) variant_count { material without paint layers } tiling:f32 (v304: f32*2)
//!            has_mask:u32 [width height blob] surface_param:f32

use crate::edf::{read_indices, read_object_materials, read_vertices, EdfVertex, ObjectMaterialSlot, Submesh};
use crate::reader::{ReadError, Reader};
use crate::texture::{read_material, read_material_table, Material, TextureBlob};

#[derive(Debug, Clone)]
pub struct SceneryMesh {
    pub materials: Vec<ObjectMaterialSlot>,
    pub vertices: Vec<EdfVertex>,
    pub indices: Vec<u32>,
    pub submeshes: Vec<Submesh>,
}

#[derive(Debug, Clone)]
pub struct TerrainMask {
    pub width: u32,
    pub height: u32,
    pub blob: TextureBlob,
}

#[derive(Debug, Clone)]
pub struct TerrainLayer {
    /// Dry, wet, ... variants of the layer surface.
    pub variants: Vec<Material>,
    /// Texture repeats across the terrain extent (u, v). Version 288 files store one value.
    pub tiling: [f32; 2],
    pub mask: Option<TerrainMask>,
    pub surface_param: f32,
}

#[derive(Debug, Clone)]
pub struct TerrainMaterials {
    pub size_x: f32,
    pub size_z: f32,
    pub height: f32,
    pub origin: [f32; 3],
    pub layers: Vec<TerrainLayer>,
}

#[derive(Debug, Clone)]
pub struct TrackMap {
    pub version: u32,
    pub scenery: SceneryMesh,
    pub materials: Vec<Material>,
    pub terrain: Option<TerrainMaterials>,
}

impl TrackMap {
    pub fn material(&self, texture_ref: u32) -> Option<&Material> {
        if texture_ref == 0 {
            None
        } else {
            self.materials.get(texture_ref as usize - 1)
        }
    }

    /// `terrain_extent` is (size_x, size_z, height) from the matching `.trh`; it is used to locate
    /// the terrain material block past the collision data.
    pub fn parse(data: &[u8], terrain_extent: Option<(f32, f32, f32)>) -> Result<Self, ReadError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        let version = match &magic {
            b"MP2\0" => r.u32()?,
            b"MAP\0" => 0,
            _ => {
                return Err(ReadError::Magic {
                    expected: *b"MP2\0",
                    found: magic,
                })
            }
        };
        let scenery = read_scenery(&mut r)?;
        let materials = read_material_table(&mut r)?;
        let terrain = match terrain_extent.filter(|_| version != 0) {
            Some(ext) => find_terrain_block(data, r.pos(), ext)
                .map(|off| read_terrain(&mut Reader::at(data, off), version))
                .transpose()?,
            None => None,
        };
        Ok(Self {
            version,
            scenery,
            materials,
            terrain,
        })
    }
}

fn read_scenery(r: &mut Reader) -> Result<SceneryMesh, ReadError> {
    let materials = read_object_materials(r)?;
    let vertex_count = r.bounded_count("map vertex count", 50_000_000)?;
    let vertices = read_vertices(r, vertex_count, 80)?;
    let indices = read_indices(r, vertex_count)?;
    let node_count = r.bounded_count("map tree node count", 10_000_000)?;
    let mut submeshes = Vec::new();
    for _ in 0..node_count {
        r.skip(24)?;
        let left = r.u32()?;
        let right = r.u32()?;
        if left == 0 && right == 0 {
            r.skip(8)?;
            let runs = r.bounded_count("map leaf run count", 1_000_000)?;
            for _ in 0..runs {
                let w = r.u32s(6)?;
                submeshes.push(Submesh {
                    material: w[1],
                    first_triangle: w[2],
                    triangle_count: w[3],
                    first_vertex: w[4],
                    vertex_count: w[5],
                });
            }
        } else {
            r.skip(12)?;
        }
    }
    Ok(SceneryMesh {
        materials,
        vertices,
        indices,
        submeshes,
    })
}

fn find_terrain_block(data: &[u8], from: usize, (sx, sz, h): (f32, f32, f32)) -> Option<usize> {
    let mut pat = Vec::with_capacity(12);
    for v in [sx, sz, h] {
        pat.extend_from_slice(&v.to_le_bytes());
    }
    data[from..]
        .windows(12)
        .position(|w| w == pat.as_slice())
        .map(|p| from + p)
}

fn read_terrain(r: &mut Reader, version: u32) -> Result<TerrainMaterials, ReadError> {
    let size_x = r.f32()?;
    let size_z = r.f32()?;
    let height = r.f32()?;
    let o = r.f32s(3)?;
    let count = r.bounded_count("terrain layer count", 64)?;
    let mut layers = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip(56)?;
        let variant_count = r.bounded_count("terrain variant count", 16)?;
        let mut variants = Vec::with_capacity(variant_count);
        for _ in 0..variant_count {
            variants.push(read_material(r, false)?);
        }
        let tu = r.f32()?;
        let tiling = [tu, if version >= 304 { r.f32()? } else { tu }];
        let mask = if r.u32()? != 0 {
            let width = r.u32()?;
            let height = r.u32()?;
            let size = r.u32()? as usize;
            let format = crate::texture::PixelFormat::from_code(r.peek_u32()?);
            let offset = r.pos();
            r.skip(size)?;
            Some(TerrainMask {
                width,
                height,
                blob: TextureBlob { offset, size, format },
            })
        } else {
            None
        };
        let surface_param = r.f32()?;
        layers.push(TerrainLayer {
            variants,
            tiling,
            mask,
            surface_param,
        });
    }
    Ok(TerrainMaterials {
        size_x,
        size_z,
        height,
        origin: [o[0], o[1], o[2]],
        layers,
    })
}
