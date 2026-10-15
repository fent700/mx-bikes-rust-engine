//! Typed bike definition assembled from the stock `.cfg`, `.geom`, `.engn` and `.ini` files.
//!
//! Setup values are resolved exactly as the garage presents them: blocks with
//! `range = min, step, max` + `setting = n` give `min + n * step`, and indexed tables
//! (`setting0..N`, `gear0..N`) are picked by the sibling `*Setting` key.

use crate::piboso_tree::{ConfigNode, ConfigValue};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

const TAU: f32 = std::f32::consts::TAU;

/// Resolves a `range`/`setting` block to its configured value.
pub fn resolve_setting(block: &ConfigNode) -> f32 {
    let range = block.get_vec3("range").unwrap_or(Vec3::ZERO);
    let n = block.get_f32("setting").unwrap_or(0.0);
    (range.x + n * range.y).min(range.z.max(range.x))
}

fn setting_of(parent: &ConfigNode, block: &str, fallback: f32) -> f32 {
    parent.get_child(block).map(resolve_setting).unwrap_or(fallback)
}

/// Picks entry `index_key` (e.g. `BumpSetting`) from a `setting0..N` table.
fn indexed<'a>(table: &'a ConfigNode, prefix: &str, index: i64) -> Option<&'a ConfigValue> {
    table.properties.get(&format!("{prefix}{index}"))
}

fn floats(v: Option<&ConfigValue>) -> Vec<f32> {
    match v {
        Some(ConfigValue::Float(f)) => vec![*f],
        Some(ConfigValue::Int(i)) => vec![*i as f32],
        Some(ConfigValue::Vec2(v)) => vec![v.x, v.y],
        Some(ConfigValue::Vec3(v)) => vec![v.x, v.y, v.z],
        Some(ConfigValue::List(l)) => l.iter().filter_map(|s| s.parse().ok()).collect(),
        Some(ConfigValue::String(s)) => s.split(',').filter_map(|p| p.trim().parse().ok()).collect(),
        None => Vec::new(),
    }
}

