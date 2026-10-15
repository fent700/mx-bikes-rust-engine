//! Track centreline stored in `.trh`: the reference line every `long`/`lat` track coordinate in
//! `.rdf` (gates, checkpoints, start stalls) and `.tl` (track lines) is measured along.
//!
//! The line starts at (x0, z0) with heading a0 (degrees) and is a chain of segments: straights
//! and circular arcs of signed radius. A running 3x3 2D transform is carried along the chain:
//! straights advance it, arcs rotate it by `length / (2 pi |r|) * 360` degrees (negated for
//! r <= 0). A point at (long, lat) inside a segment is (lat, long) in segment space for straights
//! and polar about the arc centre for arcs. Coordinates are asset-frame (x, z).
//!
//! Verified on stock tracks: closed circuits return to their start point and `start_stall0`
//! maps exactly onto the `.rdf` editor fields `startposx/startposz`, with heading
//! `atan2(dx, dz) + stall angle == startangleabs`.

use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Straight,
    Arc,
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub kind: SegmentKind,
    pub length: f32,
    /// Signed radius (arcs only).
    pub radius: f32,
    /// Distance along the line where the segment starts.
    pub start: f32,
    /// Row-major 2D affine transform [m0 m1 m2; m3 m4 m5] for the segment.
    xf: [f32; 6],
}

#[derive(Debug, Clone)]
pub struct Centerline {
    pub start: Vec2,
    pub start_heading_deg: f32,
    pub segments: Vec<Segment>,
    pub length: f32,
    /// Point-to-point lines clamp distances instead of wrapping.
    pub open: bool,
}

type Mat = [f32; 9];

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut c = [0.0; 9];
    for r in 0..3 {
        for k in 0..3 {
            c[r * 3 + k] = (0..3).map(|j| a[r * 3 + j] * b[j * 3 + k]).sum();
        }
    }
    c
}

fn rot(deg: f32) -> Mat {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s, 0.0, -s, c, 0.0, 0.0, 0.0, 1.0]
}

impl Segment {
    fn local(&self, long: f32, lat: f32) -> Vec2 {
        match self.kind {
            SegmentKind::Straight => Vec2::new(lat, long),
            SegmentKind::Arc => {
                let r = self.radius;
                let arm = r - lat;
                let a = (long / (r * std::f32::consts::TAU) * 360.0).to_radians();
                let x = if r > 0.0 { r - a.cos() * arm } else { -(a.cos() * arm) + r };
                Vec2::new(x, a.sin() * arm)
            }
        }
    }

    fn eval(&self, long: f32, lat: f32) -> Vec2 {
        let p = self.local(long, lat);
        let m = &self.xf;
        Vec2::new(p.x * m[0] + p.y * m[1] + m[2], p.x * m[3] + p.y * m[4] + m[5])
    }
}

impl Centerline {
    /// Builds the line from its stored header and segment list ((kind, length, radius) each).
    pub fn build(start: Vec2, heading_deg: f32, raw: &[(u32, f32, f32)]) -> Self {
        let mut m = mul(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], &rot(heading_deg));
        m[2] = start.x;
        m[5] = start.y;
        let mut acc = 0.0;
        let mut segments = Vec::with_capacity(raw.len());
        for &(kind, length, radius) in raw {
            let kind = if kind == 1 { SegmentKind::Arc } else { SegmentKind::Straight };
            let seg = Segment {
                kind,
                length,
                radius,
                start: acc,
                xf: [m[0], m[1], m[2], m[3], m[4], m[5]],
            };
            acc += length;
            if kind == SegmentKind::Arc && radius.abs() > 1e-6 {
                let sweep = length / (radius.abs() * std::f32::consts::TAU) * 360.0;
                m = mul(&m, &rot(if radius <= 0.0 { -sweep } else { sweep }));
            }
            let end = seg.eval(length, 0.0);
            m[2] = end.x;
            m[5] = end.y;
            segments.push(seg);
        }
        let end = Vec2::new(m[2], m[5]);
        // In the engine (FUN_1401f3df0 lines 26-34), if the distance between the
        // end of the last segment and the line's start is > 0.01 m (or length < 0.1 m),
        // the line is flagged as open (point-to-point): *(param_1 + 0x20) = 1.
        let open = end.distance(start) > 0.01 || acc < 0.1;
        Self {
            start,
            start_heading_deg: heading_deg,
            segments,
            length: acc,
            open,
        }
    }

    fn normalise(&self, long: f32) -> f32 {
        if self.open {
            long.clamp(0.0, self.length)
        } else {
            long.rem_euclid(self.length.max(1e-3))
        }
    }

    /// Asset-frame (x, z) of a track coordinate.
    pub fn to_world(&self, long: f32, lat: f32) -> Vec2 {
        let long = self.normalise(long);
        let seg = self
            .segments
            .iter()
            .find(|s| long <= s.start + s.length)
            .or(self.segments.last());
        match seg {
            Some(s) => s.eval(long - s.start, lat),
            None => self.start,
        }
    }

    /// Asset-frame heading (degrees, `atan2(dx, dz)`) of the direction of travel.
    pub fn heading_deg(&self, long: f32) -> f32 {
        let a = self.to_world(long - 0.05, 0.0);
        let b = self.to_world(long + 0.05, 0.0);
        let d = b - a;
        d.x.atan2(d.y).to_degrees()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_arcs_closes() {
        // Four straights joined by quarter circles of radius 10 form a closed loop.
        let q = 10.0 * std::f32::consts::TAU / 4.0;
        let raw = [(0, 50.0, 0.0), (1, q, 10.0), (0, 50.0, 0.0), (1, q, 10.0), (0, 50.0, 0.0), (1, q, 10.0), (0, 50.0, 0.0), (1, q, 10.0)];
        let c = Centerline::build(Vec2::new(5.0, 7.0), 30.0, &raw);
        let end = c.segments.last().unwrap().eval(q, 0.0);
        assert!(end.distance(Vec2::new(5.0, 7.0)) < 1e-3, "end {end:?}");
        assert!(!c.open);
        // Lateral offset is perpendicular to travel.
        let p = c.to_world(10.0, 0.0);
        let side = c.to_world(10.0, 2.0);
        assert!((p.distance(side) - 2.0).abs() < 1e-4);
    }
}
