//! PiBoSo bike setup file (`.stp`) format.
//!
//! Ported directly from the original engine:
//! - Setup saver: `FUN_140199330` (engine op 0x23b)
//! - Setup loader: `FUN_140198e10` (engine op 0x23c)
//! - Setup validator: `FUN_140199e00`
//!
//! Binary layout:
//! - `0x00..0x04`: Magic `0x459353c8` (little-endian u32)
//! - `0x04..0x06`: Version `0x116` or `0x115` (little-endian u16)
//! - `0x06..0x26`: Bike ID / GUID (32 bytes ASCII, null-padded)
//! - `0x26..0x28`: Bike subversion (little-endian u16)
//! - Fixed fields:
//!   - 5 x u32 (`param_2 + 0x00..0x10`: tyre compound/pressures/fuel)
//!   - 14 x u32 (`param_2 + 0x54..0x88`: suspension springs/dampers)
//!   - 1 x u32 (`param_2 + 0x20`: final drive / rear sprocket)
//!   - `num_gears` x u32 (`param_2 + 0x24..`: sequential gear ratios)
//!   - 2 x u32 (`param_2 + 0x4c..0x50`)
//!   - 6 x u32 (`param_2 + 0x8c..0xa0`: tyre compound & pressure settings)
//!   - 3 x u32 (`param_2 + 0x14..0x1c`)
//!   - 13 x u32 (`param_2 + 0xa4..0xd4`: geometry & other parameters)
//! - Notes string:
//!   - 4 bytes: length of notes string
//!   - length bytes: string characters

use thiserror::Error;

pub const STP_MAGIC: u32 = 0x459353c8;
pub const STP_VERSION_116: u16 = 0x116;
pub const STP_VERSION_115: u16 = 0x115;

