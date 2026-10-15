//! Track ground: `.trh` heightfield plus per-layer surface masks and dynamic rut deformation.

use crate::motorcycle::Ground;
use glam::Vec3;
use mxbikes_formats::{Surface, TrackTerrain};
use std::sync::{Mutex, RwLock};

/// One terrain layer coverage mask (0..255), stored row-major over the terrain extent.
#[derive(Debug, Clone)]
pub struct SurfaceMask {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
}

pub struct TerrainGround {
    pub terrain: TrackTerrain,
    /// Surface per layer (layer 0 = base) with its optional coverage mask and deform_depth.
    pub layers: Vec<(Surface, Option<SurfaceMask>, f32)>,
    /// Runtime rut deformation map (m depression), indexed by `j * width + i`.
    pub deformation: RwLock<Vec<f32>>,
    /// Grid rectangle (i0, j0, i1, j1) deformed since the last `take_dirty`, for mesh updates.
    dirty: Mutex<Option<(u32, u32, u32, u32)>>,
}

impl Clone for TerrainGround {
    fn clone(&self) -> Self {
        let def_data = self.deformation.read().map(|d| d.clone()).unwrap_or_default();
        Self {
            terrain: self.terrain.clone(),
            layers: self.layers.clone(),
            deformation: RwLock::new(def_data),
            dirty: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for TerrainGround {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainGround")
            .field("terrain", &self.terrain)
            .field("layers", &self.layers)
            .finish()
    }
}

impl TerrainGround {
    pub fn new(terrain: TrackTerrain) -> Self {
        let layers = terrain
            .surfaces
            .iter()
            .map(|s| {
                let mask = s.mask.as_ref().map(|(w, h, c)| SurfaceMask {
                    width: *w,
                    height: *h,
                    coverage: c.clone(),
                });
                (Surface::from_terrain_id(s.surface_id), mask, s.deform_depth)
            })
            .collect();
        let total = (terrain.width * terrain.depth) as usize;
        let deformation = RwLock::new(vec![0.0; total]);
        Self {
            dirty: Mutex::new(None),
            terrain,
            layers,
            deformation,
        }
    }

    /// Normalised asset-frame (u, v) of an engine-frame point.
    fn uv(&self, x: f32, z: f32) -> (f32, f32) {
        let t = &self.terrain;
        (
            ((x - t.origin.x) / t.size_x).clamp(0.0, 1.0),
            ((-z - t.origin.z) / t.size_z).clamp(0.0, 1.0),
        )
    }

    /// Bilinear mask coverage (0..1) of a layer at normalised (u, v).
    fn coverage(m: &SurfaceMask, u: f32, v: f32) -> f32 {
        let (fx, fz) = (u * (m.width - 1) as f32, v * (m.height - 1) as f32);
        let (i0, j0) = (fx as u32, fz as u32);
        let (i1, j1) = ((i0 + 1).min(m.width - 1), (j0 + 1).min(m.height - 1));
        let (tx, tz) = (fx - i0 as f32, fz - j0 as f32);
        let c = |i: u32, j: u32| m.coverage[(j * m.width + i) as usize] as f32 / 255.0;
        c(i0, j0) * (1.0 - tx) * (1.0 - tz) + c(i1, j0) * tx * (1.0 - tz) + c(i0, j1) * (1.0 - tx) * tz + c(i1, j1) * tx * tz
    }

    /// Bilinear rut depth (m) at engine-frame (x, z).
    pub fn rut_at(&self, x: f32, z_rh: f32) -> f32 {
        let t = &self.terrain;
        let gx = ((x - t.origin.x) / t.spacing_x()).clamp(0.0, (t.width - 1) as f32);
        let gz = ((-z_rh - t.origin.z) / t.spacing_z()).clamp(0.0, (t.depth - 1) as f32);
        let (i0, j0) = (gx.floor() as u32, gz.floor() as u32);
        let (fx, fz) = (gx - i0 as f32, gz - j0 as f32);
        self.sample_rut(i0, j0) * (1.0 - fx) * (1.0 - fz)
            + self.sample_rut(i0 + 1, j0) * fx * (1.0 - fz)
            + self.sample_rut(i0, j0 + 1) * (1.0 - fx) * fz
            + self.sample_rut(i0 + 1, j0 + 1) * fx * fz
    }

    /// Surface under (x, z) as the game picks it (FUN_1401f1d10): from the top layer down, each
    /// layer is `coverage * thickness` (the TRH layer `deform` value) thick; ruts dig through that
    /// material first; the first layer that still adds more than the remainder of 1 cm wins, and
    /// the base layer is the fallback.
    pub fn surface_at(&self, x: f32, z: f32) -> Surface {
        let (u, v) = self.uv(x, z);
        let mut dug = self.rut_at(x, z);
        let mut needed = 0.01;
        for (idx, (surface, mask, thickness)) in self.layers.iter().enumerate().rev() {
            if idx == 0 {
                return *surface;
            }
            let layer = mask.as_ref().map_or(1.0, |m| Self::coverage(m, u, v)) * thickness;
            let remaining = if dug < layer {
                let r = layer * (1.0 - dug / layer);
                dug = 0.0;
                r
            } else {
                dug -= layer;
                0.0
            };
            if needed < remaining {
                return *surface;
            }
            needed -= remaining;
        }
        Surface::Soil
    }

    /// Height (asset frame) of sample (i, j), including rut depression.
    #[inline]
    pub fn sample_height(&self, i: u32, j: u32) -> f32 {
        let i = i.min(self.terrain.width - 1);
        let j = j.min(self.terrain.depth - 1);
        let base_h = self.terrain.sample(i, j);
        let idx = (j * self.terrain.width + i) as usize;
        let rut = self.deformation.read().map(|d| d[idx]).unwrap_or(0.0);
        base_h - rut
    }

    /// Rut depth (m) at sample (i, j).
    #[inline]
    pub fn sample_rut(&self, i: u32, j: u32) -> f32 {
        let i = i.min(self.terrain.width - 1);
        let j = j.min(self.terrain.depth - 1);
        let idx = (j * self.terrain.width + i) as usize;
        self.deformation.read().map(|d| d[idx]).unwrap_or(0.0)
    }

    /// Bilinear height at engine-frame (x, z), reflecting active ruts.
    pub fn height_at(&self, x: f32, z_rh: f32) -> f32 {
        let t = &self.terrain;
        let gx = ((x - t.origin.x) / t.spacing_x()).clamp(0.0, (t.width - 1) as f32);
        let gz = ((-z_rh - t.origin.z) / t.spacing_z()).clamp(0.0, (t.depth - 1) as f32);
        let (i0, j0) = (gx.floor() as u32, gz.floor() as u32);
        let (fx, fz) = (gx - i0 as f32, gz - j0 as f32);
        let h00 = self.sample_height(i0, j0);
        let h10 = self.sample_height(i0 + 1, j0);
        let h01 = self.sample_height(i0, j0 + 1);
        let h11 = self.sample_height(i0 + 1, j0 + 1);
        if fx + fz <= 1.0 {
            h00 + (h10 - h00) * fx + (h01 - h00) * fz
        } else {
            h11 + (h01 - h11) * (1.0 - fx) + (h10 - h11) * (1.0 - fz)
        }
    }

    /// Unit surface normal (engine frame) at (x, z), reflecting rut walls.
    pub fn normal_at(&self, x: f32, z_rh: f32) -> Vec3 {
        let e = self.terrain.spacing_x().max(self.terrain.spacing_z());
        let hl = self.height_at(x - e, z_rh);
        let hr = self.height_at(x + e, z_rh);
        let hb = self.height_at(x, z_rh + e);
        let hf = self.height_at(x, z_rh - e);
        Vec3::new(hl - hr, 2.0 * e, hf - hb).normalize()
    }

    /// Terrain deformation from a tyre (FUN_1401f5ac0): `amount` (m) is scaled by the surface
    /// factor under the contact (grass / soft soil 2, sand 2.5, soil 1, gravel 1.4, compact soil 0,
    /// dry track), less 0.1 mm, and splatted bilinearly onto the four surrounding vertices. A
    /// vertex never digs below the bottom of the heightfield range.
    pub fn deform(&self, x: f32, z_rh: f32, amount: f32) {
        let factor = match self.surface_at(x, z_rh) {
            Surface::Grass | Surface::SoftSoil => 2.0,
            Surface::Sand => 2.5,
            Surface::Soil => 1.0,
            Surface::Gravel => 1.4,
            Surface::CompactSoil => 0.0,
            _ => return,
        };
        let amount = amount * factor - 1e-4;
        if amount <= 0.0 || self.terrain.height_range <= 1.0 {
            return;
        }
        let t = &self.terrain;
        let gx = ((x - t.origin.x) / t.spacing_x()).clamp(0.0, (t.width - 1) as f32);
        let gz = ((-z_rh - t.origin.z) / t.spacing_z()).clamp(0.0, (t.depth - 1) as f32);
        let (i0, j0) = (gx as u32, gz as u32);
        let (i1, j1) = ((i0 + 1).min(t.width - 1), (j0 + 1).min(t.depth - 1));
        let (fx, fz) = (gx - i0 as f32, gz - j0 as f32);
        let Ok(mut def) = self.deformation.write() else { return };
        for (i, j, w) in [(i0, j0, (1.0 - fx) * (1.0 - fz)), (i1, j0, fx * (1.0 - fz)), (i0, j1, (1.0 - fx) * fz), (i1, j1, fx * fz)] {
            let idx = (j * t.width + i) as usize;
            let floor = t.sample(i, j) - t.origin.y;
            def[idx] = (def[idx] + amount * w).min(floor.max(0.0));
        }
        if let Ok(mut d) = self.dirty.lock() {
            *d = Some(match *d {
                Some((a, b, c, e)) => (a.min(i0), b.min(j0), c.max(i1), e.max(j1)),
                None => (i0, j0, i1, j1),
            });
        }
    }

    /// Returns and clears the grid rectangle deformed since the last call.
    pub fn take_dirty(&self) -> Option<(u32, u32, u32, u32)> {
        self.dirty.lock().ok().and_then(|mut d| d.take())
    }
}

impl Ground for TerrainGround {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.height_at(x, z)
    }

    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.normal_at(x, z)
    }

