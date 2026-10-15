//! PiBoSo `.prm` encrypted parameter container (magic "PYRC\0" footer).
//!
//! Stored in `rider.pkz!rider/rider.prm`, loaded by engine function `FUN_140192ec0` via
//! container decryptor `FUN_140157920`.
//!
//! Binary layout:
//!   - Payload: encrypted bytes (length stored in footer).
//!   - 12-byte footer at end of file:
//!       magic: [u8; 4] = b"PYRC" (0x43525950)
//!       seed: u32 (0x201 in stock rider.prm)
//!       length: u32 (payload length matching total_len - 12)
//!
//! Cipher: ARC4 (RC4) stream cipher:
//!   - Master key: 16 bytes `b"XL$S$udrla#!oaSW"` (from literal 0x7264752453244c58 + "la#!oaSW")
//!   - KSA key scheduling with master key
//!   - Discard count: `seed + 0x100` bytes (769 bytes for seed 0x201) (FUN_14015af80)
//!   - PRGA decrypt payload (FUN_14015b070)
//!
//! Decrypted payload is PiBoSo key/value block text parsed into rider parameters.

use crate::piboso_tree::ConfigNode;
use thiserror::Error;

const PRM_MASTER_KEY: &[u8; 16] = b"XL$S$udrla#!oaSW";
const PRM_MAGIC: u32 = 0x43525950; // "PYRC" in little-endian

