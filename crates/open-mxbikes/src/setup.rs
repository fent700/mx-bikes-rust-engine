//! Garage setup: the bike's adjustable settings as the game's garage pages show and change them.
//!
//! The values live where the game reads them: `range = min, step, max` + `setting = n` blocks
//! and `*Setting` indices into tables in the bike `.cfg`, and the `pressure` block of the tyre
//! parameter files. Changing an item rewrites that `setting`, so rebuilding the bike from these
//! nodes applies it, as the game applies the setup when the garage closes (`FUN_14009c330`).
//!
//! Texts and formats are the game's (`FUN_140098260` general, `FUN_14009a1f0` / `FUN_140099750`
//! suspensions, `FUN_14009ad40` / `FUN_14009b2b0` drivetrain, `FUN_14009b9a0` others); spin
//! steps follow the page handlers (`FUN_1400a2150`, `FUN_1400a48f0`, `FUN_1400a5d90`,
//! `FUN_1400a6320`): the left arrow lowers the stored index and the right arrow raises it, except
//! for damper clicks, which count the other way.
//!
//! Units are metric (profile `units` = 0, the game's default); the imperial formats are kept
//! next to them for when profiles are read.

use mxbikes_formats::{ConfigNode, ConfigValue, StpFile, STP_VERSION_116};

/// Sag display mode for the garage suspensions page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SagMode {
    #[default]
    Race,
    Static,
}

/// A garage page dialog in `garage.mnu`, picked per bike as `FUN_1400a6680` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    General,
    Suspensions,
    Drivetrain,
    Others,
}

#[derive(Debug, Clone)]
pub struct TyreChoice {
    /// `refname` (falling back to `id`) of the compound in the tyre set's `.cfg`.
    pub name: String,
    pub params: ConfigNode,
    pub sets: u32,
}

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct BikeSetup {
    pub bike_id: String,
    pub base_cfg: ConfigNode,
    pub base_geom: ConfigNode,
    pub cfg: ConfigNode,
    /// The bike's `.geom` (fork offset base, rod length, swingarm positions).
    pub geom: ConfigNode,
    /// Compounds per wheel (0 front, 1 rear) and the chosen compound / tyre set.
    pub tyres: [Vec<TyreChoice>; 2],
    pub compound: [usize; 2],
    pub tyre_set: [usize; 2],
    /// Profile units: 0 metric, 1 imperial, 2 metric with psi (`DAT_140e557d4`).
    pub units: u32,
    /// Strings for `cc_click` / `cc_clicks`.
    pub click: String,
    pub clicks: String,
    pub cur_setup_name: String,
    pub notes: String,
    pub sag_mode: SagMode,
}

/// `range = min, step, max` with the current `setting` index.
#[derive(Debug, Clone, Copy)]
struct Range {
    min: f32,
    step: f32,
    max_index: i64,
    setting: i64,
}

impl Range {
    fn value(&self, index: i64) -> f32 {
        self.min + index as f32 * self.step
    }
}

fn floats(v: Option<&ConfigValue>) -> Vec<f32> {
    match v {
        Some(ConfigValue::Float(f)) => vec![*f],
        Some(ConfigValue::Int(i)) => vec![*i as f32],
        Some(ConfigValue::Vec2(v)) => vec![v.x, v.y],
        Some(ConfigValue::Vec3(v)) => vec![v.x, v.y, v.z],
        Some(ConfigValue::List(l)) => l.iter().filter_map(|s| s.trim().parse().ok()).collect(),
        Some(ConfigValue::String(s)) => s.split(',').filter_map(|p| p.trim().parse().ok()).collect(),
        None => Vec::new(),
    }
}

fn range_of(node: Option<&ConfigNode>, range_key: &str, setting_key: &str) -> Option<Range> {
    let node = node?;
    let r = floats(node.get_value(range_key));
    if r.len() < 3 || r[1] == 0.0 {
        return None;
    }
    let max_index = ((r[2] - r[0]) / r[1] + 0.5).floor().max(0.0) as i64;
    let setting = node.get_i64(setting_key).or_else(|| node.get_f32(setting_key).map(|f| f as i64)).unwrap_or(0);
    Some(Range { min: r[0], step: r[1], max_index, setting: setting.clamp(0, max_index) })
}

/// Number of consecutive `<prefix>0..N` entries in a block.
fn table_len(node: Option<&ConfigNode>, prefix: &str) -> i64 {
    let Some(node) = node else { return 0 };
    (0..).take_while(|i| node.get_value(&format!("{prefix}{i}")).is_some()).count() as i64
}

/// Number of consecutive `<prefix>0..N` child blocks.
fn block_len(node: Option<&ConfigNode>, prefix: &str) -> i64 {
    let Some(node) = node else { return 0 };
    (0..).take_while(|i| node.get_child(&format!("{prefix}{i}")).is_some()).count() as i64
}

fn int(node: Option<&ConfigNode>, key: &str) -> i64 {
    node.and_then(|n| n.get_i64(key).or_else(|| n.get_f32(key).map(|f| f as i64))).unwrap_or(0)
}

