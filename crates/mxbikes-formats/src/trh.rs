//! PiBoSo `.trh` terrain heightfield.
//!
//!   "TRH\0" width:u32 depth:u32 heights:u16*width*depth
//!   size_x:f32 height_range:f32 size_z:f32 origin:vec3
//!   surface_count:u32 { surface_id:u32 deform_depth:f32 u32 u32 } * surface_count
//!   followed by wear / deformation maps (kept raw).
//!
//! Heights are signed 16-bit: y = origin.y + (h + 32768) / 65535 * height_range
//! (verified against scenery ground contact and the trackside camera positions in `.tsc`).
//! Sample (i, j) sits at world (origin.x + i/(width-1)*size_x, origin.z + j/(depth-1)*size_z)
//! in the left-handed asset frame; the engine frame mirrors Z.

use crate::centerline::Centerline;
use crate::reader::{ReadError, Reader};
use glam::Vec2;
use glam::Vec3;

pub type TrhError = ReadError;

#[derive(Debug, Clone)]
pub struct TerrainSurface {
    pub surface_id: u32,
    /// Maximum rut depth (m) the surface deforms to.
    pub deform_depth: f32,
    /// Optional coverage mask over the terrain extent (row-major, 0..255).
    pub mask: Option<(u32, u32, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub struct TrackTerrain {
    pub width: u32,
    pub depth: u32,
    pub size_x: f32,
    pub size_z: f32,
    pub height_range: f32,
    /// Asset-frame origin (left-handed).
    pub origin: Vec3,
    pub heights: Vec<i16>,
    pub surfaces: Vec<TerrainSurface>,
    /// Track reference line for long/lat coordinates (None if the file is truncated).
    pub centerline: Option<Centerline>,
}

impl TrackTerrain {
    pub fn parse(data: &[u8]) -> Result<Self, TrhError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"TRH\0" {
            return Err(ReadError::Magic {
                expected: *b"TRH\0",
                found: magic,
            });
        }
        let width = r.bounded_count("trh width", 16385)? as u32;
        let depth = r.bounded_count("trh depth", 16385)? as u32;
        let raw = r.bytes((width * depth * 2) as usize)?;
        let heights = raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let size_x = r.f32()?;
        let height_range = r.f32()?;
        let size_z = r.f32()?;
        let origin = r.vec3()?;
        let n = r.bounded_count("trh surface count", 64)?;
        let mut surfaces = Vec::with_capacity(n);
        for _ in 0..n {
            let surface_id = r.u32()?;
            let deform_depth = r.f32()?;
            let w = r.u32()?;
            let h = r.u32()?;
            let mask = if w > 0 && h > 0 {
                Some((w, h, r.bytes((w * h) as usize)?.to_vec()))
            } else {
                None
            };
            surfaces.push(TerrainSurface {
                surface_id,
                deform_depth,
                mask,
            });
        }
        let centerline = Self::read_centerline(&mut r).ok();
        Ok(Self {
            width,
            depth,
            size_x,
            size_z,
            height_range,
            origin,
            heights,
            surfaces,
            centerline,
        })
    }

    /// Flat default terrain with zero height and rut deformation (used for showroom and menu scene).
    pub fn flat(size_x: f32, size_z: f32) -> Self {
        Self {
            width: 2,
            depth: 2,
            size_x,
            size_z,
            height_range: 0.0,
            origin: Vec3::new(-size_x * 0.5, 0.0, -size_z * 0.5),
            heights: vec![0, 0, 0, 0],
            surfaces: vec![TerrainSurface {
                surface_id: 0,
                deform_depth: 0.0,
                mask: None,
            }],
            centerline: None,
        }
    }

    fn read_centerline(r: &mut Reader) -> Result<Centerline, ReadError> {
        let maps = r.bounded_count("trh map count", 64)?;
        for _ in 0..maps {
            r.skip(4)?;
            let w = r.u32()? as usize;
            let h = r.u32()? as usize;
            r.skip(w * h)?;
        }
        let start = Vec2::new(r.f32()?, r.f32()?);
        let heading = r.f32()?;
        r.skip(4 + 24)?;
        let names = r.bounded_count("trh surface name count", 256)?;
        r.skip(names * 52)?;
        let count = r.bounded_count("centreline segment count", 100_000)?;
        let mut raw = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = r.u32()?;
            let length = r.f32()?;
            let radius = r.f32()?;
            r.skip(12 + 36)?;
            raw.push((kind, length, radius));
        }
        Ok(Centerline::build(start, heading, &raw))
    }