#[derive(Error, Debug)]
pub enum PrmError {
    #[error("File too short for PRM container footer (expected >= 12 bytes, got {0})")]
    TooShort(usize),
    #[error("Invalid PRM container magic (expected 0x43525950 'PYRC', got {0:#x})")]
    InvalidMagic(u32),
    #[error("Container payload length mismatch (footer says {footer_len}, buffer has {actual_len})")]
    LengthMismatch {
        footer_len: usize,
        actual_len: usize,
    },
    #[error("Failed to parse decrypted tree: {0}")]
    Parse(#[from] crate::piboso_tree::ParseError),
}

/// Decrypts a PiBoSo PYRC container using the master key and container footer.
pub fn decrypt_prm(data: &[u8]) -> Result<Vec<u8>, PrmError> {
    if data.len() < 12 {
        return Err(PrmError::TooShort(data.len()));
    }
    let footer_start = data.len() - 12;
    let magic = u32::from_le_bytes(data[footer_start..footer_start + 4].try_into().unwrap());
    if magic != PRM_MAGIC {
        return Err(PrmError::InvalidMagic(magic));
    }
    let seed = u32::from_le_bytes(data[footer_start + 4..footer_start + 8].try_into().unwrap());
    let length = u32::from_le_bytes(data[footer_start + 8..footer_start + 12].try_into().unwrap()) as usize;
    if length != footer_start {
        return Err(PrmError::LengthMismatch {
            footer_len: length,
            actual_len: footer_start,
        });
    }

    // ARC4 KSA (FUN_14015af80)
    let mut s = [0u8; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(PRM_MASTER_KEY[i % PRM_MASTER_KEY.len()]);
        s.swap(i, j as usize);
    }

    // Discard initial seed + 0x100 bytes (FUN_14015af80 lines 39-51)
    let discard = (seed as usize).wrapping_add(0x100);
    let mut i: u8 = 0;
    j = 0;
    for _ in 0..discard {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
    }

    // PRGA decrypt payload (FUN_14015b070)
    let mut out = vec![0u8; length];
    for k in 0..length {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        let key_byte = s[(s[i as usize].wrapping_add(s[j as usize])) as usize];
        out[k] = data[k] ^ key_byte;
    }
    Ok(out)
}

/// Rider physics and spring parameters loaded from `rider.prm` (`FUN_140192ec0`).
#[derive(Debug, Clone, PartialEq)]
pub struct RiderPrm {
    /// Torso lean slew rate (+0x23780, default 10.0, stock rider.prm: 2.0).
    pub lean_speed: f32,
    /// Fore/aft pitch slew rate (+0x23784, default 4.0, stock rider.prm: 5.0).
    pub pitch_speed: f32,
    pub pitch_spring_seat_forward: f32,
    pub pitch_spring_seat_backward: f32,
    pub pitch_spring_standing_forward: f32,
    pub pitch_spring_standing_backward: f32,
    pub pitch_spring_damping: f32,
    pub rotation_spring: f32,
    pub long_spring: f32,
    pub springs_factor: f32,
    pub long_damping: f32,
    pub legs_seat_spring: f32,
    pub legs_stand_spring: f32,
    pub legs_comp_damping: f32,
    pub legs_ext_damping: f32,
    pub lean: f32,
    pub sitting_lean: f32,
    pub standing_pos: f32,
    pub sitting_pitch_forward: f32,
    pub sitting_pitch_backward: f32,
    pub standing_pitch_forward: f32,
    pub standing_pitch_backward: f32,
    pub sitting_pitch_max_angle_forward: f32,
    pub sitting_pitch_max_angle_backward: f32,
    pub sitting_pitch_rate: f32,
    pub standing_pitch_max_angle_forward: f32,
    pub standing_pitch_max_angle_backward: f32,
    pub standing_pitch_rate: f32,
    pub wheelie_pitch_max_angle: f32,
    pub wheelie_pitch_rate: f32,
    pub stoppie_pitch_max_angle: f32,
    pub stoppie_pitch_rate: f32,
    pub jump_pitch_max_angle: f32,
    pub jump_pitch_rate: f32,
    pub stability_pitch_factor: f32,
    pub stability_pitch_rate: f32,
    pub stability_yaw_factor: f32,
    pub stability_yaw_rate: f32,
    pub stability_roll_factor: f32,
    pub stability_roll_rate: f32,
    pub steer_rot: f32,
}

impl Default for RiderPrm {
    /// Engine defaults initialized in `FUN_140192cc0`.
    fn default() -> Self {
        Self {
            lean_speed: 10.0,
            pitch_speed: 4.0,
            pitch_spring_seat_forward: 0.5,
            pitch_spring_seat_backward: 0.5,
            pitch_spring_standing_forward: 0.5,
            pitch_spring_standing_backward: 0.5,
            pitch_spring_damping: 0.5,
            rotation_spring: 5.0,
            long_spring: 9000.0,
            springs_factor: 3.0,
            long_damping: 0.8,
            legs_seat_spring: 720.0,
            legs_stand_spring: 360.0,
            legs_comp_damping: 0.8,
            legs_ext_damping: 2.4,
            lean: 45.0,
            sitting_lean: 20.0,
            standing_pos: -0.05,
            sitting_pitch_forward: 40.0,
            sitting_pitch_backward: 20.0,
            standing_pitch_forward: 35.0,
            standing_pitch_backward: 0.0,
            sitting_pitch_max_angle_forward: 30.0,
            sitting_pitch_max_angle_backward: 30.0,
            sitting_pitch_rate: 300.0,
            standing_pitch_max_angle_forward: 45.0,
            standing_pitch_max_angle_backward: 45.0,
            standing_pitch_rate: 450.0,
            wheelie_pitch_max_angle: 45.0,
            wheelie_pitch_rate: 450.0,
            stoppie_pitch_max_angle: 45.0,
            stoppie_pitch_rate: 450.0,
            jump_pitch_max_angle: 45.0,
            jump_pitch_rate: 450.0,
            stability_pitch_factor: 10.0,
            stability_pitch_rate: 1.0,
            stability_yaw_factor: 2.0,
            stability_yaw_rate: 0.5,
            stability_roll_factor: 2.0,
            stability_roll_rate: 0.0,
            steer_rot: 1.0,
        }
    }
}

impl RiderPrm {
    /// Parses and decrypts a `.prm` container.
    /// Parses `rider.prm`, encrypted or already decrypted (the VFS decrypts on read).
    pub fn parse(data: &[u8]) -> Result<Self, PrmError> {
        let dec = if data.len() >= 12 && &data[data.len() - 12..data.len() - 8] == b"PYRC" {
            decrypt_prm(data)?
        } else {
            data.to_vec()
        };
        let text = match std::str::from_utf8(&dec) {
            Ok(s) => s.to_string(),
            Err(_) => dec.iter().map(|&b| b as char).collect::<String>(),
        };
        let root = ConfigNode::parse(&text)?;
        Ok(Self::from_tree(&root))
    }