fn set_int(node: Option<&mut ConfigNode>, key: &str, value: i64) {
    if let Some(n) = node {
        n.set(key, ConfigValue::Int(value));
    }
}

/// Quarter turns as the game prints them (`FUN_140099630`): "2", "1-1/2", "3/4".
fn turns(quarters: i64) -> String {
    let (whole, rest) = (quarters / 4, quarters % 4);
    let frac = match rest {
        0 => None,
        2 => Some(format!("{}/2", rest / 2)),
        _ => Some(format!("{rest}/4")),
    };
    match (whole, frac) {
        (w, None) => format!("{w}"),
        (0, Some(f)) => f,
        (w, Some(f)) => format!("{w}-{f}"),
    }
}

impl BikeSetup {
    pub fn new(bike_id: String, cfg: ConfigNode, geom: ConfigNode, tyres: [Vec<TyreChoice>; 2], click: String, clicks: String) -> Self {
        let base_cfg = cfg.clone();
        let base_geom = geom.clone();
        Self {
            bike_id,
            base_cfg,
            base_geom,
            cfg,
            geom,
            tyres,
            compound: [0, 0],
            tyre_set: [0, 0],
            units: 0,
            click,
            clicks,
            cur_setup_name: "-Default-".into(),
            notes: String::new(),
            sag_mode: SagMode::Race,
        }
    }

    pub fn reset_to_default(&mut self) {
        self.cfg = self.base_cfg.clone();
        self.geom = self.base_geom.clone();
        self.compound = [0, 0];
        self.tyre_set = [0, 0];
        self.cur_setup_name = "-Default-".into();
        self.notes.clear();
    }

    pub fn to_stp(&self) -> StpFile {
        let num_gears = self.gear_count() as usize;
        let mut gears = Vec::with_capacity(num_gears);
        let gb = self.cfg.get_child("gearbox");
        for i in 0..num_gears {
            gears.push(int(gb, &format!("gear{i}")) as u32);
        }

        let front_leverage = int(self.cfg.path("wheel0/brake"), "leveragesetting") as u32;
        let front_master = int(self.cfg.path("wheel0/brake"), "mastercylindersetting") as u32;
        let front_disc = int(self.cfg.path("wheel0/brake"), "discsetting") as u32;
        let rear_master = int(self.cfg.path("wheel1/brake"), "mastercylindersetting") as u32;
        let rear_disc = int(self.cfg.path("wheel1/brake"), "discsetting") as u32;
        let engine_mapping = int(self.cfg.get_child("engine"), "mappingsetting") as u32;

        let suspensions = [
            int(self.cfg.path("front_suspension/spring"), "setting") as u32,
            int(self.cfg.path("front_suspension/preload"), "setting") as u32,
            int(self.cfg.path("front_suspension/oil"), "setting") as u32,
            int(self.cfg.path("front_suspension/rideheight"), "setting") as u32,
            int(self.cfg.path("front_suspension/damper"), "bumpsetting") as u32,
            int(self.cfg.path("front_suspension/damper"), "reboundsetting") as u32,
            int(self.cfg.path("rear_suspension/spring"), "setting") as u32,
            int(self.cfg.path("rear_suspension/preload"), "setting") as u32,
            int(self.cfg.path("rear_suspension/rodlength"), "setting") as u32,
            int(self.cfg.path("rear_suspension/damper"), "bumpsetting") as u32,
            int(self.cfg.path("rear_suspension/damper"), "fastbumpsetting") as u32,
            int(self.cfg.path("rear_suspension/damper"), "reboundsetting") as u32,
            int(self.cfg.get_child("rear_suspension"), "lengthsetting") as u32,
            int(self.cfg.path("steer/forkoffset"), "setting") as u32,
        ];

        let p_front = self.pressure(0).map(|r| r.setting as u32).unwrap_or(0);
        let p_rear = self.pressure(1).map(|r| r.setting as u32).unwrap_or(0);
        let fuel = int(self.cfg.get_child("fueltank"), "capacitysetting") as u32;
        let r_sprocket = int(self.cfg.get_child("driveline"), "rsprocketsetting") as u32;
        let f_sprocket = int(self.cfg.get_child("driveline"), "fsprocketsetting") as u32;

        StpFile {
            version: STP_VERSION_116,
            bike_id: self.bike_id.clone(),
            bike_subversion: 1,
            tyre_front: self.compound[0] as u32,
            tyre_rear: self.compound[1] as u32,
            pressure_front: p_front,
            pressure_rear: p_rear,
            fuel,
            suspensions,
            final_drive: r_sprocket,
            gears,
            secondary_driveline: [f_sprocket, 0],
            tyre_params: [
                self.tyre_set[0] as u32,
                self.tyre_set[1] as u32,
                front_leverage,
                front_master,
                front_disc,
                engine_mapping,
            ],
            chassis_params: [rear_master, rear_disc, 0],
            geometry_params: [0; 13],
            notes: self.notes.clone(),
        }
    }

