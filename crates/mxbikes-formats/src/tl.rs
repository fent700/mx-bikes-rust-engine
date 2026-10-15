//! PiBoSo `.tl` track lines (`minrace`, `maxrace`, `checkleft`, `checkright`).
//!
//!   "TL \0" count:u32 { longitudinal:f32 lateral:f32 } * count
//!
//! Points are expressed in track coordinates: distance along the centreline and lateral offset.

use crate::reader::{ReadError, Reader};

#[derive(Debug, Clone)]
pub struct TrackLine {
    /// (longitudinal distance m, lateral offset m)
    pub points: Vec<(f32, f32)>,
}

impl TrackLine {
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"TL \0" {
            return Err(ReadError::Magic {
                expected: *b"TL \0",
                found: magic,
            });
        }
        let n = r.bounded_count("track line point count", 1_000_000)?;
        let raw = r.f32s(n * 2)?;
        Ok(Self {
            points: raw.chunks_exact(2).map(|p| (p[0], p[1])).collect(),
        })
    }

    /// Lateral offset at a longitudinal distance, linearly interpolated.
    pub fn lateral_at(&self, long: f32) -> Option<f32> {
        let i = self.points.partition_point(|p| p.0 < long);
        match (self.points.get(i.wrapping_sub(1)), self.points.get(i)) {
            (Some(a), Some(b)) if b.0 > a.0 => Some(a.1 + (b.1 - a.1) * (long - a.0) / (b.0 - a.0)),
            (_, Some(b)) => Some(b.1),
            (Some(a), None) => Some(a.1),
            _ => None,
        }
    }
}
