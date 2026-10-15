//! Ground surface kinds shared by tyre compound tables and terrain layers.
//!
//! The `.tyre` compound blocks name each surface; `.trh` terrain layers reference them by id.
//! Ids 4 (grass), 5 (sand) and 7 (soft soil / mud) are confirmed by every stock track's layer
//! textures. Ids 10–13 only appear on stock dirt/sand tracks and are assigned from their texture
//! sets (`dirt` → soil, `mud_treads` → compact soil, sand tracks → sand); revisit once the
//! decrypted binary can be inspected.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    Asphalt1,
    Asphalt2,
    Asphalt3,
    Concrete,
    Grass,
    Sand,
    Kerb,
    SoftSoil,
    Soil,
    CompactSoil,
    Paint,
    Gravel,
    Rock,
    ArtificialTurf,
}

impl Surface {
    pub const ALL: [Surface; 14] = [
        Surface::Asphalt1,
        Surface::Asphalt2,
        Surface::Asphalt3,
        Surface::Concrete,
        Surface::Grass,
        Surface::Sand,
        Surface::Kerb,
        Surface::SoftSoil,
        Surface::Soil,
        Surface::CompactSoil,
        Surface::Paint,
        Surface::Gravel,
        Surface::Rock,
        Surface::ArtificialTurf,
    ];

    /// Block name used inside `.tyre` compound definitions.
    pub fn tyre_key(self) -> &'static str {
        match self {
            Surface::Asphalt1 => "asphalt1",
            Surface::Asphalt2 => "asphalt2",
            Surface::Asphalt3 => "asphalt3",
            Surface::Concrete => "concrete",
            Surface::Grass => "grass",
            Surface::Sand => "sand",
            Surface::Kerb => "kerb",
            Surface::SoftSoil => "soft soil",
            Surface::Soil => "soil",
            Surface::CompactSoil => "compact soil",
            Surface::Paint => "paint",
            Surface::Gravel => "gravel",
            Surface::Rock => "rock",
            Surface::ArtificialTurf => "artificial turf",
        }
    }

    /// Maps a `.trh` terrain layer surface id.
    /// Terrain surface id (inverse of [`Surface::from_terrain_id`]).
    pub fn terrain_id(self) -> u32 {
        match self {
            Surface::Asphalt1 => 0,
            Surface::Asphalt2 => 1,
            Surface::Asphalt3 => 2,
            Surface::Concrete => 3,
            Surface::Grass => 4,
            Surface::Sand => 5,
            Surface::Kerb => 6,
            Surface::Soil => 7,
            Surface::Paint => 8,
            Surface::ArtificialTurf => 9,
            Surface::SoftSoil => 10,
            Surface::CompactSoil => 11,
            Surface::Gravel => 12,
            Surface::Rock => 13,
        }
    }

    pub fn from_terrain_id(id: u32) -> Surface {
        match id {
            0 => Surface::Asphalt1,
            1 => Surface::Asphalt2,
            2 => Surface::Asphalt3,
            3 => Surface::Concrete,
            4 => Surface::Grass,
            5 => Surface::Sand,
            6 => Surface::Kerb,
            7 => Surface::Soil,
            8 => Surface::Paint,
            9 => Surface::ArtificialTurf,
            10 => Surface::SoftSoil,
            11 => Surface::CompactSoil,
            12 => Surface::Gravel,
            13 => Surface::Rock,
            _ => Surface::Soil,
        }
    }
}
