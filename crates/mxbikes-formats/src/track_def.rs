//! Track metadata from `<track>.ini` and timing / start layout from `<track>.rdf`.

use crate::piboso_tree::ConfigNode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingGate {
    pub name: String,
    pub longitudinal_pos: f32,
    pub left_bound: f32,
    pub right_bound: f32,
}

/// A pit or grid stall in track coordinates; `angle_deg` is relative to the centreline heading.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrackStall {
    pub long: f32,
    pub lat: f32,
    pub angle_deg: f32,
}

/// Editor-cached start pose from `pit_lane` (asset frame, degrees). The game itself spawns from
/// `stalls` through the centreline; these fields equal stall 0's world pose on stock tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartGrid {
    pub pos_x: f32,
    pub pos_z: f32,
    pub spacing_x: f32,
    pub spacing_z: f32,
    pub angle_deg: f32,
    pub columns: u32,
    /// `pit_lane/start_stallN` entries, the source the game actually reads.
    pub stalls: Vec<TrackStall>,
}

impl StartGrid {
    /// Asset-frame (x, z) of a start slot.
    pub fn slot(&self, index: u32) -> (f32, f32) {
        let cols = self.columns.max(1);
        let (row, col) = ((index / cols) as f32, (index % cols) as f32);
        (
            self.pos_x + col * self.spacing_x,
            self.pos_z + row * self.spacing_z,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackDefinition {
    pub name: String,
    pub short_name: String,
    pub length: f32,
    pub altitude: f32,
    pub finish_line: Option<TimingGate>,
    pub splits: Vec<TimingGate>,
    pub start: Option<StartGrid>,
}

fn gate(node: &ConfigNode, name: &str) -> TimingGate {
    TimingGate {
        name: name.to_string(),
        longitudinal_pos: node.get_f32("long").unwrap_or(0.0),
        left_bound: node.get_f32("left").unwrap_or(-5.0),
        right_bound: node.get_f32("right").unwrap_or(5.0),
    }
}

impl TrackDefinition {
    pub fn from_nodes(ini: &ConfigNode, rdf: Option<&ConfigNode>) -> Self {
        let info = ini.get_child("info");
        let name = info
            .and_then(|i| i.get_str("name"))
            .unwrap_or("Unknown Track")
            .to_string();
        let short_name = info
            .and_then(|i| i.get_str("short_name"))
            .unwrap_or(&name)
            .to_string();
        let length = info.and_then(|i| i.get_f32("length")).unwrap_or(0.0);
        let altitude = info.and_then(|i| i.get_f32("altitude")).unwrap_or(0.0);

        let finish_line = rdf
            .and_then(|r| r.get_child("finish_line"))
            .map(|n| gate(n, "finish_line"));
        let splits = rdf
            .map(|r| {
                (1..=8)
                    .filter_map(|i| {
                        let key = format!("split{i}");
                        r.get_child(&key).map(|n| gate(n, &key))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let start = rdf.and_then(|r| r.get_child("pit_lane")).map(|p| {
            let stalls = (0..p.get_i64("numstalls").unwrap_or(0))
                .filter_map(|i| p.get_child(&format!("start_stall{i}")))
                .map(|s| TrackStall {
                    long: s.get_f32("long").unwrap_or(0.0),
                    lat: s.get_f32("lat").unwrap_or(0.0),
                    angle_deg: s.get_f32("angle").unwrap_or(0.0),
                })
                .collect();
            StartGrid {
                pos_x: p.get_f32("startposx").unwrap_or(0.0),
                pos_z: p.get_f32("startposz").unwrap_or(0.0),
                spacing_x: p.get_f32("startspacingx").unwrap_or(-1.0),
                spacing_z: p.get_f32("startspacingz").unwrap_or(1.0),
                angle_deg: p.get_f32("startangleabs").unwrap_or(0.0),
                columns: p.get_i64("startcolumns").unwrap_or(1).max(1) as u32,
                stalls,
            }
        });

        Self {
            name,
            short_name,
            length,
            altitude,
            finish_line,
            splits,
            start,
        }
    }
}
