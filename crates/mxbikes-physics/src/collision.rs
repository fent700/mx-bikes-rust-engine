//! Static collision mesh of the track objects (fences, walls, buildings) from the `.map`
//! collision section, with a uniform grid over x / z for sphere queries. The game hands the same
//! mesh to its physics engine as a static triangle mesh.

use glam::Vec3;
use std::collections::HashMap;

const CELL: f32 = 4.0;

#[derive(Debug, Clone, Default)]
pub struct CollisionMesh {
    tris: Vec<[Vec3; 3]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

/// Closest point on triangle `t` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

impl CollisionMesh {
    /// Triangles in the engine (right-handed) frame.
    pub fn new(tris: Vec<[Vec3; 3]>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            for gx in cell(lo.x)..=cell(hi.x) {
                for gz in cell(lo.z)..=cell(hi.z) {
                    grid.entry((gx, gz)).or_default().push(i as u32);
                }
            }
        }
        Self { tris, grid }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Deepest penetration of a sphere into the mesh: (contact normal pointing out of the mesh,
    /// depth). Triangles are treated as two-sided.
    pub fn sphere_contact(&self, center: Vec3, radius: f32) -> Option<(Vec3, f32)> {
        if self.tris.is_empty() {
            return None;
        }
        let mut best: Option<(Vec3, f32)> = None;
        let mut seen: Vec<u32> = Vec::new();
        for gx in cell(center.x - radius)..=cell(center.x + radius) {
            for gz in cell(center.z - radius)..=cell(center.z + radius) {
                let Some(list) = self.grid.get(&(gx, gz)) else { continue };
                for &i in list {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let q = closest_on_triangle(center, self.tris[i as usize]);
                    let d = center - q;
                    let dist = d.length();
                    if dist >= radius {
                        continue;
                    }
                    let n = if dist > 1e-5 {
                        d / dist
                    } else {
                        let t = self.tris[i as usize];
                        (t[1] - t[0]).cross(t[2] - t[0]).normalize_or(Vec3::Y)
                    };
                    let depth = radius - dist;
                    if best.is_none_or(|(_, b)| depth > b) {
                        best = Some((n, depth));
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_against_wall() {
        // A 2 x 2 m wall in the x = 0 plane.
        let m = CollisionMesh::new(vec![
            [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 2.0, -1.0), Vec3::new(0.0, 0.0, 1.0)],
            [Vec3::new(0.0, 2.0, -1.0), Vec3::new(0.0, 2.0, 1.0), Vec3::new(0.0, 0.0, 1.0)],
        ]);
        let (n, d) = m.sphere_contact(Vec3::new(0.3, 1.0, 0.0), 0.5).unwrap();
        assert!((d - 0.2).abs() < 1e-4 && (n - Vec3::X).length() < 1e-4);
        assert!(m.sphere_contact(Vec3::new(0.6, 1.0, 0.0), 0.5).is_none());
    }
}