fn teeth_ratio(v: Option<Vec2>) -> f32 {
    v.map(|t| if t.x > 0.0 { t.y / t.x } else { 1.0 }).unwrap_or(1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClutchDefinition {
    pub max_torque: f32,
    pub mapping: f32,
    pub dead_zone: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineDefinition {
    pub inertia: f32,
    pub idle_rpm: f32,
    pub idle_throttle: f32,
    pub stall_rpm: f32,
    pub stall_torque: f32,
    pub limiter_rpm: f32,
    pub max_rpm: f32,
    pub shift_rpm: f32,
    pub breaking_rpm: f32,
    /// Crank to gearbox input reduction (output teeth / input teeth).
    pub primary_ratio: f32,
    pub clutch: ClutchDefinition,
    pub starter_torque: f32,
    pub starter_max_rpm: f32,
    /// Full-throttle torque (rpm, Nm), from `.engn` `torque` (x stored in rev/s).
    pub torque_curve: Vec<(f32, f32)>,
    /// Closed-throttle friction/pumping torque (rpm, Nm, negative).
    pub braking_curve: Vec<(f32, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GearboxDefinition {
    /// Gear ratios as output/input speed reduction for gears 1..N.
    pub ratios: Vec<f32>,
    pub upshift_time: f32,
    pub downshift_time: f32,
    pub disengage_time: f32,
    pub engage_time: f32,
    /// Automatic-gearbox assist table from `gearbox { autoshift { gearN { down up } } }`:
    /// (down rpm, up rpm) per gear index, gear 0 = neutral. Missing entries are None.
    pub autoshift: Vec<(Option<f32>, Option<f32>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrivelineDefinition {
    pub first_axis_inertia: f32,
    pub second_axis_inertia: f32,
    pub efficiency: f32,
    pub front_sprocket: u32,
    pub rear_sprocket: u32,
}

impl DrivelineDefinition {
    pub fn final_ratio(&self) -> f32 {
        self.rear_sprocket as f32 / self.front_sprocket.max(1) as f32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrakeDefinition {
    pub wheel_friction: f32,
    pub rider_max_force: f32,
    pub dead_zone: f32,
    pub master_cylinder_diameter: f32,
    pub leverage: f32,
    pub pressure_to_torque: f32,
    pub disc_mass: f32,
}

impl BrakeDefinition {
    /// Maximum brake torque (Nm) at full lever: hydraulic pressure (kPa) times pressure-to-torque.
    pub fn max_torque(&self) -> f32 {
        let area = std::f32::consts::PI * (self.master_cylinder_diameter * 0.5).powi(2);
        let pressure_kpa = self.rider_max_force * self.leverage / area.max(1e-6) / 1000.0;
        pressure_kpa * self.pressure_to_torque
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChassisDefinition {
    pub mass: f32,
    pub inertia: Vec3,
    pub front_lat_stiffness: f32,
    pub front_lat_damping: f32,
    pub rear_lat_stiffness: f32,
    pub rear_lat_damping: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AeroDefinition {
    pub area: Vec3,
    pub cx: f32,
    pub cl: f32,
    pub rot_drag: Vec3,
    pub min_cda: f32,
    pub max_cda: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerDefinition {
    pub mass: f32,
    pub inertia_box: Vec3,
    pub fork_offset_adjust: f32,
    pub lock_deg: f32,
    pub spg: [f32; 2],
    pub sdg: [f32; 2],
    pub sig: [f32; 2],
    pub k_yaw: f32,
    pub k_damping: [f32; 2],
}

/// Speed-sensitive three stage damper: coefficient `c[i]` (N·s/m) applies past `threshold[i-1]` (m/s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamperCurve {
    pub thresholds: [f32; 2],
    pub coefficients: [f32; 3],
}

impl DamperCurve {
    /// Damping force for a stroke speed magnitude (m/s); continuous and piecewise linear.
    pub fn force(&self, speed: f32) -> f32 {
        let v = speed.abs();
        let [t0, t1] = [self.thresholds[0].abs(), self.thresholds[1].abs()];
        let [c0, c1, c2] = self.coefficients;
        let f = if v <= t0 {
            c0 * v
        } else if v <= t1 {
            c0 * t0 + c1 * (v - t0)
        } else {
            c0 * t0 + c1 * (t1 - t0) + c2 * (v - t1)
        };
        f * speed.signum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontSuspensionDefinition {
    pub mass: f32,
    pub spring_rate: f32,
    pub preload: f32,
    pub ride_height: f32,
    pub air_area: f32,
    pub air_length_base: f32,
    pub air_pressure_kpa: f32,
    pub oil_level: f32,
    pub bump: DamperCurve,
    pub rebound: DamperCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BumpStopDefinition {
    pub length: f32,
    pub spring_base: f32,
    pub spring_rising_rate: f32,
    pub damping_base: f32,
    pub damping_rising_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RearSuspensionDefinition {
    pub mass: f32,
    pub spring_rate: f32,
    pub preload: f32,
    pub rod_length_adjust: f32,
    pub bump: DamperCurve,
    pub rebound: DamperCurve,
    pub bump_stop: BumpStopDefinition,
}

/// Hard points from `.geom` (asset frame: +X right, +Y up, +Z forward).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BikeGeometry {
    pub chassis_mass: Vec3,
    pub chassis_steer: Vec3,
    pub swingarm_pivot: Vec3,
    pub fuel: Vec3,
    pub pressure_center: Vec3,
    pub rider_mass: Vec3,
    pub rider: Vec3,
    pub rider_view: Vec3,
    pub front_sprocket: Vec3,
    pub rear_sprocket: f32,
    pub chain_pitch: f32,
    pub rake_deg: f32,
    pub steer_mass: Vec3,
    pub front_upper: Vec3,
    pub front_lower: Vec3,
    pub front_length: f32,
    pub fsusp_mass: Vec3,
    pub fwheel: Vec3,
    pub rsusp_mass: Vec3,
    pub rear_length: f32,
    pub shock_min_length: f32,
    pub shock_chassis: Vec3,
    pub rocker_swingarm: Vec3,
    pub rocker_length: f32,
    pub rod_chassis: Vec3,
    pub rod_length: f32,
    pub shock_rocker: Vec3,
    pub rocker_ref_angle_deg: f32,
    pub rwheel: Vec3,
    pub t_cam: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BikeDefinition {
    pub id: String,
    pub name: String,
    pub category: String,
    pub engine: EngineDefinition,
    pub gearbox: GearboxDefinition,
    pub driveline: DrivelineDefinition,
    pub brakes: [BrakeDefinition; 2],
    pub chassis: ChassisDefinition,
    pub aero: AeroDefinition,
    pub steer: SteerDefinition,
    pub front_suspension: FrontSuspensionDefinition,
    pub rear_suspension: RearSuspensionDefinition,
    pub geometry: BikeGeometry,
    pub tyres_type: String,
    pub autoclutch_rpm: (f32, f32),
}

fn curve(engn: Option<&ConfigNode>, block: &str) -> Vec<(f32, f32)> {
    let Some(b) = engn.and_then(|e| e.get_child(block)) else {
        return Vec::new();
    };
    let n = b.get_i64("numpoints").unwrap_or(0);
    (0..n)
        .filter_map(|i| b.get_vec2(&format!("point{i}")))
        .map(|p| (p.x * 60.0, p.y))
        .collect()
}

fn damper_from(block: &ConfigNode, prefix: &str, table: &str, fallback: [f32; 3]) -> DamperCurve {
    let t0 = block.get_f32(&format!("{prefix}threshold0")).unwrap_or(0.05);
    let t1 = block.get_f32(&format!("{prefix}threshold1")).unwrap_or(1.0);
    let row = block
        .get_child(table)
        .and_then(|t| indexed(t, "setting", block.get_i64(&format!("{table}setting")).unwrap_or(0)))
        .map(|v| floats(Some(v)))
        .unwrap_or_default();
    let mut c = fallback;
    for (i, v) in row.iter().take(3).enumerate() {
        c[i] = *v;
    }
    if row.len() == 1 {
        c[1] = c[0];
        c[2] = c[0];
    }
    DamperCurve {
        thresholds: [t0, t1],
        coefficients: c,
    }
}

fn brake_from(node: Option<&ConfigNode>) -> BrakeDefinition {
    let friction = node.and_then(|n| n.get_f32("friction")).unwrap_or(1.5);
    let b = node.and_then(|n| n.get_child("brake"));
    let pick = |prefix: &str, setting_key: &str| -> Option<&ConfigNode> {
        let b = b?;
        let idx = b.get_i64(setting_key).unwrap_or(0);
        b.get_child(&format!("{prefix}{idx}"))
    };
    BrakeDefinition {
        wheel_friction: friction,
        rider_max_force: b.and_then(|b| b.get_f32("ridermaxforce")).unwrap_or(40.0),
        dead_zone: b.and_then(|b| b.get_f32("deadzone")).unwrap_or(0.01),
        master_cylinder_diameter: pick("mastercylinder", "mastercylindersetting")
            .and_then(|m| m.get_f32("diameter"))
            .unwrap_or(0.013),
        leverage: pick("leverage", "leveragesetting")
            .and_then(|m| m.get_f32("ratio"))
            .unwrap_or(10.0),
        pressure_to_torque: pick("disc", "discsetting")
            .and_then(|m| m.get_f32("pressuretotorque"))
            .unwrap_or(0.14),
        disc_mass: pick("disc", "discsetting")
            .and_then(|m| m.get_f32("mass"))
            .unwrap_or(0.6),
    }
}

impl BikeDefinition {
    pub fn from_nodes(
        cfg: &ConfigNode,
        geom: Option<&ConfigNode>,
        engn: Option<&ConfigNode>,
        ini: Option<&ConfigNode>,
    ) -> Self {
        let empty = ConfigNode::new("");
        let id = cfg.get_str("id").unwrap_or("unknown").to_string();
        let info = ini.and_then(|i| i.get_child("info"));
        let data = ini.and_then(|i| i.get_child("data"));

        let e = cfg.get_child("engine").unwrap_or(&empty);
        let clutch = e.get_child("clutch").unwrap_or(&empty);
        let starter = e.get_child("starter").unwrap_or(&empty);
        let engine = EngineDefinition {
            inertia: e.get_f32("inertia").unwrap_or(0.006),
            idle_rpm: e.get_f32("idlerpm").unwrap_or(2000.0),
            idle_throttle: e.get_f32("idlethrottle").unwrap_or(0.08),
            stall_rpm: e.get_child("stall").and_then(|s| s.get_f32("rpm")).unwrap_or(1500.0),
            stall_torque: e.get_child("stall").and_then(|s| s.get_f32("torque")).unwrap_or(0.03),
            limiter_rpm: e.get_f32("limiter0").unwrap_or(14000.0),
            max_rpm: e.get_f32("maxrpm").unwrap_or(14000.0),
            shift_rpm: e.get_f32("shiftrpm").unwrap_or(13000.0),
            breaking_rpm: e.get_f32("breakingrpm").unwrap_or(17500.0),
            primary_ratio: teeth_ratio(e.get_vec2("ratio0")),
            clutch: ClutchDefinition {
                max_torque: clutch.get_f32("maxtorque").unwrap_or(100.0),
                mapping: clutch.get_f32("mapping").unwrap_or(1.0),
                dead_zone: clutch.get_f32("deadzone").unwrap_or(0.2),
            },
            starter_torque: starter.get_f32("torque").unwrap_or(3.0),
            starter_max_rpm: starter.get_f32("maxrpm").unwrap_or(2000.0),
            torque_curve: curve(engn, "torque"),
            braking_curve: curve(engn, "brake"),
        };

        let gb = cfg.get_child("gearbox").unwrap_or(&empty);
        let gearing = gb.get_child("gearing").unwrap_or(&empty);
        let num_gears = gb.get_i64("numgears").unwrap_or(5);
        let ratios = (0..num_gears)
            .map(|g| {
                let idx = gb.get_i64(&format!("gear{g}")).unwrap_or(g);
                teeth_ratio(gearing.get_vec2(&format!("ratio{idx}")))
            })
            .collect();
        let gearbox = GearboxDefinition {
            ratios,
            upshift_time: gb.get_f32("upshifttime").unwrap_or(0.04),
            downshift_time: gb.get_f32("downshifttime").unwrap_or(0.04),
            disengage_time: gb.get_f32("disengagetime").unwrap_or(0.02),
            engage_time: gb.get_f32("engagetime").unwrap_or(0.02),
            autoshift: gb
                .get_child("autoshift")
                .map(|a| {
                    (0..=num_gears)
                        .map(|g| {
                            let b = a.get_child(&format!("gear{g}"));
                            (b.and_then(|b| b.get_f32("down")), b.and_then(|b| b.get_f32("up")))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };

        let dl = cfg.get_child("driveline").unwrap_or(&empty);
        let sprocket = |block: &str, setting: &str, fallback: u32| -> u32 {
            let idx = dl.get_i64(setting).unwrap_or(0);
            dl.get_child(block)
                .and_then(|b| b.get_f32(&format!("gear{idx}")))
                .map(|t| t as u32)
                .unwrap_or(fallback)
        };
        let driveline = DrivelineDefinition {
            first_axis_inertia: dl.get_f32("firstaxisinertia").unwrap_or(0.0025),
            second_axis_inertia: dl.get_f32("secondaxisinertia").unwrap_or(0.00075),
            efficiency: dl.get_f32("efficiency").unwrap_or(0.92),
            front_sprocket: sprocket("fsprocket", "fsprocketsetting", 13),
            rear_sprocket: sprocket("rsprocket", "rsprocketsetting", 50),
        };

        let ch = cfg.get_child("chassis").unwrap_or(&empty);
        let chassis = ChassisDefinition {
            mass: ch.get_f32("mass").unwrap_or(60.0),
            inertia: ch.get_vec3("inertia").unwrap_or(Vec3::new(4.8, 3.2, 2.2)),
            front_lat_stiffness: ch.get_f32("frontlatstiffness").unwrap_or(750.0),
            front_lat_damping: ch.get_f32("frontlatdamping").unwrap_or(4.0),
            rear_lat_stiffness: ch.get_f32("rearlatstiffness").unwrap_or(500.0),
            rear_lat_damping: ch.get_f32("rearlatdamping").unwrap_or(3.0),
        };

        let ae = cfg.get_child("aero").unwrap_or(&empty);
        let aero = AeroDefinition {
            area: Vec3::new(
                ae.get_f32("areax").unwrap_or(1.3),
                ae.get_f32("areay").unwrap_or(0.75),
                ae.get_f32("areaz").unwrap_or(0.5),
            ),
            cx: ae.get_f32("cx").unwrap_or(0.8),
            cl: ae.get_f32("cl").unwrap_or(0.0),
            rot_drag: Vec3::new(
                ae.get_f32("rotx").unwrap_or(0.08),
                ae.get_f32("roty").unwrap_or(0.16),
                ae.get_f32("rotz").unwrap_or(0.02),
            ),
            min_cda: ae.get_f32("mincda").unwrap_or(0.54),
            max_cda: ae.get_f32("maxcda").unwrap_or(0.61),
        };

        let st = cfg.get_child("steer").unwrap_or(&empty);
        let pair = |a: &str, b: &str, d: [f32; 2]| [st.get_f32(a).unwrap_or(d[0]), st.get_f32(b).unwrap_or(d[1])];
        let steer = SteerDefinition {
            mass: st.get_f32("mass").unwrap_or(10.0),
            inertia_box: st.get_vec3("inertiabox").unwrap_or(Vec3::new(0.3, 0.8, 0.1)),
            fork_offset_adjust: setting_of(st, "forkoffset", 0.0),
            lock_deg: st.get_f32("lock").unwrap_or(42.0),
            spg: pair("spg0", "spg1", [-150.0, -2.5]),
            sdg: pair("sdg0", "sdg1", [-30.0, 0.25]),
            sig: pair("sig0", "sig1", [0.0, 0.0]),
            k_yaw: st.get_f32("kyaw").unwrap_or(10.0),
            k_damping: pair("kdamping0", "kdamping1", [6.0, 0.15]),
        };

        let fs = cfg.get_child("front_suspension").unwrap_or(&empty);
        let fdamp = fs.get_child("damper").unwrap_or(&empty);
        let front_suspension = FrontSuspensionDefinition {
            mass: fs.get_f32("mass").unwrap_or(3.5),
            spring_rate: setting_of(fs, "spring", 4600.0),
            preload: setting_of(fs, "preload", 0.01),
            ride_height: setting_of(fs, "rideheight", 0.0),
            air_area: fs.get_f32("area").unwrap_or(0.0016),
            air_length_base: fs.get_f32("airlengthbase").unwrap_or(-0.05),
            air_pressure_kpa: fs.get_f32("airpressure").unwrap_or(100.0),
            oil_level: setting_of(fs, "oil", 0.1),
            bump: damper_from(fdamp, "bump", "bump", [2000.0, 350.0, 1500.0]),
            rebound: damper_from(fdamp, "rebound", "rebound", [1400.0, 450.0, 1500.0]),
        };

        let rs = cfg.get_child("rear_suspension").unwrap_or(&empty);
        let rdamp = rs.get_child("damper").unwrap_or(&empty);
        let mut rear_bump = damper_from(rdamp, "bump", "bump", [8600.0, 1760.0, 12000.0]);
        if let Some(fast) = rdamp
            .get_child("fastbump")
            .and_then(|t| indexed(t, "setting", rdamp.get_i64("fastbumpsetting").unwrap_or(0)))
        {
            let f = floats(Some(fast));
            if f.len() >= 2 {
                rear_bump.coefficients[1] = f[0];
                rear_bump.coefficients[2] = f[1];
            }
        }
        let bs = rs.get_child("bumpstop").unwrap_or(&empty);
        let rear_suspension = RearSuspensionDefinition {
            mass: rs.get_f32("mass").unwrap_or(9.0),
            spring_rate: setting_of(rs, "spring", 54000.0),
            preload: setting_of(rs, "preload", 0.013),
            rod_length_adjust: setting_of(rs, "rodlength", 0.0),
            bump: rear_bump,
            rebound: damper_from(rdamp, "rebound", "rebound", [5700.0, 4350.0, 12000.0]),
            bump_stop: BumpStopDefinition {
                length: resolve_setting(bs),
                spring_base: bs.get_f32("springbase").unwrap_or(5.0),
                spring_rising_rate: bs.get_f32("springrisingrate").unwrap_or(0.4),
                damping_base: bs.get_f32("dampingbase").unwrap_or(0.5),
                damping_rising_rate: bs.get_f32("dampingrisingrate").unwrap_or(0.04),
            },
        };

        let g = geom.unwrap_or(&empty);
        let v = |k: &str, d: Vec3| g.get_vec3(k).unwrap_or(d);
        let f = |k: &str, d: f32| g.get_f32(k).unwrap_or(d);
        let geometry = BikeGeometry {
            chassis_mass: v("chassis_mass", Vec3::new(0.0, 0.628, -0.027)),
            chassis_steer: v("chassis_steer", Vec3::new(0.0, 0.991, 0.337)),
            swingarm_pivot: v("chassis_rsusp_min", Vec3::new(0.0, 0.47, -0.17)),
            fuel: v("fuel", Vec3::new(0.0, 0.85, 0.07)),
            pressure_center: v("pressure_center", Vec3::new(0.0, 0.9, 0.2)),
            rider_mass: v("rider_mass", Vec3::new(0.0, 1.05, 0.0)),
            rider: v("rider", Vec3::ZERO),
            rider_view: v("rider_view", Vec3::new(0.0, 1.05, -0.2)),
            front_sprocket: v("front_sprocket", Vec3::new(0.0, 0.467, -0.109)),
            rear_sprocket: f("rear_sprocket", -0.074),
            chain_pitch: f("chain_pitch", 0.02),
            rake_deg: f("rakeangle_min", 26.5),
            steer_mass: v("steer_mass", Vec3::new(0.0, -0.117, 0.022)),
            front_upper: v("front_upper", Vec3::new(0.0, -0.082, 0.021)),
            front_lower: v("front_lower", Vec3::new(0.0, 0.078, 0.021)),
            front_length: f("front_length", 0.3),
            fsusp_mass: v("fsusp_mass", Vec3::new(0.0, -0.116, 0.023)),
            fwheel: v("fwheel", Vec3::new(0.0, -0.333, 0.066)),
            rsusp_mass: v("rsusp_mass", Vec3::new(0.0, -0.005, -0.308)),
            rear_length: f("rear_length", 0.12),
            shock_min_length: f("shock_minlength", 0.375),
            shock_chassis: v("shock_chassis", Vec3::new(0.0, 0.785, -0.15)),
            rocker_swingarm: v("rocker_swingarm", Vec3::new(0.0, -0.046, -0.151)),
            rocker_length: f("rocker_length", 0.045),
            rod_chassis: v("rod_chassis", Vec3::new(0.0, 0.376, -0.205)),
            rod_length: f("rod_length", 0.145),
            shock_rocker: v("shock_rocker", Vec3::new(0.0, 0.049, 0.04)),
            rocker_ref_angle_deg: f("ref_angle", -90.0),
            rwheel: v("rwheel_min", Vec3::new(0.0, -0.001, -0.551)),
            t_cam: v("t-cam", Vec3::new(0.0, 1.45, 0.0)),
        };

        let ac = cfg.get_child("autoclutch");
        Self {
            name: info.and_then(|i| i.get_str("name")).unwrap_or(&id).to_string(),
            category: data.and_then(|d| d.get_str("cat")).unwrap_or("").to_string(),
            tyres_type: data
                .and_then(|d| d.get_str("tyres_type"))
                .or_else(|| cfg.get_child("tyres").and_then(|t| t.get_str("type")))
                .unwrap_or("mx")
                .to_string(),
            id,
            engine,
            gearbox,
            driveline,
            brakes: [
                brake_from(cfg.get_child("wheel0")),
                brake_from(cfg.get_child("wheel1")),
            ],
            chassis,
            aero,
            steer,
            front_suspension,
            rear_suspension,
            geometry,
            autoclutch_rpm: (
                ac.and_then(|a| a.get_f32("minrpm")).unwrap_or(2500.0),
                ac.and_then(|a| a.get_f32("maxrpm")).unwrap_or(3000.0),
            ),
        }
    }

    /// Overall reduction from crank to rear wheel in a gear (1-based).
    pub fn overall_ratio(&self, gear: usize) -> f32 {
        let g = self.gearbox.ratios.get(gear.wrapping_sub(1)).copied().unwrap_or(0.0);
        self.engine.primary_ratio * g * self.driveline.final_ratio()
    }

    /// Peak power (kW) and the rpm it occurs at, from the torque curve.
    pub fn peak_power(&self) -> (f32, f32) {
        self.engine
            .torque_curve
            .iter()
            .map(|&(rpm, nm)| (nm * rpm / 60.0 * TAU / 1000.0, rpm))
            .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = r#"
ID = 250xf
engine
{
	Inertia = 0.006
	IdleRPM = 2300
	Limiter0 = 14300
	Ratio0 = 24, 73
	Clutch
	{
		MaxTorque = 105
	}
}
gearbox
{
	gearing
	{
		ratio0 = 13, 32
		ratio1 = 16, 32
	}
	NumGears = 2
	Gear0 = 0
	Gear1 = 1
}
driveline
{
	FSprocket
	{
		gear0 = 11
		gear1 = 12
		gear2 = 13
	}
	FSprocketSetting = 2
	RSprocket
	{
		gear6 = 50
	}
	RSprocketSetting = 6
}
front_suspension
{
	Spring
	{
		range = 4000, 200, 5200
		setting = 3
	}
	Damper
	{
		BumpThreshold0 = -0.05
		BumpThreshold1 = -1.1
		Bump
		{
			setting0 = 1460, 325, 1500
			setting1 = 1560, 330, 1500
		}
		BumpSetting = 1
	}
}
"#;

    #[test]
    fn resolves_setup_values() {
        let cfg = ConfigNode::parse(CFG).unwrap();
        let engn = ConfigNode::parse("torque\n{\n numpoints = 2\n point0 = 100, 20\n point1 = 200, 25\n}\n").unwrap();
        let b = BikeDefinition::from_nodes(&cfg, None, Some(&engn), None);
        assert!((b.engine.primary_ratio - 73.0 / 24.0).abs() < 1e-5);
        assert!((b.gearbox.ratios[0] - 32.0 / 13.0).abs() < 1e-5);
        assert_eq!(b.driveline.front_sprocket, 13);
        assert_eq!(b.driveline.rear_sprocket, 50);
        assert!((b.front_suspension.spring_rate - 4600.0).abs() < 1e-3);
        assert_eq!(b.front_suspension.bump.coefficients, [1560.0, 330.0, 1500.0]);
        assert_eq!(b.engine.torque_curve[0], (6000.0, 20.0));
        assert!((b.engine.clutch.max_torque - 105.0).abs() < 1e-5);
    }

    #[test]
    fn damper_is_continuous() {
        let d = DamperCurve {
            thresholds: [0.05, 1.1],
            coefficients: [2000.0, 350.0, 1500.0],
        };
        let a = d.force(0.05 - 1e-4);
        let b = d.force(0.05 + 1e-4);
        assert!((a - b).abs() < 1.0);
        assert!(d.force(-0.5) < 0.0);
    }
}