    pub fn load_stp(&mut self, stp: &StpFile) {
        if (stp.tyre_front as usize) < self.tyres[0].len() {
            self.compound[0] = stp.tyre_front as usize;
        }
        if (stp.tyre_rear as usize) < self.tyres[1].len() {
            self.compound[1] = stp.tyre_rear as usize;
        }

        if let Some(t) = self.tyres[0].get_mut(self.compound[0]) {
            set_int(t.params.get_child_mut("pressure"), "setting", stp.pressure_front as i64);
        }
        if let Some(t) = self.tyres[1].get_mut(self.compound[1]) {
            set_int(t.params.get_child_mut("pressure"), "setting", stp.pressure_rear as i64);
        }

        set_int(self.cfg.get_child_mut("fueltank"), "capacitysetting", stp.fuel as i64);

        set_int(self.cfg.path_mut("front_suspension/spring"), "setting", stp.suspensions[0] as i64);
        set_int(self.cfg.path_mut("front_suspension/preload"), "setting", stp.suspensions[1] as i64);
        set_int(self.cfg.path_mut("front_suspension/oil"), "setting", stp.suspensions[2] as i64);
        set_int(self.cfg.path_mut("front_suspension/rideheight"), "setting", stp.suspensions[3] as i64);
        set_int(self.cfg.path_mut("front_suspension/damper"), "bumpsetting", stp.suspensions[4] as i64);
        set_int(self.cfg.path_mut("front_suspension/damper"), "reboundsetting", stp.suspensions[5] as i64);

        set_int(self.cfg.path_mut("rear_suspension/spring"), "setting", stp.suspensions[6] as i64);
        set_int(self.cfg.path_mut("rear_suspension/preload"), "setting", stp.suspensions[7] as i64);
        set_int(self.cfg.path_mut("rear_suspension/rodlength"), "setting", stp.suspensions[8] as i64);
        set_int(self.cfg.path_mut("rear_suspension/damper"), "bumpsetting", stp.suspensions[9] as i64);
        set_int(self.cfg.path_mut("rear_suspension/damper"), "fastbumpsetting", stp.suspensions[10] as i64);
        set_int(self.cfg.path_mut("rear_suspension/damper"), "reboundsetting", stp.suspensions[11] as i64);
        set_int(self.cfg.get_child_mut("rear_suspension"), "lengthsetting", stp.suspensions[12] as i64);

        set_int(self.cfg.path_mut("steer/forkoffset"), "setting", stp.suspensions[13] as i64);

        set_int(self.cfg.get_child_mut("driveline"), "rsprocketsetting", stp.final_drive as i64);
        set_int(self.cfg.get_child_mut("driveline"), "fsprocketsetting", stp.secondary_driveline[0] as i64);

        for (i, &g) in stp.gears.iter().enumerate() {
            set_int(self.cfg.get_child_mut("gearbox"), &format!("gear{i}"), g as i64);
        }

        self.tyre_set[0] = stp.tyre_params[0] as usize;
        self.tyre_set[1] = stp.tyre_params[1] as usize;

        set_int(self.cfg.path_mut("wheel0/brake"), "leveragesetting", stp.tyre_params[2] as i64);
        set_int(self.cfg.path_mut("wheel0/brake"), "mastercylindersetting", stp.tyre_params[3] as i64);
        set_int(self.cfg.path_mut("wheel0/brake"), "discsetting", stp.tyre_params[4] as i64);
        set_int(self.cfg.get_child_mut("engine"), "mappingsetting", stp.tyre_params[5] as i64);

        set_int(self.cfg.path_mut("wheel1/brake"), "mastercylindersetting", stp.chassis_params[0] as i64);
        set_int(self.cfg.path_mut("wheel1/brake"), "discsetting", stp.chassis_params[1] as i64);

        self.notes = stp.notes.clone();
    }

    pub fn sag_text(&self, front: bool) -> String {
        let (spring_setting, preload_setting) = if front {
            (
                int(self.cfg.path("front_suspension/spring"), "setting"),
                int(self.cfg.path("front_suspension/preload"), "setting"),
            )
        } else {
            (
                int(self.cfg.path("rear_suspension/spring"), "setting"),
                int(self.cfg.path("rear_suspension/preload"), "setting"),
            )
        };

        let base_sag = match (front, self.sag_mode) {
            (true, SagMode::Static) => 35.0,
            (true, SagMode::Race) => 75.0,
            (false, SagMode::Static) => 38.0,
            (false, SagMode::Race) => 102.0,
        };

        let preload_effect = preload_setting as f32 * if front { 1.0 } else { 2.5 };
        let spring_effect = spring_setting as f32 * if front { 0.8 } else { 1.5 };
        let sag_mm = (base_sag - preload_effect - spring_effect).max(0.0);

        if self.units == 1 {
            format!("{:.2}\"", sag_mm / 25.4)
        } else {
            format!("{sag_mm:.0} mm")
        }
    }

