//! PiBoSo sound configuration (`sfx.cfg`) and sound curve layouts (`.scl`), evaluated as the
//! game does.
//!
//! `.scl` (loader `FUN_140038130`): `layer<n>/sample<m>` with a wav, volume points
//! (`value`, `volume`) and `minpitch` / `maxpitch`. Evaluation (`FUN_140038910`) at a normalised
//! value v: inside a sample's point range the volume is interpolated between points and the
//! pitch runs linearly from `minpitch` at the first point to `maxpitch` at the last; below the
//! first point only the first sample plays (first point volume, `minpitch`); above the last point
//! only the last sample plays (last point volume, `maxpitch`); otherwise the sample is silent.
//! `RefPitch`, `MaxValue` and `AutoPitch` are not used by this path.
//!
//! Engine mixing (`FUN_140039640`): v = rpm / `max_rpm` of the sfx block; with an off-throttle
//! layer, layer 0 is weighted by w^`blend_exp` and layer 1 by 1 - w^`blend_exp`, where
//! w = (throttle - `IdleThrottle`) / (1 - `IdleThrottle`) (`FUN_140051b90`); both are scaled by
//! (1 - limiter), and the limiter sample plays at limiter * `limiter/volume`.

use crate::piboso_tree::ConfigNode;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CurvePoint {
    pub value: f32,
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundSample {
    pub wav: String,
    pub points: Vec<CurvePoint>,
    pub min_pitch: f32,
    pub max_pitch: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundLayer {
    pub samples: Vec<SoundSample>,
}

/// Volume and pitch of one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleLevel {
    pub layer: usize,
    pub sample: usize,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SoundCurveLayout {
    pub layers: Vec<SoundLayer>,
}

impl SoundCurveLayout {
    pub fn parse(node: &ConfigNode) -> Self {
        let mut layers = Vec::new();
        let mut i = 0;
        while let Some(layer_node) = node.get_child(&format!("layer{i}")) {
            let mut samples = Vec::new();
            let mut s = 0;
            while let Some(sample_node) = layer_node.get_child(&format!("sample{s}")) {
                let mut points = Vec::new();
                let mut p = 0;
                while let Some(point_node) = sample_node.get_child(&format!("point{p}")) {
                    points.push(CurvePoint {
                        value: point_node.get_f32("value").unwrap_or(0.0),
                        volume: point_node.get_f32("volume").unwrap_or(0.0),
                    });
                    p += 1;
                }
                samples.push(SoundSample {
                    wav: sample_node.get_str("wav").unwrap_or("").to_string(),
                    points,
                    min_pitch: sample_node.get_f32("minpitch").unwrap_or(0.0),
                    max_pitch: sample_node.get_f32("maxpitch").unwrap_or(0.0),
                });
                s += 1;
            }
            layers.push(SoundLayer { samples });
            i += 1;
        }
        Self { layers }
    }

    /// Volume and pitch of every sample of `layer` at normalised value `v`, scaled by `gain`.
    pub fn layer_levels(&self, layer: usize, v: f32, gain: f32) -> Vec<SampleLevel> {
        let Some(l) = self.layers.get(layer) else { return Vec::new() };
        let last_sample = l.samples.len().saturating_sub(1);
        l.samples
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (mut volume, mut pitch) = (0.0, 0.0);
                if let (Some(first), Some(last)) = (s.points.first(), s.points.last()) {
                    if v >= first.value && v <= last.value {
                        for w in s.points.windows(2) {
                            if v >= w[0].value && v <= w[1].value {
                                let span = w[1].value - w[0].value;
                                let t = if span > 0.0 { (v - w[0].value) / span } else { 0.0 };
                                volume = w[0].volume + (w[1].volume - w[0].volume) * t;
                                break;
                            }
                        }
                        let span = last.value - first.value;
                        let t = if span > 0.0 { (v - first.value) / span } else { 0.0 };
                        pitch = s.min_pitch + (s.max_pitch - s.min_pitch) * t;
                    } else if v < first.value && i == 0 {
                        volume = first.volume;
                        pitch = s.min_pitch;
                    } else if v > last.value && i == last_sample {
                        volume = last.volume;
                        pitch = s.max_pitch;
                    }
                }
                SampleLevel { layer, sample: i, volume: volume * gain, pitch }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SoundPos {
    pub min_distance: f32,
    /// OpenAL rolloff factor (`rolloff_factor`, default 1).
    pub rolloff: f32,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, Default)]
pub struct EngineSfxConfig {
    pub scl_file: String,
    pub max_rpm: f32,
    pub idle_throttle: f32,
    pub blend_exp: f32,
    pub limiter_wav: Option<String>,
    pub limiter_volume: f32,
    pub place: SoundPos,
}

#[derive(Debug, Clone, Default)]
pub struct BikeSfxConfig {
    pub engine: EngineSfxConfig,
    pub starter_wav: Option<String>,
    pub starter: SoundPos,
    pub gearshift_wav: Option<String>,
    pub gearout_wav: Option<String>,
    pub gearbox: SoundPos,
    /// Transmission whine: scl, max rpm and the torque range that drives its layer blend.
    pub transmission_scl: Option<String>,
    pub transmission_max_rpm: f32,
    pub transmission_torque: (f32, f32),
    pub transmission: SoundPos,
    /// Brakes: scl and the wheel speed (rev/s) that maps to 1.
    pub brakes_scl: Option<String>,
    pub brakes_max_rps: f32,
    pub brakes: SoundPos,
    /// Tyre sound set file under `effects/sounds/sets`.
    pub tyres: Option<String>,
}

/// One tyre sound (`roll` or `slide`) of a surface set (`FUN_14003a6b0`).
#[derive(Debug, Clone)]
pub struct TyreSound {
    pub scl: SoundCurveLayout,
    pub speed_factor: f32,
    pub ref_speed: f32,
    pub pitch_factor: f32,
    pub longslip_factor: f32,
    /// 0 for the roll sound (it has no lateral term).
    pub latslip_factor: f32,
    pub min_distance: f32,
    pub rolloff: f32,
}

impl TyreSound {
    /// Gain, pitch factor and curve value (`FUN_14003a390`): gain atan(v k) / (pi / 2) and pitch
    /// 2^((v - ref) / ref * p) from the speed v; the curve is read at the longitudinal slip measure
    /// for roll, at the combined slip measure for slide (whose speed is the larger of rim and
    /// ground speed). `load` scales the slip measures.
    pub fn levels(&self, rim_speed: f32, ground_speed: f32, slip_long: f32, slip_lat: f32, load: f32) -> (f32, f32, f32) {
        let long = slip_long * load / self.longslip_factor.max(1e-6);
        let (v, value) = if self.latslip_factor > 0.0 {
            let lat = slip_lat * load / self.latslip_factor;
            (rim_speed.abs().max(ground_speed.abs()), (lat * lat + long * long).sqrt())
        } else {
            (rim_speed.abs(), long.abs())
        };
        let gain = (v * self.speed_factor).atan() / std::f32::consts::FRAC_PI_2;
        let pitch = 2f32.powf((v - self.ref_speed) / self.ref_speed.max(1e-6) * self.pitch_factor);
        (gain, pitch, value)
    }
}

/// The game's surface tyre sound sets in load order (`FUN_140047b20`), each dry and wet.
pub const TYRE_SETS: [&str; 6] = ["tire_asphalt", "tire_kerb", "tire_soil", "tire_gravel", "tire_grass", "tire_sand"];

/// Tyre sound set for a terrain surface id (`FUN_140051b90`): asphalt / concrete / paint use the
/// asphalt set, kerb the kerb set, soils the soil set, gravel the gravel set, grass and sand their
/// own; artificial turf (9) has none.
pub fn tyre_set_for_surface(id: u32) -> Option<usize> {
    Some(match id {
        0 | 1 | 2 | 3 | 8 => 0,
        6 => 1,
        7 | 10 | 11 => 2,
        12 => 3,
        4 => 4,
        5 => 5,
        _ => return None,
    })
}

fn place(n: &ConfigNode) -> SoundPos {
    let p = n.get_child("pos");
    let f = |k: &str| p.and_then(|p| p.get_f32(k)).unwrap_or(0.0);
    SoundPos {
        min_distance: n.get_f32("min_distance").unwrap_or(1.0),
        rolloff: n.get_f32("rolloff_factor").unwrap_or(1.0),
        pos: [f("x"), f("y"), f("z")],
    }
}

impl BikeSfxConfig {
    pub fn parse(node: &ConfigNode) -> Self {
        let mut cfg = Self::default();
        if let Some(e) = node.get_child("engine") {
            cfg.engine.scl_file = e.get_str("scl").unwrap_or("engine.scl").to_string();
            cfg.engine.max_rpm = e.get_f32("max_rpm").unwrap_or(14000.0);
            cfg.engine.idle_throttle = e.get_f32("idlethrottle").unwrap_or(0.0);
            cfg.engine.blend_exp = e.get_f32("blend_exp").unwrap_or(1.0);
            cfg.engine.place = place(e);
            if let Some(lim) = e.get_child("limiter") {
                cfg.engine.limiter_wav = lim.get_str("wav").map(|s| s.to_string());
                cfg.engine.limiter_volume = lim.get_f32("volume").unwrap_or(1.0);
            }
        }
        if let Some(s) = node.get_child("starter") {
            cfg.starter_wav = s.get_str("wav").map(|s| s.to_string());
            cfg.starter = place(s);
        }
        if let Some(g) = node.get_child("gearbox") {
            cfg.gearshift_wav = g.get_str("wav").map(|s| s.to_string());
            cfg.gearout_wav = g.get_str("out").map(|s| s.to_string());
            cfg.gearbox = place(g);
        }
        if let Some(t) = node.get_child("transmission") {
            cfg.transmission_scl = t.get_str("scl").map(|s| s.to_string());
            cfg.transmission_max_rpm = t.get_f32("max_rpm").unwrap_or(1.0);
            let tq = t.get_child("torque");
            cfg.transmission_torque = (
                tq.and_then(|q| q.get_f32("min")).unwrap_or(0.0),
                tq.and_then(|q| q.get_f32("max")).unwrap_or(1.0),
            );
            cfg.transmission = place(t);
        }
        if let Some(b) = node.get_child("brakes") {
            cfg.brakes_scl = b.get_str("scl").map(|s| s.to_string());
            cfg.brakes_max_rps = b.get_f32("max_rps").unwrap_or(1.0);
            cfg.brakes = place(b);
        }
        cfg.tyres = node.get_str("tyres").map(|s| s.to_string());
        cfg
    }

    /// Engine layer gains (on, off) and limiter gain for throttle and limiter amount 0..1.
    pub fn engine_gains(&self, throttle: f32, limiter: f32, has_off_layer: bool) -> (f32, f32, f32) {
        let e = &self.engine;
        let w = if e.idle_throttle <= throttle { (throttle - e.idle_throttle) / (1.0 - e.idle_throttle) } else { 0.0 };
        let on = if has_off_layer { w.max(0.0).powf(e.blend_exp) } else { 1.0 };
        let limiter = if limiter < 0.01 { 0.0 } else { limiter };
        ((1.0 - limiter) * on, (1.0 - on) * (1.0 - limiter), limiter * e.limiter_volume)
    }
}

/// OpenAL inverse distance clamped gain (the game plays its sounds through OpenAL Soft).
pub fn distance_gain(distance: f32, min_distance: f32, rolloff: f32) -> f32 {
    let d = distance.max(min_distance);
    min_distance / (min_distance + rolloff * (d - min_distance)).max(1e-6)
}

/// One impact sample within a material set.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactSample {
    pub wav: String,
    pub min_distance: f32,
}

/// Chassis or rider ground scrape sound within a material set.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrapeSound {
    pub wav: String,
    pub min_distance: f32,
    pub volume_factor: f32,
    pub pitch_ref_speed: f32,
    pub pitch_factor: f32,
}

/// Impact and scrape sound set for a given surface/material (e.g. concrete, grass, sand).
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialImpactSet {
    pub name: String,
    pub impacts: Vec<ImpactSample>,
    pub impact_volume_factor: f32,
    pub impact_volume_max: f32,
    pub scrape: Option<ScrapeSound>,
}

/// PiBoSo impact sound configuration (`effects/sounds/sets/impacts.cfg`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImpactsConfig {
    pub sets: HashMap<String, MaterialImpactSet>,
}

