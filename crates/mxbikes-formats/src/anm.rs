//! PiBoSo skeletal animation `.anm` format.
//!
//! Header:
//!   "ANM\0" (magic)
//!   version: u32 (1 or 2)
//!
//! Followed by one or more animation clips:
//!   clip_name: name[100] (null-terminated)
//!   frame_count: u32
//!   bone_count: u32
//!
//! For each bone (0 .. bone_count):
//!   bone_name: name[100] (null-terminated)
//!   frame_count keyframes, each 28 bytes (7 × f32):
//!     quat: Quat { x, y, z, w } (16 bytes)
//!     translation: Vec3 { x, y, z } (12 bytes)
//!   weight: f32 per-bone mask (0.0 or 1.0 in stock files) that the game multiplies into the clip
//!   weight when blending (kept as `trailer`)
//!
//! Keys are full parent-relative transforms (translations equal the EDF rest offsets). The stored
//! quaternion is the inverse rotation of the EDF convention, so it is conjugated as well as
//! mirrored into the right-handed frame: (x, y, -z, w). Checked against `rider.edf`: the pelvis
//! key reproduces its rest rotation exactly.
//!
//! Verified on all 92 stock `.anm` files: every quaternion is unit length and every file is
//! consumed exactly.

use crate::reader::{ReadError, Reader};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformKey {
    pub rotation: Quat,
    pub translation: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoneTrack {
    pub name: String,
    pub keys: Vec<TransformKey>,
    /// Per-bone blend mask stored after the keys (0.0 or 1.0 in stock files).
    pub trailer: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationClip {
    pub name: String,
    pub frame_count: u32,
    pub bones: Vec<BoneTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationFile {
    pub version: u32,
    pub clips: Vec<AnimationClip>,
}

impl AnimationFile {
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"ANM\0" {
            return Err(ReadError::Magic {
                expected: *b"ANM\0",
                found: magic,
            });
        }
        let version = r.u32()?;
        let mut clips = Vec::new();

        while r.remaining() >= 108 {
            let name = r.name(100)?;
            let frame_count = r.bounded_count("anm frame count", 100_000)? as u32;
            let bone_count = r.bounded_count("anm bone count", 1024)?;
            let mut bones = Vec::with_capacity(bone_count);

            for _ in 0..bone_count {
                let bone_name = r.name(100)?;
                let mut keys = Vec::with_capacity(frame_count as usize);

                for _ in 0..frame_count {
                    let q = r.f32s(4)?;
                    let t = r.f32s(3)?;
                    keys.push(TransformKey {
                        rotation: Quat::from_xyzw(q[0], q[1], -q[2], q[3]),
                        translation: Vec3::new(t[0], t[1], -t[2]),
                    });
                }

                let trailer = r.f32()?;
                bones.push(BoneTrack {
                    name: bone_name,
                    keys,
                    trailer,
                });
            }

            clips.push(AnimationClip {
                name,
                frame_count,
                bones,
            });
        }

        Ok(Self { version, clips })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_clip() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"ANM\0");
        bytes.extend_from_slice(&2u32.to_le_bytes()); // version 2

        // Clip 1
        let mut name = [0u8; 100];
        name[..4].copy_from_slice(b"test");
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&2u32.to_le_bytes()); // 2 frames
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 1 bone

        // Bone 1
        let mut bname = [0u8; 100];
        bname[..4].copy_from_slice(b"root");
        bytes.extend_from_slice(&bname);

        // Frame 0: identity quat, zero pos
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());

        // Frame 1: identity quat, pos (0, 1, 0)
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes()); // trailer

        let anm = AnimationFile::parse(&bytes).unwrap();
        assert_eq!(anm.version, 2);
        assert_eq!(anm.clips.len(), 1);
        assert_eq!(anm.clips[0].name, "test");
        assert_eq!(anm.clips[0].frame_count, 2);
        assert_eq!(anm.clips[0].bones.len(), 1);
        assert_eq!(anm.clips[0].bones[0].name, "root");
        assert_eq!(anm.clips[0].bones[0].keys[1].translation, Vec3::Y);
    }
}