    pub fn list_setup_names(&self) -> Vec<String> {
        let mut names = vec!["-Default-".to_string()];
        let dir = format!("setups/{}", self.bike_id);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut file_names = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("stp") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if stem != "-Default-" {
                            file_names.push(stem.to_string());
                        }
                    }
                }
            }
            file_names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            names.extend(file_names);
        }
        names
    }

    fn geom_vec(&self, key: &str) -> Vec<f32> {
        floats(self.geom.get_value(key))
    }

    pub fn pages(&self) -> [&'static str; 4] {
        [
            if range_of(self.cfg.get_child("fueltank"), "capacityrange", "capacitysetting").is_some() {
                "idd_garage_general"
            } else {
                "idd_garage_general2"
            },
            if table_len(self.cfg.path("rear_suspension/damper/fastbump"), "setting") > 0 {
                "idd_garage_suspensions2"
            } else {
                "idd_garage_suspensions"
            },
            if self.gear_count() > 0 { "idd_garage_drivetrain" } else { "idd_garage_drivetrain2" },
            if block_len(self.cfg.get_child("engine"), "mapping") >= 2 { "idd_garage_others2" } else { "idd_garage_others" },
        ]
    }

    pub fn page_dialog(&self, page: Page) -> &'static str {
        self.pages()[page as usize]
    }

    fn tyre_params(&self, wheel: usize) -> Option<&ConfigNode> {
        self.tyres[wheel].get(self.compound[wheel]).map(|t| &t.params)
    }

    fn pressure(&self, wheel: usize) -> Option<Range> {
        range_of(self.tyre_params(wheel).and_then(|p| p.get_child("pressure")), "range", "setting")
    }

    fn named_options(&self, wheel: usize, block: &str) -> (Vec<String>, i64) {
        let brake = self.cfg.path(&format!("wheel{wheel}/brake"));
        let n = block_len(brake, block);
        let names = (0..n)
            .map(|i| {
                brake
                    .and_then(|b| b.get_child(&format!("{block}{i}")))
                    .and_then(|c| c.get_str("name"))
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        (names, int(brake, &format!("{block}setting")))
    }

    fn gear_count(&self) -> i64 {
        let gb = self.cfg.get_child("gearbox");
        if table_len(gb.and_then(|g| g.get_child("gearing")), "ratio") == 0 {
            return 0;
        }
        int(gb, "numgears")
    }

    fn gearing(&self, index: i64) -> Option<(i64, i64)> {
        let v = floats(self.cfg.path("gearbox/gearing").and_then(|g| g.get_value(&format!("ratio{index}"))));
        (v.len() >= 2).then(|| (v[0] as i64, v[1] as i64))
    }

    fn sprocket(&self, key: &str) -> (Vec<i64>, i64) {
        let d = self.cfg.get_child("driveline");
        let table = d.and_then(|d| d.get_child(key));
        let n = table_len(table, "gear");
        let teeth = (0..n).map(|i| int(table, &format!("gear{i}"))).collect();
        (teeth, int(d, &format!("{key}setting")))
    }

    fn clicks_text(&self, count: i64, setting: i64) -> String {
        let n = count - setting - 1;
        match n {
            0 => "0".into(),
            1 => format!("{n} {}", self.click),
            _ => format!("{n} {}", self.clicks),
        }
    }

    fn damper(&self, path: &str, table: &str) -> (i64, i64) {
        let d = self.cfg.path(path);
        (table_len(d.and_then(|d| d.get_child(table)), "setting"), int(d, &format!("{table}setting")))
    }

    fn spring_text(&self, v: f32) -> String {
        if self.units == 1 {
            format!("{} lb/in", (v / 175.126) as i64)
        } else {
            format!("{:.1} N/mm", v / 1000.0)
        }
    }

    fn mm(v: f32) -> String {
        format!("{} mm", (v * 1000.0).round() as i64)
    }

    /// Top speed (m/s) in a gear: rpm / 60 * primary * gear * final * 2 pi r (`FUN_14009ad40`).
    /// INFERRED: the engine speed is the first limiter when the bike has one, else `MaxRPM`
    /// (the game picks between two engine values on a flag), in revolutions per second.
    fn max_speed(&self, gear_ratio: f32) -> f32 {
        let e = self.cfg.get_child("engine");
        let rpm = if int(e, "numlimiter") > 0 {
            e.and_then(|e| e.get_f32("limiter0")).unwrap_or(0.0)
        } else {
            e.and_then(|e| e.get_f32("maxrpm")).unwrap_or(0.0)
        };
        let primary = floats(e.and_then(|n| n.get_value(&format!("ratio{}", int(e, "ratiosetting")))));
        let primary = if primary.len() >= 2 && primary[1] != 0.0 { primary[0] / primary[1] } else { 1.0 };
        let (f, fs) = self.sprocket("fsprocket");
        let (r, rs) = self.sprocket("rsprocket");
        let final_ratio = match (f.get(fs as usize), r.get(rs as usize)) {
            (Some(&a), Some(&b)) if b != 0 => a as f32 / b as f32,
            _ => 1.0,
        };
        let radius = self.tyre_params(1).and_then(|p| p.get_f32("radius")).unwrap_or(0.335);
        rpm / 60.0 * primary * gear_ratio * final_ratio * radius * std::f32::consts::TAU
    }

    fn speed_text(&self, ms: f32) -> String {
        if (1..=2).contains(&self.units) {
            format!("[{} mph]", (ms * 2.23694) as i64)
        } else {
            format!("[{} km/h]", (ms * 3.6) as i64)
        }
    }

    /// Text of a spin / text item, or `None` when the item is not a setup value.
    pub fn text(&self, id: &str) -> Option<String> {
        let id = id.to_ascii_lowercase();
        if id.ends_with("comp") {
            // Compare column: empty while no compare setup is chosen (`DAT_1404c37c4 == 0`).
            return Some(String::new());
        }
        let c = &self.cfg;
        let fs = "front_suspension";
        let rs = "rear_suspension";
        Some(match id.as_str() {
            "id_fuelfillto" => {
                let r = range_of(c.get_child("fueltank"), "capacityrange", "capacitysetting")?;
                let v = r.value(r.setting);
                if self.units == 1 {
                    format!("{:.2} gal", v * 0.26417)
                } else {
                    format!("{v:.1} L")
                }
            }
            "id_frontpressure" | "id_rearpressure" => {
                let r = self.pressure(if id == "id_frontpressure" { 0 } else { 1 })?;
                let kpa = r.value(r.setting);
                if (1..=2).contains(&self.units) {
                    format!("{:.1} psi", kpa * 0.145038)
                } else {
                    format!("{:.2} bar", kpa / 100.0)
                }
            }
            "id_frontspring" => {
                let r = range_of(c.path(&format!("{fs}/spring")), "range", "setting")?;
                self.spring_text(r.value(r.setting))
            }
            "id_rearspring" => {
                let r = range_of(c.path(&format!("{rs}/spring")), "range", "setting")?;
                self.spring_text(r.value(r.setting))
            }
            "id_frontcompression" => {
                let (n, s) = self.damper(&format!("{fs}/damper"), "bump");
                self.clicks_text(n, s)
            }
            "id_frontrebound" => {
                let (n, s) = self.damper(&format!("{fs}/damper"), "rebound");
                self.clicks_text(n, s)
            }
            "id_rearcompression" | "id_rearslowcompression" => {
                let (n, s) = self.damper(&format!("{rs}/damper"), "bump");
                self.clicks_text(n, s)
            }
            "id_rearfastcompression" => {
                let (n, s) = self.damper(&format!("{rs}/damper"), "fastbump");
                turns(n - s - 1)
            }
            "id_rearrebound" => {
                let (n, s) = self.damper(&format!("{rs}/damper"), "rebound");
                self.clicks_text(n, s)
            }
            "id_frontpreload" => {
                let r = range_of(c.path(&format!("{fs}/preload")), "range", "setting")?;
                Self::mm(r.value(r.setting))
            }
            "id_rearpreload" => {
                let r = range_of(c.path(&format!("{rs}/preload")), "range", "setting")?;
                Self::mm(r.value(r.setting))
            }
            "id_frontoil" => {
                let r = range_of(c.path(&format!("{fs}/oil")), "range", "setting")?;
                Self::mm(r.value(r.setting))
            }
            "id_frontsprocket" | "id_rearsprocket" => {
                let (teeth, s) = self.sprocket(if id == "id_frontsprocket" { "fsprocket" } else { "rsprocket" });
                format!("{}", teeth.get(s as usize)?)
            }
            "id_maxspeed" => {
                // Drivetrain page without selectable gears: top speed in the highest gear.
                let gb = c.get_child("gearbox");
                let top = int(gb, "numgears").max(1) - 1;
                let (a, b) = self.gearing(int(gb, &format!("gear{top}"))).unwrap_or((1, 1));
                self.speed_text(self.max_speed(a as f32 / b.max(1) as f32))
            }
            "id_offset" => {
                // ForkOffset value plus the geometry's offset: front_upper.z - steer_joint.z
                // (setup info +0x30 = bike +0xc8 - +0x90, the .geom block sitting at bike +0x4c).
                let r = range_of(c.path("steer/forkoffset"), "range", "setting")?;
                let (u, j) = (self.geom_vec("front_upper"), self.geom_vec("steer_joint"));
                let base = u.get(2).copied().unwrap_or(0.0) - j.get(2).copied().unwrap_or(0.0);
                Self::mm(r.value(r.setting) + base)
            }
            "id_frontrideheight" => {
                // Shown reversed: (last index - setting) * step (`FUN_14009b9a0`). INFERRED: the
                // added base is taken as 0.
                let r = range_of(c.path(&format!("{fs}/rideheight")), "range", "setting")?;
                Self::mm((r.max_index - r.setting) as f32 * r.step)
            }
            "id_swingarmlength" => format!("{}", int(c.get_child(rs), "lengthsetting")),
            "id_rodlength" => {
                // "-" when the .geom rod_length is 0.01 m or less, else range value + rod_length.
                let base = self.geom.get_f32("rod_length").unwrap_or(0.0);
                match range_of(c.path(&format!("{rs}/rodlength")), "range", "setting") {
                    Some(r) if base > 0.01 => Self::mm(r.value(r.setting) + base),
                    _ => "-".into(),
                }
            }
            "id_frontsag" => self.sag_text(true),
            "id_rearsag" => self.sag_text(false),
            "id_cursetup" => self.cur_setup_name.clone(),
            "id_setupname" => self.cur_setup_name.clone(),
            "id_notes" | "id_edit" => self.notes.clone(),
            _ => {
                if let Some(n) = id.strip_prefix("id_maxspeed").and_then(|n| n.parse::<i64>().ok()) {
                    let gb = c.get_child("gearbox");
                    if n > self.gear_count() {
                        return Some(String::new());
                    }
                    let (a, b) = self.gearing(int(gb, &format!("gear{}", n - 1)))?;
                    return Some(self.speed_text(self.max_speed(a as f32 / b.max(1) as f32)));
                }
                if let Some(n) = id.strip_prefix("id_gear").and_then(|n| n.parse::<i64>().ok()) {
                    if n > self.gear_count() {
                        return Some(String::new());
                    }
                    let (a, b) = self.gearing(int(c.get_child("gearbox"), &format!("gear{}", n - 1)))?;
                    return Some(format!("{a}/{b}"));
                }
                return None;
            }
        })
    }

    /// Items hidden on the drivetrain page past the bike's gear count (`FUN_14009ad40`).
    pub fn hidden(&self, id: &str) -> bool {
        let id = id.to_ascii_lowercase();
        let n = id
            .strip_prefix("id_gear")
            .or_else(|| id.strip_prefix("id_maxspeed"))
            .map(|r| r.trim_end_matches("comp"))
            .and_then(|r| r.parse::<i64>().ok());
        n.is_some_and(|n| n > self.gear_count())
    }

    /// Spin arrow: `dir` -1 for the left button, +1 for the right one.
    pub fn spin(&mut self, id: &str, dir: i64) -> bool {
        let id = id.to_ascii_lowercase();
        let fs = "front_suspension";
        let rs = "rear_suspension";
        let step_range = |cfg: &mut ConfigNode, path: &str, range_key: &str, setting_key: &str| -> bool {
            let Some(r) = range_of(cfg.path(path), range_key, setting_key) else { return false };
            let next = (r.setting + dir).clamp(0, r.max_index);
            set_int(cfg.path_mut(path), setting_key, next);
            next != r.setting
        };
        let step_table = |cfg: &mut ConfigNode, path: &str, table: &str, reversed: bool| -> bool {
            let n = table_len(cfg.path(path).and_then(|d| d.get_child(table)), if table.starts_with("gear") { "gear" } else { "setting" });
            let key = format!("{table}setting");
            let s = int(cfg.path(path), &key);
            let next = (s + if reversed { -dir } else { dir }).clamp(0, (n - 1).max(0));
            set_int(cfg.path_mut(path), &key, next);
            next != s
        };
        match id.as_str() {
            "id_fuelfillto" => step_range(&mut self.cfg, "fueltank", "capacityrange", "capacitysetting"),
            "id_frontpressure" | "id_rearpressure" => {
                let wheel = if id == "id_frontpressure" { 0 } else { 1 };
                let c = self.compound[wheel];
                let Some(t) = self.tyres[wheel].get_mut(c) else { return false };
                step_range(&mut t.params, "pressure", "range", "setting")
            }
            "id_frontspring" => step_range(&mut self.cfg, &format!("{fs}/spring"), "range", "setting"),
            "id_rearspring" => step_range(&mut self.cfg, &format!("{rs}/spring"), "range", "setting"),
            "id_frontpreload" => step_range(&mut self.cfg, &format!("{fs}/preload"), "range", "setting"),
            "id_rearpreload" => step_range(&mut self.cfg, &format!("{rs}/preload"), "range", "setting"),
            "id_frontoil" => step_range(&mut self.cfg, &format!("{fs}/oil"), "range", "setting"),
            "id_offset" => step_range(&mut self.cfg, "steer/forkoffset", "range", "setting"),
            "id_frontrideheight" => step_range(&mut self.cfg, &format!("{fs}/rideheight"), "range", "setting"),
            "id_rodlength" => step_range(&mut self.cfg, &format!("{rs}/rodlength"), "range", "setting"),
            "id_frontcompression" => step_table(&mut self.cfg, &format!("{fs}/damper"), "bump", true),
            "id_frontrebound" => step_table(&mut self.cfg, &format!("{fs}/damper"), "rebound", true),
            "id_rearcompression" | "id_rearslowcompression" => step_table(&mut self.cfg, &format!("{rs}/damper"), "bump", true),
            "id_rearfastcompression" => step_table(&mut self.cfg, &format!("{rs}/damper"), "fastbump", true),
            "id_rearrebound" => step_table(&mut self.cfg, &format!("{rs}/damper"), "rebound", true),
            "id_frontsprocket" => step_table(&mut self.cfg, "driveline", "fsprocket", false),
            "id_rearsprocket" => step_table(&mut self.cfg, "driveline", "rsprocket", false),
            "id_swingarmlength" => {
                // Positions: .geom swingarm_steps + 1 (setup info +0x38 = steps + 1).
                let count = self.geom.get_i64("swingarm_steps").unwrap_or(0) + 1;
                let s = int(self.cfg.get_child(rs), "lengthsetting");
                let next = (s + dir).clamp(0, count - 1);
                set_int(self.cfg.get_child_mut(rs), "lengthsetting", next);
                next != s
            }
            _ => {
                let Some(n) = id.strip_prefix("id_gear").and_then(|n| n.parse::<i64>().ok()) else { return false };
                if n > self.gear_count() {
                    return false;
                }
                let ratios = table_len(self.cfg.path("gearbox/gearing"), "ratio");
                let key = format!("gear{}", n - 1);
                let s = int(self.cfg.get_child("gearbox"), &key);
                let next = (s + dir).clamp(0, (ratios - 1).max(0));
                set_int(self.cfg.get_child_mut("gearbox"), &key, next);
                next != s
            }
        }
    }

    /// Entries and selection of a pull item.
    pub fn pull(&self, id: &str) -> Option<(Vec<String>, String)> {
        let id = id.to_ascii_lowercase();
        let pick = |(names, s): (Vec<String>, i64)| {
            let sel = names.get(s as usize).cloned().unwrap_or_default();
            (names, sel)
        };
        Some(match id.as_str() {
            "id_frontcompound" | "id_rearcompound" => {
                let w = if id == "id_frontcompound" { 0 } else { 1 };
                pick((self.tyres[w].iter().map(|t| t.name.clone()).collect(), self.compound[w] as i64))
            }
            "id_fronttyreset" | "id_reartyreset" | "id_frontcurtyreset" | "id_rearcurtyreset" => {
                // Tyre sets are "<compound> <n>" for n = 1..numsets of the chosen compound
                // (`FUN_140193860`).
                let w = if id.starts_with("id_front") { 0 } else { 1 };
                let t = self.tyres[w].get(self.compound[w])?;
                let names: Vec<String> = (1..=t.sets.max(1)).map(|n| format!("{} {n}", t.name)).collect();
                pick((names, self.tyre_set[w] as i64))
            }
            "id_frontbrakeleverage" => pick(self.named_options(0, "leverage")),
            "id_frontbrakemastercylinder" => pick(self.named_options(0, "mastercylinder")),
            "id_frontbrakedisc" => pick(self.named_options(0, "disc")),
            "id_rearbrakemastercylinder" => pick(self.named_options(1, "mastercylinder")),
            "id_rearbrakedisc" => pick(self.named_options(1, "disc")),
            "id_enginemapping" => {
                let e = self.cfg.get_child("engine");
                let n = block_len(e, "mapping");
                let names = (0..n)
                    .map(|i| e.and_then(|e| e.get_child(&format!("mapping{i}"))).and_then(|m| m.get_str("name")).unwrap_or("").to_string())
                    .collect();
                pick((names, int(e, "mappingsetting")))
            }
            "id_cursetup" => {
                let names = self.list_setup_names();
                let sel = if names.contains(&self.cur_setup_name) {
                    self.cur_setup_name.clone()
                } else {
                    "-Default-".into()
                };
                (names, sel)
            }
            "id_dryqual" | "id_dryrace" | "id_wetqual" | "id_wetrace" => (vec!["-None-".into()], "-None-".into()),
            "id_comparesetup" => (vec!["--------".into(), "-Default-".into()], "--------".into()),
            _ => return None,
        })
    }

    /// Picks entry `index` of a pull item.
    pub fn pull_select(&mut self, id: &str, index: usize) -> bool {
        let id = id.to_ascii_lowercase();
        let brake = |s: &mut Self, wheel: usize, block: &str| {
            set_int(s.cfg.path_mut(&format!("wheel{wheel}/brake")), &format!("{block}setting"), index as i64);
            true
        };
        match id.as_str() {
            "id_frontcompound" | "id_rearcompound" => {
                let w = if id == "id_frontcompound" { 0 } else { 1 };
                if index < self.tyres[w].len() {
                    self.compound[w] = index;
                    self.tyre_set[w] = 0;
                }
                true
            }
            "id_fronttyreset" | "id_frontcurtyreset" => {
                self.tyre_set[0] = index;
                true
            }
            "id_reartyreset" | "id_rearcurtyreset" => {
                self.tyre_set[1] = index;
                true
            }
            "id_frontbrakeleverage" => brake(self, 0, "leverage"),
            "id_frontbrakemastercylinder" => brake(self, 0, "mastercylinder"),
            "id_frontbrakedisc" => brake(self, 0, "disc"),
            "id_rearbrakemastercylinder" => brake(self, 1, "mastercylinder"),
            "id_rearbrakedisc" => brake(self, 1, "disc"),
            "id_enginemapping" => {
                set_int(self.cfg.get_child_mut("engine"), "mappingsetting", index as i64);
                true
            }
            "id_cursetup" => {
                let names = self.list_setup_names();
                if let Some(name) = names.get(index) {
                    if name == "-Default-" {
                        self.reset_to_default();
                    } else {
                        let path = format!("setups/{}/{name}.stp", self.bike_id);
                        if let Ok(bytes) = std::fs::read(&path) {
                            let num_gears = self.gear_count() as usize;
                            if let Ok(stp) = StpFile::parse(&bytes, num_gears) {
                                self.load_stp(&stp);
                                self.cur_setup_name = name.clone();
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIKE: &str = "fueltank\n{\n\tCapacityRange = 1, 0.5, 7.5\n\tCapacitySetting = 13\n}\nfront_suspension\n{\n\tSpring\n\t{\n\t\trange = 4000, 200, 5200\n\t\tsetting = 3\n\t}\n\tDamper\n\t{\n\t\tBump\n\t\t{\n\t\t\tsetting0 = 1, 2, 3\n\t\t\tsetting1 = 1, 2, 3\n\t\t\tsetting2 = 1, 2, 3\n\t\t}\n\t\tBumpSetting = 0\n\t}\n\tRideHeight\n\t{\n\t\trange = -0.02, 0.001, 0.005\n\t\tsetting = 20\n\t}\n}\n";

    fn setup() -> BikeSetup {
        BikeSetup::new("test_bike".into(), ConfigNode::parse(BIKE).unwrap(), ConfigNode::default(), [vec![], vec![]], "click".into(), "clicks".into())
    }

    #[test]
    fn game_formats() {
        let s = setup();
        assert_eq!(s.text("ID_FUELFILLTO").as_deref(), Some("7.5 L"));
        assert_eq!(s.text("id_frontspring").as_deref(), Some("4.6 N/mm"));
        assert_eq!(s.text("id_frontcompression").as_deref(), Some("2 clicks"));
        assert_eq!(s.text("id_frontrideheight").as_deref(), Some("5 mm"));
        assert_eq!(turns(6), "1-1/2");
        assert_eq!(turns(3), "3/4");
        assert_eq!(turns(2), "1/2");
        assert_eq!(turns(8), "2");
    }

    #[test]
    fn spins_clamp_and_clicks_count_down() {
        let mut s = setup();
        assert!(!s.spin("id_fuelfillto", 1));
        assert!(s.spin("id_fuelfillto", -1));
        assert_eq!(s.text("id_fuelfillto").as_deref(), Some("7.0 L"));
        // Left arrow on clicks: one click fewer.
        assert!(s.spin("id_frontcompression", -1));
        assert_eq!(s.text("id_frontcompression").as_deref(), Some("1 click"));
    }

    #[test]
    fn stp_roundtrip_and_sag_calculations() {
        let mut s = setup();
        s.notes = "Test setup notes".into();
        assert_eq!(s.sag_text(true), "73 mm");
        s.sag_mode = SagMode::Static;
        assert_eq!(s.sag_text(true), "33 mm");

        let stp = s.to_stp();
        assert_eq!(stp.bike_id, "test_bike");
        assert_eq!(stp.notes, "Test setup notes");
        assert_eq!(stp.suspensions[0], 3);

        s.reset_to_default();
        assert_eq!(s.cur_setup_name, "-Default-");
        assert_eq!(s.notes, "");

        s.load_stp(&stp);
        assert_eq!(s.notes, "Test setup notes");
    }

    #[test]
    fn setup_discovery_and_selection() {
        let mut s = setup();
        let dir = format!("setups/{}", s.bike_id);
        let _ = std::fs::create_dir_all(&dir);
        let test_path = format!("{}/Mxgp_Dry.stp", dir);
        let stp = s.to_stp();
        let _ = std::fs::write(&test_path, stp.write());

        let names = s.list_setup_names();
        assert!(names.contains(&"-Default-".to_string()));
        assert!(names.contains(&"Mxgp_Dry".to_string()));

        let (pull_names, sel) = s.pull("id_cursetup").unwrap();
        assert_eq!(sel, "-Default-");
        assert!(pull_names.contains(&"Mxgp_Dry".to_string()));

        let idx = pull_names.iter().position(|n| n == "Mxgp_Dry").unwrap();
        assert!(s.pull_select("id_cursetup", idx));
        assert_eq!(s.cur_setup_name, "Mxgp_Dry");

        let _ = std::fs::remove_file(&test_path);
        let _ = std::fs::remove_dir(&dir);
    }
}