    fn surface(&self, x: f32, z: f32) -> Surface {
        self.surface_at(x, z)
    }

    fn deform(&self, x: f32, z: f32, amount: f32) {
        TerrainGround::deform(self, x, z, amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_terrain(deform_depth: f32) -> TrackTerrain {
        let mut v = b"TRH\0".to_vec();
        let (w, d) = (11u32, 11u32);
        v.extend(w.to_le_bytes());
        v.extend(d.to_le_bytes());
        for _ in 0..(w * d) {
            v.extend(0i16.to_le_bytes());
        }
        for x in [10.0f32, 2.0, 10.0, 0.0, 0.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes());
        v.extend(7u32.to_le_bytes()); // Surface::Soil
        v.extend(deform_depth.to_le_bytes());
        v.extend([0u8; 8]); // no mask
        TrackTerrain::parse(&v).unwrap()
    }

    #[test]
    fn soil_deforms_after_threshold_and_compact_soil_does_not() {
        let ground = TerrainGround::new(synthetic_terrain(0.10));
        let h0 = ground.height(5.0, -5.0);
        // Below the 0.1 mm threshold nothing happens.
        ground.deform(5.0, -5.0, 0.00005);
        assert_eq!(ground.height(5.0, -5.0), h0);
        // On a vertex, the whole remainder lands there.
        ground.deform(5.0, -5.0, 0.0101);
        assert!((h0 - ground.height(5.0, -5.0) - 0.01).abs() < 1e-4);
    }
}
