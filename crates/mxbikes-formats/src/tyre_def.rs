//! `.tyre` definitions: Magic Formula coefficients, structure and per-surface compound data.

use crate::piboso_tree::ConfigNode;
use crate::surface::Surface;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct SurfaceGrip {
    pub dry_grip: f32,
    pub wet_grip: f32,
    pub resistance_dry: f32,
    pub resistance_wet: f32,
    pub compacting_factor: f32,
    pub bulldozing_factor: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TyreDefinition {
    pub id: String,
    /// Magic Formula coefficients keyed by their file names (`p_Dx1`, `q_Bz1`, ...), lowercased.
    pub mf: HashMap<String, f32>,
    pub unloaded_radius: f32,
    pub nominal_load: f32,
    pub radius: f32,
    pub torus_radius: f32,
    pub width_mm: f32,
    pub mass: f32,
    pub inertia: Vec3,
    pub spring_base: f32,
    pub spring_factor: f32,
    pub damper: f32,
    pub optimal_pressure: f32,
    pub pressure_kpa: f32,
    pub knobs_factor: f32,
    pub knobs_max_velocity: f32,
    pub slip_sinkage: f32,
    pub surfaces: HashMap<String, SurfaceGrip>,
}

impl TyreDefinition {
    pub fn coef(&self, name: &str) -> f32 {
        self.mf.get(&name.to_ascii_lowercase()).copied().unwrap_or(0.0)
    }

    pub fn grip(&self, surface: Surface) -> SurfaceGrip {
        self.surfaces
            .get(surface.tyre_key())
            .copied()
            .unwrap_or(SurfaceGrip {
                dry_grip: 1.0,
                wet_grip: 0.9,
                resistance_dry: 1.0,
                resistance_wet: 1.0,
                compacting_factor: 0.0,
                bulldozing_factor: 0.0,
            })
    }

    pub fn from_node(node: &ConfigNode) -> Self {
        let mf = node
            .properties
            .iter()
            .filter(|(k, _)| {
                let b = k.as_bytes();
                b.len() > 2 && b[1] == b'_' && matches!(b[0], b'p' | b'q' | b'r' | b's')
            })
            .filter_map(|(k, v)| v.as_f32().map(|f| (k.clone(), f)))
            .collect();

        let mut surfaces = HashMap::new();
        if let Some(compound) = node.get_child("compound0") {
            for s in Surface::ALL {
                if let Some(b) = compound.get_child(s.tyre_key()) {
                    surfaces.insert(
                        s.tyre_key().to_string(),
                        SurfaceGrip {
                            dry_grip: b.get_f32("dry_grip").unwrap_or(1.0),
                            wet_grip: b.get_f32("wet_grip").unwrap_or(0.9),
                            resistance_dry: b.get_f32("resistance_dry").unwrap_or(1.0),
                            resistance_wet: b.get_f32("resistance_wet").unwrap_or(1.0),
                            compacting_factor: b.get_f32("compacting_factor").unwrap_or(0.0),
                            bulldozing_factor: b.get_f32("bulldozing_factor").unwrap_or(0.0) * 10000.0,
                        },
                    );
                }
            }
        }

        let pressure_kpa = node
            .get_child("pressure")
            .map(crate::bike_def::resolve_setting)
            .unwrap_or_else(|| node.get_f32("optimalpressure").unwrap_or(90.0));

        Self {
            id: node.get_str("id").unwrap_or("").to_string(),
            mf,
            unloaded_radius: node.get_f32("r0").unwrap_or(0.34),
            nominal_load: node.get_f32("fz0").unwrap_or(1000.0),
            radius: node.get_f32("radius").unwrap_or(0.33),
            torus_radius: node.get_f32("torusradius").unwrap_or(0.06),
            width_mm: node.get_f32("width").unwrap_or(75.0),
            mass: node.get_f32("mass").unwrap_or(8.0),
            inertia: node.get_vec3("inertia").unwrap_or(Vec3::new(0.48, 0.25, 0.25)),
            spring_base: node.get_f32("springbase").unwrap_or(60000.0),
            spring_factor: node.get_f32("springfactor").unwrap_or(500.0),
            damper: node.get_f32("damper").unwrap_or(200.0),
            optimal_pressure: node.get_f32("optimalpressure").unwrap_or(90.0),
            pressure_kpa,
            knobs_factor: node.get_f32("knobsfactor").unwrap_or(0.0),
            knobs_max_velocity: node.get_f32("knobsmaxvelocity").unwrap_or(14.0),
            slip_sinkage: node.get_f32("slipsinkage").unwrap_or(1.0),
            surfaces,
        }
    }
}
