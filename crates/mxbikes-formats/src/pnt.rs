//! PiBoSo `.pnt` paint files (bike, rider suit, gloves, helmet, goggles and boots liveries).
//!
//!   "PNT\0" name[100] count:u32 { ShortImage }*count
//!
//! Each image record (see `texture.rs`) carries the name of the model diffuse texture it
//! replaces, for example `rider`, `gloves` or `goggles`. Name fields are NUL terminated and the
//! bytes after the terminator are unused.

use crate::reader::{ReadError, Reader};
use crate::texture::{read_short_image, ImageRecord};

#[derive(Debug, Clone)]
pub struct PaintFile {
    /// Display name shown in the game's paint selector.
    pub name: String,
    pub textures: Vec<ImageRecord>,
}

impl PaintFile {
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let mut r = Reader::new(data);
        let magic: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        if &magic != b"PNT\0" {
            return Err(ReadError::Magic {
                expected: *b"PNT\0",
                found: magic,
            });
        }
        let name = r.name(100)?;
        let count = r.bounded_count("paint texture count", 64)?;
        let textures = (0..count).map(|_| read_short_image(&mut r)).collect::<Result<Vec<_>, _>>()?;
        if r.remaining() != 0 {
            return Err(ReadError::Implausible {
                what: "trailing paint bytes",
                value: r.remaining() as u64,
                offset: r.pos(),
            });
        }
        Ok(Self { name, textures })
    }

    /// The replacement for a model diffuse texture, matched by name without case.
    pub fn texture(&self, name: &str) -> Option<&ImageRecord> {
        self.textures.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }
}