    /// Engine-frame position of a track coordinate (long, lat), on the terrain surface.
    pub fn track_to_world(&self, long: f32, lat: f32) -> Option<Vec3> {
        let p = self.centerline.as_ref()?.to_world(long, lat);
        let (x, z) = (p.x, -p.y);
        Some(Vec3::new(x, self.height_at(x, z), z))
    }

    #[inline]
    pub fn spacing_x(&self) -> f32 {
        self.size_x / (self.width - 1) as f32
    }

    #[inline]
    pub fn spacing_z(&self) -> f32 {
        self.size_z / (self.depth - 1) as f32
    }

    /// Height (asset frame) of grid sample (i, j).
    #[inline]
    pub fn sample(&self, i: u32, j: u32) -> f32 {
        let i = i.min(self.width - 1);
        let j = j.min(self.depth - 1);
        let h = self.heights[(j * self.width + i) as usize] as f32;
        self.origin.y + (h + 32768.0) / 65535.0 * self.height_range
    }

    /// Bilinear height at engine-frame (x, z). Outside the grid the border height is extended.
    pub fn height_at(&self, x: f32, z_rh: f32) -> f32 {
        let gx = ((x - self.origin.x) / self.spacing_x()).clamp(0.0, (self.width - 1) as f32);
        let gz = ((-z_rh - self.origin.z) / self.spacing_z()).clamp(0.0, (self.depth - 1) as f32);
        let (i0, j0) = (gx.floor() as u32, gz.floor() as u32);
        let (fx, fz) = (gx - i0 as f32, gz - j0 as f32);
        let h00 = self.sample(i0, j0);
        let h10 = self.sample(i0 + 1, j0);
        let h01 = self.sample(i0, j0 + 1);
        let h11 = self.sample(i0 + 1, j0 + 1);
        // Split each cell along the same diagonal as the rendered mesh so physics matches graphics.
        if fx + fz <= 1.0 {
            h00 + (h10 - h00) * fx + (h01 - h00) * fz
        } else {
            h11 + (h01 - h11) * (1.0 - fx) + (h10 - h11) * (1.0 - fz)
        }
    }

    /// Unit surface normal (engine frame) at (x, z).
    pub fn normal_at(&self, x: f32, z_rh: f32) -> Vec3 {
        let e = self.spacing_x().max(self.spacing_z());
        let hl = self.height_at(x - e, z_rh);
        let hr = self.height_at(x + e, z_rh);
        let hb = self.height_at(x, z_rh + e);
        let hf = self.height_at(x, z_rh - e);
        Vec3::new(hl - hr, 2.0 * e, hf - hb).normalize()
    }

    /// Engine-frame position of grid sample (i, j).
    pub fn vertex(&self, i: u32, j: u32) -> Vec3 {
        Vec3::new(
            self.origin.x + i as f32 * self.spacing_x(),
            self.sample(i, j),
            -(self.origin.z + j as f32 * self.spacing_z()),
        )
    }

    /// Engine-frame centre of the terrain.
    pub fn center(&self) -> Vec3 {
        Vec3::new(
            self.origin.x + self.size_x * 0.5,
            self.origin.y,
            -(self.origin.z + self.size_z * 0.5),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(w: u32, d: u32, f: impl Fn(u32, u32) -> i16) -> Vec<u8> {
        let mut v = b"TRH\0".to_vec();
        v.extend(w.to_le_bytes());
        v.extend(d.to_le_bytes());
        for j in 0..d {
            for i in 0..w {
                v.extend(f(i, j).to_le_bytes());
            }
        }
        for x in [10.0f32, 2.0, 10.0, 0.0, 0.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes());
        v.extend(11u32.to_le_bytes());
        v.extend(0.1f32.to_le_bytes());
        v.extend([0u8; 8]);
        v
    }

    #[test]
    fn slope_and_normal() {
        // Height rises along +i (asset X): 0 .. 2 m over 10 m.
        let data = synthetic(11, 11, |i, _| (i as i32 * 65535 / 10 - 32768) as i16);
        let t = TrackTerrain::parse(&data).unwrap();
        assert!((t.height_at(5.0, -5.0) - 1.0).abs() < 1e-3);
        let n = t.normal_at(5.0, -5.0);
        assert!(n.x < 0.0 && n.y > 0.9, "normal tilts against the slope: {n:?}");
        assert_eq!(t.surfaces[0].surface_id, 11);
    }
}