#[derive(Debug, Error)]
pub enum StpError {
    #[error("File too short for STP header (expected >= 40 bytes, got {0})")]
    TooShort(usize),
    #[error("Invalid STP magic: expected {STP_MAGIC:#x}, got {0:#x}")]
    InvalidMagic(u32),
    #[error("Unsupported STP version: expected 0x115 or 0x116, got {0:#x}")]
    UnsupportedVersion(u16),
    #[error("Truncated data while reading STP file: needed {needed} bytes, had {available}")]
    Truncated { needed: usize, available: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StpFile {
    pub version: u16,
    pub bike_id: String,
    pub bike_subversion: u16,
    pub tyre_front: u32,
    pub tyre_rear: u32,
    pub pressure_front: u32,
    pub pressure_rear: u32,
    pub fuel: u32,
    pub suspensions: [u32; 14],
    pub final_drive: u32,
    pub gears: Vec<u32>,
    pub secondary_driveline: [u32; 2],
    pub tyre_params: [u32; 6],
    pub chassis_params: [u32; 3],
    pub geometry_params: [u32; 13],
    pub notes: String,
}

impl Default for StpFile {
    fn default() -> Self {
        Self {
            version: STP_VERSION_116,
            bike_id: String::new(),
            bike_subversion: 0,
            tyre_front: 0,
            tyre_rear: 0,
            pressure_front: 0,
            pressure_rear: 0,
            fuel: 0,
            suspensions: [0; 14],
            final_drive: 0,
            gears: vec![0; 5],
            secondary_driveline: [0; 2],
            tyre_params: [0; 6],
            chassis_params: [0; 3],
            geometry_params: [0; 13],
            notes: String::new(),
        }
    }
}

impl StpFile {
    /// Parses a binary `.stp` file as decoded in `FUN_140198e10`.
    pub fn parse(data: &[u8], num_gears: usize) -> Result<Self, StpError> {
        if data.len() < 40 {
            return Err(StpError::TooShort(data.len()));
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if magic != STP_MAGIC {
            return Err(StpError::InvalidMagic(magic));
        }

        let version = u16::from_le_bytes(data[4..6].try_into().unwrap());
        if version != STP_VERSION_116 && version != STP_VERSION_115 {
            return Err(StpError::UnsupportedVersion(version));
        }

        let bike_id_bytes = &data[6..38];
        let bike_id = match bike_id_bytes.iter().position(|&b| b == 0) {
            Some(end) => String::from_utf8_lossy(&bike_id_bytes[..end]).to_string(),
            None => String::from_utf8_lossy(bike_id_bytes).to_string(),
        };

        let mut offset = 38;
        let bike_subversion = if version == STP_VERSION_115 {
            0x100
        } else {
            if data.len() < offset + 2 {
                return Err(StpError::Truncated { needed: offset + 2, available: data.len() });
            }
            let sub = u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap());
            offset += 2;
            sub
        };

        let read_u32 = |cur_off: &mut usize| -> Result<u32, StpError> {
            if data.len() < *cur_off + 4 {
                return Err(StpError::Truncated { needed: *cur_off + 4, available: data.len() });
            }
            let val = u32::from_le_bytes(data[*cur_off..*cur_off + 4].try_into().unwrap());
            *cur_off += 4;
            Ok(val)
        };

        let tyre_front = read_u32(&mut offset)?;
        let tyre_rear = read_u32(&mut offset)?;
        let pressure_front = read_u32(&mut offset)?;
        let pressure_rear = read_u32(&mut offset)?;
        let fuel = read_u32(&mut offset)?;

        let mut suspensions = [0u32; 14];
        for s in &mut suspensions {
            *s = read_u32(&mut offset)?;
        }

        let final_drive = read_u32(&mut offset)?;

        let mut gears = Vec::with_capacity(num_gears);
        for _ in 0..num_gears {
            gears.push(read_u32(&mut offset)?);
        }

        let secondary_driveline = [read_u32(&mut offset)?, read_u32(&mut offset)?];

        let mut tyre_params = [0u32; 6];
        for t in &mut tyre_params {
            *t = read_u32(&mut offset)?;
        }

        let chassis_params = [read_u32(&mut offset)?, read_u32(&mut offset)?, read_u32(&mut offset)?];

        let mut geometry_params = [0u32; 13];
        for g in &mut geometry_params {
            *g = read_u32(&mut offset)?;
        }

        let mut notes = String::new();
        if offset + 4 <= data.len() {
            let notes_len = read_u32(&mut offset)? as usize;
            if offset + notes_len <= data.len() {
                let note_bytes = &data[offset..offset + notes_len];
                let end = note_bytes.iter().position(|&b| b == 0).unwrap_or(note_bytes.len());
                notes = String::from_utf8_lossy(&note_bytes[..end]).to_string();
            }
        }

        Ok(Self {
            version,
            bike_id,
            bike_subversion,
            tyre_front,
            tyre_rear,
            pressure_front,
            pressure_rear,
            fuel,
            suspensions,
            final_drive,
            gears,
            secondary_driveline,
            tyre_params,
            chassis_params,
            geometry_params,
            notes,
        })
    }

    /// Serializes to the binary `.stp` layout written by `FUN_140199330`.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.notes.len());
        out.extend_from_slice(&STP_MAGIC.to_le_bytes());
        out.extend_from_slice(&self.version.to_le_bytes());

        // 32-byte bike ID string (null-padded)
        let mut id_buf = [0u8; 32];
        let id_bytes = self.bike_id.as_bytes();
        let copy_len = id_bytes.len().min(31);
        id_buf[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
        out.extend_from_slice(&id_buf);

        out.extend_from_slice(&self.bike_subversion.to_le_bytes());

        out.extend_from_slice(&self.tyre_front.to_le_bytes());
        out.extend_from_slice(&self.tyre_rear.to_le_bytes());
        out.extend_from_slice(&self.pressure_front.to_le_bytes());
        out.extend_from_slice(&self.pressure_rear.to_le_bytes());
        out.extend_from_slice(&self.fuel.to_le_bytes());

        for s in &self.suspensions {
            out.extend_from_slice(&s.to_le_bytes());
        }

        out.extend_from_slice(&self.final_drive.to_le_bytes());

        for g in &self.gears {
            out.extend_from_slice(&g.to_le_bytes());
        }

        for d in &self.secondary_driveline {
            out.extend_from_slice(&d.to_le_bytes());
        }

        for t in &self.tyre_params {
            out.extend_from_slice(&t.to_le_bytes());
        }

        for c in &self.chassis_params {
            out.extend_from_slice(&c.to_le_bytes());
        }

        for g in &self.geometry_params {
            out.extend_from_slice(&g.to_le_bytes());
        }

        let notes_bytes = self.notes.as_bytes();
        let notes_len = (notes_bytes.len() + 1) as u32; // including null terminator as in FUN_140199330
        out.extend_from_slice(&notes_len.to_le_bytes());
        out.extend_from_slice(notes_bytes);
        out.push(0); // null terminator

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stp_file() {
        let stp = StpFile {
            version: STP_VERSION_116,
            bike_id: "mxb_250xf".into(),
            bike_subversion: 1,
            tyre_front: 2,
            tyre_rear: 3,
            pressure_front: 120,
            pressure_rear: 110,
            fuel: 75,
            suspensions: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
            final_drive: 48,
            gears: vec![14, 18, 22, 26, 30],
            secondary_driveline: [100, 200],
            tyre_params: [1, 2, 3, 4, 5, 6],
            chassis_params: [7, 8, 9],
            geometry_params: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
            notes: "Factory race setup dry".into(),
        };

        let bytes = stp.write();
        assert!(bytes.len() > 100);
        let parsed = StpFile::parse(&bytes, 5).expect("parse should succeed");
        assert_eq!(parsed, stp);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut bytes = StpFile::default().write();
        bytes[0] = 0;
        assert!(matches!(StpFile::parse(&bytes, 5), Err(StpError::InvalidMagic(_))));
    }
}