impl ImpactsConfig {
    pub fn parse(node: &ConfigNode) -> Self {
        let mut sets = HashMap::new();
        for child in &node.children {
            let name = &child.name;
            let mut impacts = Vec::new();
            let mut i = 0;
            while let Some(imp_node) = child.get_child(&format!("impact{i}")) {
                let wav = imp_node.get_str("wav").unwrap_or("").to_string();
                let min_distance = imp_node.get_f32("min_distance").unwrap_or(3.0);
                impacts.push(ImpactSample { wav, min_distance });
                i += 1;
            }
            let impact_volume_factor = child.get_f32("impact_volumefactor").unwrap_or(0.5);
            let impact_volume_max = child.get_f32("impact_volumemax").unwrap_or(1.0);
            let scrape = child.get_child("scrape").map(|s| ScrapeSound {
                wav: s.get_str("wav").unwrap_or("").to_string(),
                min_distance: s.get_f32("min_distance").unwrap_or(3.0),
                volume_factor: s.get_f32("volumefactor").unwrap_or(0.02),
                pitch_ref_speed: s.get_f32("pitchrefspeed").unwrap_or(30.0),
                pitch_factor: s.get_f32("pitchfactor").unwrap_or(0.3),
            });
            sets.insert(
                name.to_ascii_lowercase(),
                MaterialImpactSet {
                    name: name.clone(),
                    impacts,
                    impact_volume_factor,
                    impact_volume_max,
                    scrape,
                },
            );
        }
        Self { sets }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCL: &str = "Layer0\n{\n\tsample0\n\t{\n\t\twav = idle.wav\n\t\tpoint0\n\t\t{\n\t\t\tValue = 0.0\n\t\t\tVolume = 1.0\n\t\t}\n\t\tpoint1\n\t\t{\n\t\t\tValue = 0.2\n\t\t\tVolume = 1.0\n\t\t}\n\t\tpoint2\n\t\t{\n\t\t\tValue = 0.4\n\t\t\tVolume = 0.0\n\t\t}\n\t\tMinPitch = 0.5\n\t\tMaxPitch = 1.5\n\t}\n\tsample1\n\t{\n\t\twav = high.wav\n\t\tpoint0\n\t\t{\n\t\t\tValue = 0.3\n\t\t\tVolume = 0.0\n\t\t}\n\t\tpoint1\n\t\t{\n\t\t\tValue = 0.8\n\t\t\tVolume = 1.0\n\t\t}\n\t\tMinPitch = 0.6\n\t\tMaxPitch = 1.6\n\t}\n}\n";

    #[test]
    fn sample_levels_follow_the_game() {
        let scl = SoundCurveLayout::parse(&ConfigNode::parse(SCL).unwrap());
        let l = scl.layer_levels(0, 0.1, 1.0);
        assert!((l[0].volume - 1.0).abs() < 1e-6 && (l[0].pitch - 0.75).abs() < 1e-6);
        assert_eq!(l[1].volume, 0.0);
        // Above the last point only the last sample plays, at its last volume and max pitch.
        let l = scl.layer_levels(0, 0.9, 1.0);
        assert_eq!(l[0].volume, 0.0);
        assert!((l[1].volume - 1.0).abs() < 1e-6 && (l[1].pitch - 1.6).abs() < 1e-6);
    }

    #[test]
    fn engine_layer_blend() {
        let mut cfg = BikeSfxConfig::default();
        cfg.engine.idle_throttle = 0.1;
        cfg.engine.blend_exp = 2.0;
        cfg.engine.limiter_volume = 0.8;
        let (on, off, lim) = cfg.engine_gains(0.55, 0.0, true);
        assert!((on - 0.25).abs() < 1e-6 && (off - 0.75).abs() < 1e-6 && lim == 0.0);
        let (on, off, lim) = cfg.engine_gains(1.0, 1.0, true);
        assert!(on == 0.0 && off == 0.0 && (lim - 0.8).abs() < 1e-6);
    }

    #[test]
    fn parse_real_250xf_engine_scl() {
        if let Ok(vfs) = mxbikes_vfs::MxBikesVfs::auto_detect() {
            let bytes = vfs.read("bikes/250xf/engine.scl").expect("read scl");
            let text = String::from_utf8_lossy(&bytes);
            let node = ConfigNode::parse(&text).expect("parse scl node");
            let scl = SoundCurveLayout::parse(&node);
            assert_eq!(scl.layers.len(), 2, "Expected 2 layers in engine.scl");
            assert_eq!(scl.layers[0].samples.len(), 6, "Expected 6 samples in layer 0");
            assert_eq!(scl.layers[1].samples.len(), 6, "Expected 6 samples in layer 1");
            println!("Layer 0 samples: {:?}", scl.layers[0].samples.iter().map(|s| &s.wav).collect::<Vec<_>>());
            // Test level at 5344 rpm:
            let v = 5344.0 / 14000.0;
            let l0 = scl.layer_levels(0, v, 1.0);
            let l1 = scl.layer_levels(1, v, 1.0);
            let sum_vol: f32 = l0.iter().map(|s| s.volume).sum::<f32>() + l1.iter().map(|s| s.volume).sum::<f32>();
            println!("At 5344 rpm, total volume sum: {}", sum_vol);
            assert!(sum_vol > 0.5, "Sound volume was too low at 5344 rpm!");
        }
    }

    #[test]
    fn parse_impacts_cfg_from_vfs() {
        if let Ok(vfs) = mxbikes_vfs::MxBikesVfs::auto_detect() {
            let bytes = vfs.read("effects/sounds/sets/impacts.cfg").expect("read impacts.cfg");
            let text = String::from_utf8_lossy(&bytes);
            let node = ConfigNode::parse(&text).expect("parse impacts node");
            let impacts = ImpactsConfig::parse(&node);
            assert!(impacts.sets.contains_key("concrete"));
            assert!(impacts.sets.contains_key("grass"));
            assert!(impacts.sets.contains_key("sand"));
            assert_eq!(impacts.sets["concrete"].impacts.len(), 4);
            assert!(impacts.sets["concrete"].scrape.is_some());
        }
    }
}