    /// Extracts rider parameters from a parsed configuration tree (`FUN_140192ec0`).
    pub fn from_tree(root: &ConfigNode) -> Self {
        let mut p = Self::default();

        if let Some(v) = root.get_f32("LeanSpeed") {
            p.lean_speed = v;
        }
        if let Some(v) = root.get_f32("PitchSpeed") {
            p.pitch_speed = v;
        }

        let ps = root.get_child("pitchspring");
        if let Some(ps) = ps {
            if let Some(v) = ps.get_f32("seatforward").or_else(|| ps.get_f32("forward")) {
                p.pitch_spring_seat_forward = v;
            }
            if let Some(v) = ps.get_f32("seatbackward").or_else(|| ps.get_f32("backward")) {
                p.pitch_spring_seat_backward = v;
            }
            if let Some(v) = ps.get_f32("standingforward").or_else(|| ps.get_f32("forward")) {
                p.pitch_spring_standing_forward = v;
            }
            if let Some(v) = ps.get_f32("standingbackward").or_else(|| ps.get_f32("backward")) {
                p.pitch_spring_standing_backward = v;
            }
            if let Some(v) = ps.get_f32("damping") {
                p.pitch_spring_damping = v;
            }
        }

        if let Some(v) = root.get_f32("rotationspring") {
            p.rotation_spring = v;
        }
        if let Some(v) = root.get_f32("longspring") {
            p.long_spring = v;
        }
        if let Some(v) = root.get_f32("springsfactor") {
            p.springs_factor = v;
        }
        if let Some(v) = root.get_f32("longdamping") {
            p.long_damping = v;
        }

        if let Some(legs) = root.get_child("legs") {
            if let Some(v) = legs.get_f32("SeatSpring") {
                p.legs_seat_spring = v;
            }
            if let Some(v) = legs.get_f32("StandSpring") {
                p.legs_stand_spring = v;
            }
            if let Some(v) = legs.get_f32("CompDamping") {
                p.legs_comp_damping = v;
            }
            if let Some(v) = legs.get_f32("ExtDamping") {
                p.legs_ext_damping = v;
            }
        }

        if let Some(v) = root.get_f32("lean") {
            p.lean = v;
        }

        if let Some(sitting) = root.get_child("sitting") {
            if let Some(v) = sitting.get_f32("lean") {
                p.sitting_lean = v;
            }
            if let Some(pitch) = sitting.get_child("pitch") {
                if let Some(v) = pitch.get_f32("Forward") {
                    p.sitting_pitch_forward = v;
                }
                if let Some(v) = pitch.get_f32("Backward") {
                    p.sitting_pitch_backward = v;
                }
                if let Some(v) = pitch.get_f32("MaxAngleForward") {
                    p.sitting_pitch_max_angle_forward = v;
                }
                if let Some(v) = pitch.get_f32("MaxAngleBackward") {
                    p.sitting_pitch_max_angle_backward = v;
                }
                if let Some(v) = pitch.get_f32("Rate") {
                    p.sitting_pitch_rate = v;
                }
            }
        }

        if let Some(standing) = root.get_child("standing") {
            if let Some(v) = standing.get_f32("pos") {
                p.standing_pos = v;
            }
            if let Some(pitch) = standing.get_child("pitch") {
                if let Some(v) = pitch.get_f32("Forward") {
                    p.standing_pitch_forward = v;
                }
                if let Some(v) = pitch.get_f32("Backward") {
                    p.standing_pitch_backward = v;
                }
                if let Some(v) = pitch.get_f32("MaxAngleForward") {
                    p.standing_pitch_max_angle_forward = v;
                }
                if let Some(v) = pitch.get_f32("MaxAngleBackward") {
                    p.standing_pitch_max_angle_backward = v;
                }
                if let Some(v) = pitch.get_f32("Rate") {
                    p.standing_pitch_rate = v;
                }
            }
        }

        if let Some(wheelie) = root.get_child("wheelie") {
            if let Some(pitch) = wheelie.get_child("pitch") {
                if let Some(v) = pitch.get_f32("MaxAngle") {
                    p.wheelie_pitch_max_angle = v;
                }
                if let Some(v) = pitch.get_f32("Rate") {
                    p.wheelie_pitch_rate = v;
                }
            }
        }

        if let Some(stoppie) = root.get_child("stoppie") {
            if let Some(pitch) = stoppie.get_child("pitch") {
                if let Some(v) = pitch.get_f32("MaxAngle") {
                    p.stoppie_pitch_max_angle = v;
                }
                if let Some(v) = pitch.get_f32("Rate") {
                    p.stoppie_pitch_rate = v;
                }
            }
        }

        if let Some(jump) = root.get_child("jump") {
            if let Some(pitch) = jump.get_child("pitch") {
                if let Some(v) = pitch.get_f32("MaxAngle") {
                    p.jump_pitch_max_angle = v;
                }
                if let Some(v) = pitch.get_f32("Rate") {
                    p.jump_pitch_rate = v;
                }
            }
        }

        if let Some(stab) = root.get_child("stability") {
            if let Some(pitch) = stab.get_child("pitch") {
                if let Some(v) = pitch.get_f32("factor") {
                    p.stability_pitch_factor = v;
                }
                if let Some(v) = pitch.get_f32("rate") {
                    p.stability_pitch_rate = v;
                }
            }
            if let Some(yaw) = stab.get_child("yaw") {
                if let Some(v) = yaw.get_f32("factor") {
                    p.stability_yaw_factor = v;
                }
                if let Some(v) = yaw.get_f32("rate") {
                    p.stability_yaw_rate = v;
                }
            }
            if let Some(roll) = stab.get_child("roll") {
                if let Some(v) = roll.get_f32("factor") {
                    p.stability_roll_factor = v;
                }
                if let Some(v) = roll.get_f32("rate") {
                    p.stability_roll_rate = v;
                }
            }
        }

        if let Some(v) = root.get_f32("steer_rot") {
            p.steer_rot = v;
        }

        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tree_values() {
        let text = r#"
steer_rot = 0
leanspeed = 2
pitchspeed = 5
pitchspring
{
  seatforward = 0.1
  seatbackward = 0.2
  standingforward = 0.5
  standingbackward = 0.5
  damping = 0.4
}
rotationspring = 2
longspring = 9000
springsfactor = 3
longdamping = 0.6
legs
{
	SeatSpring = 200
	StandSpring = 400
	CompDamping = 1
	ExtDamping = 2.5
}
lean = 20
sitting
{
	lean = 30
	pitch
	{
		Forward = 20
		Backward = 10
		MaxAngleForward = 40
		MaxAngleBackward = 60
		Rate = 900
	}
}
standing
{
	pos = 0.1
	pitch
	{
		Forward = 30
		Backward = 10
		MaxAngleForward = 30
		MaxAngleBackward = 50
		Rate = 900
	}
}
"#;
        let root = ConfigNode::parse(text).unwrap();
        let p = RiderPrm::from_tree(&root);
        assert_eq!(p.steer_rot, 0.0);
        assert_eq!(p.lean_speed, 2.0);
        assert_eq!(p.pitch_speed, 5.0);
        assert_eq!(p.pitch_spring_seat_forward, 0.1);
        assert_eq!(p.pitch_spring_seat_backward, 0.2);
        assert_eq!(p.pitch_spring_damping, 0.4);
        assert_eq!(p.rotation_spring, 2.0);
        assert_eq!(p.long_spring, 9000.0);
        assert_eq!(p.springs_factor, 3.0);
        assert_eq!(p.long_damping, 0.6);
        assert_eq!(p.legs_seat_spring, 200.0);
        assert_eq!(p.legs_stand_spring, 400.0);
        assert_eq!(p.legs_comp_damping, 1.0);
        assert_eq!(p.legs_ext_damping, 2.5);
        assert_eq!(p.lean, 20.0);
        assert_eq!(p.sitting_lean, 30.0);
        assert_eq!(p.standing_pos, 0.1);
        assert_eq!(p.sitting_pitch_forward, 20.0);
        assert_eq!(p.standing_pitch_forward, 30.0);
    }
}

