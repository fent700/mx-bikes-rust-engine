//! The rider profile (`profiles/<name>/profile.ini`) with the game's defaults, and the settings
//! pages bound to it.
//!
//! Defaults are the values the profile loader (`FUN_1400ecd00`) sets before reading each key.
//! The settings pages show and change them as the game's page functions do: graphics
//! (`FUN_1400d4580`), misc (`FUN_1400d14e0`) and others / aids (`FUN_1400d2510`); option name
//! tables are the game's (`CC_Low` / `CC_Medium` / `CC_High` for draw distance 500 / 700 / 1000
//! and detail bias 0.4 / 0.2 / 0, `CC_Bilinear` / `CC_Trilinear`, screen ratios, connection
//! speeds, "3D" / "2D" / `cc_both` flags).
//!
//! The profile is kept in memory and saved next to the executable as `profile.ini` (the game's
//! own profile folder under Documents is not written).

use crate::ui::ItemValue;
use bevy::prelude::Resource;
use mxbikes_formats::StringTable;
use std::collections::BTreeMap;

/// (section, key, default) as `FUN_1400ecd00` sets them.
const DEFAULTS: &[(&str, &str, &str)] = &[
    ("info", "race_number", "2"),
    ("input", "gearbox_preload", "0"),
    ("input", "sit_direct", "0"),
    ("input", "combined_brakes", "0"),
    ("input", "combined_brakes_min", "0"),
    ("input", "combined_brakes_max", "1"),
    ("input", "rider_tracking", "0"),
    ("input", "rumble", "1"),
    // The loader falls back to mxbikes.ini [gfx_default], whose stock x / y are 0 (desktop size).
    ("gfx", "x", "0"),
    ("gfx", "y", "0"),
    ("gfx", "bpp", "32"),
    ("gfx", "fullscreen", "1"),
    ("gfx", "refresh", "0"),
    ("gfx", "multisample", "0"),
    ("gfx", "vsync", "0"),
    ("gfx", "display_ratio", "0"),
    ("gfx", "texturesize", "0"),
    ("gfx", "texturefilter", "1"),
    ("gfx", "anisotropic", "3"),
    ("gfx", "drawdistance", "1000"),
    ("gfx", "detail_bias", "0"),
    ("gfx", "terrain_detail_bias", "0"),
    ("gfx", "3d_grass", "1"),
    ("gfx", "marbles_disable", "0"),
    ("gfx", "map_shadow_disable", "0"),
    ("gfx", "shadow_disable", "0"),
    ("gfx", "shaders_disable", "0"),
    ("gfx", "reflection_disable", "0"),
    ("gfx", "screens_disable", "0"),
    ("gfx", "hdr_enable", "0"),
    ("sound", "master_volume", "0.5"),
    ("sound", "ui_volume", "1"),
    ("sound", "music_volume", "1"),
    ("sound", "brakes_volume", "1"),
    ("sound", "aero_volume", "0"),
    ("sound", "remotebikes_volume", "0.75"),
    ("sound", "pit_environment_sounds", "0"),
    ("misc", "units", "0"),
    ("misc", "date", "0"),
    ("misc", "skip_intro", "0"),
    ("misc", "disable_driving_chat", "0"),
    ("misc", "track_help", "2"),
    ("misc", "flags", "0"),
    ("misc", "hide_names", "0"),
    ("misc", "showfps", "0"),
    ("net", "connection", "2"),
    ("view", "fov", "90"),
    ("view", "tilt", "0.5"),
    ("view", "pitch", "0.5"),
    ("view", "mode", "1"),
    ("view", "dynamic_shift", "0"),
    ("view", "corner_anticipation", "0"),
    ("view", "corner_anticipation_scale", "0.5"),
    ("view", "lean_heading_scale", "35"),
    ("view", "free_look", "0"),
    ("view", "free_look_input", "0"),
    ("view", "show_HUD", "0"),
    ("view", "show_riderstand", "0"),
    ("ext_view", "mode", "1"),
    ("ext_view", "dynamic", "1"),
    ("ext_view", "follow_bike", "0"),
    ("ext_view", "corner_anticipation", "0"),
    ("ext_view", "free_look", "1"),
    ("ext_view", "distance", "0"),
    ("ext_view", "height", "0.3"),
    ("ext_view", "latoffset", "0"),
    ("sim", "air_steer_reverse", "1"),
    ("sim", "air_riderlrlean", "0"),
    ("aids", "autoshift", "1"),
    ("aids", "brakehelp", "1"),
    ("aids", "tractionhelp", "0"),
    ("aids", "autoriderfblean", "1"),
    ("aids", "autoriderlrlean", "1"),
    ("aids", "autoriderdab", "1"),
    ("aids", "autoridersit", "1"),
    ("aids", "leanhelp", "3"),
    ("aids", "leanhelp_scale", "0.3"),
    ("aids", "shifthelp", "1"),
    ("aids", "autoclutch", "1"),
    ("aids", "autotyreschange", "1"),
    ("aids", "stabilityhelp", "1"),
    ("aids", "stabilityhelp_scale", "1"),
    ("hardcore", "directsteer", "0"),
    ("testing", "trainer", "1"),
    ("testing", "trainer_lead", "0"),
    ("testing", "groove_multiplier", "1"),
    ("testing", "wet_multiplier", "1"),
    ("testing", "deformation_multiplier", "1"),
    ("testing", "reset_type", "0"),
    ("testing", "reset_time", "0"),
];

#[derive(Resource, Debug, Clone)]
pub struct Profile {
    pub values: BTreeMap<(String, String), String>,
}

impl Default for Profile {
    fn default() -> Self {
        let values = DEFAULTS.iter().map(|(s, k, v)| ((s.to_string(), k.to_ascii_lowercase()), v.to_string())).collect();
        Self { values }
    }
}

const SAVE_PATH: &str = "profile.ini";

impl Profile {
    /// Defaults overlaid with a saved `profile.ini`, if any.
    pub fn load() -> Self {
        let mut p = Self::default();
        if let Ok(text) = std::fs::read_to_string(SAVE_PATH) {
            let mut section = String::new();
            for line in text.lines() {
                let line = line.split(';').next().unwrap_or("").trim();
                if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    section = s.to_ascii_lowercase();
                } else if let Some((k, v)) = line.split_once('=') {
                    p.values.insert((section.clone(), k.trim().to_ascii_lowercase()), v.trim().to_string());
                }
            }
        }
        p
    }

    pub fn save(&self) {
        let mut out = String::new();
        let mut section = "";
        for ((s, k), v) in &self.values {
            if s != section {
                out.push_str(&format!("\n[{s}]\n"));
                section = s;
            }
            out.push_str(&format!("{k}={v}\n"));
        }
        if let Err(e) = std::fs::write(SAVE_PATH, out.trim_start()) {
            bevy::log::warn!("profile.ini: {e}");
        }
    }

    /// Profile name (`info/name`).
    pub fn name(&self) -> String {
        let n = self.str("info", "name");
        if n.is_empty() { "Player".into() } else { n }
    }

    pub fn f(&self, s: &str, k: &str) -> f32 {
        self.values.get(&(s.to_string(), k.to_ascii_lowercase())).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }

    pub fn i(&self, s: &str, k: &str) -> i64 {
        self.f(s, k) as i64
    }

    pub fn str(&self, s: &str, k: &str) -> String {
        self.values.get(&(s.to_string(), k.to_ascii_lowercase())).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, s: &str, k: &str, v: impl ToString) {
        self.values.insert((s.to_string(), k.to_ascii_lowercase()), v.to_string());
    }

    fn flip(&mut self, s: &str, k: &str) {
        let v = self.i(s, k);
        self.set(s, k, i64::from(v == 0));
    }

    /// Value of a settings item, `None` when the item is not a profile setting.
    pub fn item(&self, id: &str, strings: &StringTable, window: (u32, u32)) -> Option<ItemValue> {
        let t = |id: &str| strings.get(id).unwrap_or(id).to_string();
        let lmh = |i: i64| t(["CC_Low", "CC_Medium", "CC_High"][i.clamp(0, 2) as usize]);
        let text = |s: String| Some(ItemValue { text: Some(s), ..Default::default() });
        let check = |b: bool| Some(ItemValue { checked: Some(b), ..Default::default() });
        let slider = |pos: i64, steps: i64| Some(ItemValue { slider: Some((pos.clamp(0, steps), steps)), ..Default::default() });
        let pull = |entries: Vec<String>, sel: usize| {
            let selected = entries.get(sel).cloned();
            Some(ItemValue { entries, selected, ..Default::default() })
        };
        let pct = |v: i64| text(format!("{v}%"));
        let id = id.to_ascii_lowercase();
        match id.as_str() {
            // Graphics.
            "id_resolution" => {
                let (x, y) = (self.i("gfx", "x"), self.i("gfx", "y"));
                // INFERRED: the game lists the display's modes and picks the profile's; the
                // window size stands in when the profile has none.
                let (x, y) = if x > 0 && y > 0 { (x, y) } else { (window.0 as i64, window.1 as i64) };
                text(format!("{x}x{y}"))
            }
            "id_curresolution" => {
                let (w, h) = if window.0 > 0 && window.1 > 0 {
                    (window.0 as i64, window.1 as i64)
                } else {
                    (self.i("gfx", "x"), self.i("gfx", "y"))
                };
                text(format!("{w}x{h}"))
            }
            "id_bpp" => text(format!("{} bits", self.i("gfx", "bpp"))),
            "id_frequency" => match self.i("gfx", "refresh") {
                0 => text(t("CC_default")),
                hz => text(format!("{hz} hz")),
            },
            "id_vsync" => check(self.i("gfx", "vsync") != 0),
            "id_fullscreen" => pull(
                vec![t("cc_fullscreen"), t("cc_borderless"), t("cc_windowed")],
                match self.i("gfx", "fullscreen") {
                    1 => 0,
                    2 => 1,
                    _ => 2,
                },
            ),
            "id_antialias" => slider(log2(self.i("gfx", "multisample")), 4),
            "id_antialias_label" => match log2(self.i("gfx", "multisample")) {
                0 => text("Off".into()),
                p => text(format!("{}x", 1 << p)),
            },
            "id_screenratio" => pull(
                ["Resolution", "4:3", "16:9", "15:9", "16:10", "5:4"].iter().map(|s| s.to_string()).collect(),
                self.i("gfx", "display_ratio").clamp(0, 5) as usize,
            ),
            "id_drawdist" => slider(drawdist_pos(self.f("gfx", "drawdistance")), 2),
            "id_drawdist_label" => text(lmh(drawdist_pos(self.f("gfx", "drawdistance")))),
            // Texture size steps: positions count the table entries 80 / 40 / 0 above the value.
            "id_texturesize" => slider([80, 40, 0].iter().filter(|&&e| self.i("gfx", "texturesize") < e).count() as i64, 2),
            "id_texturesize_label" => text(lmh([80, 40, 0].iter().filter(|&&e| self.i("gfx", "texturesize") < e).count() as i64)),
            "id_texturefilter" => text(t(["CC_Bilinear", "CC_Trilinear"][self.i("gfx", "texturefilter").clamp(0, 1) as usize])),
            // INFERRED: the step count is log2 of the graphics card's maximum anisotropy; 16x
            // (4 steps) is assumed.
            "id_aniso" => slider(self.i("gfx", "anisotropic"), 4),
            "id_aniso_label" => text(format!("{}x", 1 << self.i("gfx", "anisotropic").clamp(0, 4))),
            "id_detailbias" => slider(bias_pos(self.f("gfx", "detail_bias")), 2),
            "id_detailbias_label" => text(lmh(bias_pos(self.f("gfx", "detail_bias")))),
            "id_terraindetailbias" => slider(bias_pos(self.f("gfx", "terrain_detail_bias")), 2),
            "id_terraindetailbias_label" => text(lmh(bias_pos(self.f("gfx", "terrain_detail_bias")))),
            "id_3dgrass" => check(self.i("gfx", "3d_grass") != 0),
            "id_trackscreens" => check(self.i("gfx", "screens_disable") == 0),
            "id_bikesshadow" => check(self.i("gfx", "shadow_disable") == 0),
            "id_shaders" => check(self.i("gfx", "shaders_disable") == 0),
            "id_bikesreflection" => check(self.i("gfx", "reflection_disable") == 0),
            // Misc.
            "id_mastervolume" => slider((self.f("sound", "master_volume") * 100.0).round() as i64, 100),
            "id_mastervolumelabel" => pct((self.f("sound", "master_volume") * 100.0).round() as i64),
            "id_uivolume" => slider((self.f("sound", "ui_volume") * 100.0).round() as i64, 100),
            "id_uivolumelabel" => pct((self.f("sound", "ui_volume") * 100.0).round() as i64),
            "id_brakesvolume" => slider((self.f("sound", "brakes_volume") * 100.0).round() as i64, 100),
            "id_brakesvolumelabel" => pct((self.f("sound", "brakes_volume") * 100.0).round() as i64),
            "id_aerovolume" => slider((self.f("sound", "aero_volume") * 100.0).round() as i64, 100),
            "id_aerovolumelabel" => pct((self.f("sound", "aero_volume") * 100.0).round() as i64),
            "id_remotebikesvolume" => slider((self.f("sound", "remotebikes_volume") * 100.0).round() as i64, 100),
            "id_remotebikesvolumelabel" => pct((self.f("sound", "remotebikes_volume") * 100.0).round() as i64),
            "id_pitenvironmentsounds" => check(self.i("sound", "pit_environment_sounds") != 0),
            "id_unitsmetric" => check(self.i("misc", "units") == 0),
            "id_unitsus" => check(self.i("misc", "units") == 1),
            "id_unitsuk" => check(self.i("misc", "units") == 2),
            "id_date1" => check(self.i("misc", "date") == 0),
            "id_date2" => check(self.i("misc", "date") == 1),
            "id_date3" => check(self.i("misc", "date") == 2),
            "id_conn_type" => text(t(["CC_VeryLow", "CC_Low", "CC_Medium", "CC_High", "CC_VeryHigh"][self.i("net", "connection").clamp(0, 4) as usize])),
            "id_skipintro" => check(self.i("misc", "skip_intro") != 0),
            "id_drivingchat" => check(self.i("misc", "disable_driving_chat") != 0),
            "id_hidenames" => check(self.i("misc", "hide_names") != 0),
            "id_flags" => pull(vec!["3D".into(), "2D".into(), t("cc_both")], self.i("misc", "flags").clamp(0, 2) as usize),
            "id_trackhelp" => text(["Off", "Min", "On"][self.i("misc", "track_help").clamp(0, 2) as usize].into()),
            // Others: view and aids.
            "id_fov" => slider(self.i("view", "fov") - 50, 80),
            "id_fov_label" => text(format!("{}", self.i("view", "fov"))),
            "id_fov_effective" => text(String::new()),
            "id_tilt" => slider((self.f("view", "tilt") * 100.0).round() as i64, 100),
            "id_tilt_label" => pct((self.f("view", "tilt") * 100.0).round() as i64),
            "id_pitch" => slider((self.f("view", "pitch") * 100.0).round() as i64, 100),
            "id_pitch_label" => pct((self.f("view", "pitch") * 100.0).round() as i64),
            "id_freelook" => check(self.i("view", "free_look") != 0),
            "id_mouselook" => check(self.i("view", "free_look_input") == 0),
            "id_tracking" => check(self.i("view", "free_look_input") == 1),
            "id_facetracking" => check(self.i("view", "free_look_input") == 2),
            "id_joystick" => check(self.i("view", "free_look_input") == 3),
            "id_trackir" => check(self.i("view", "free_look_input") == 4),
            "id_corneranticipation" => check(self.i("view", "corner_anticipation") != 0),
            "id_looktoapex" => check(self.i("view", "corner_anticipation") == 1),
            "id_leanheading" => check(self.i("view", "corner_anticipation") == 2),
            "id_corneranticipationscale" => slider((self.f("view", "corner_anticipation_scale") * 100.0).round() as i64, 100),
            "id_corneranticipation_label" => pct((self.f("view", "corner_anticipation_scale") * 100.0).round() as i64),
            "id_leanheadingscale" => slider((self.f("view", "lean_heading_scale") / 40.0 * 100.0).round() as i64, 100),
            "id_leanheading_label" => pct((self.f("view", "lean_heading_scale") / 40.0 * 100.0).round() as i64),
            "id_viewmode" => check(self.i("view", "mode") == 0),
            "id_showhud" => check(self.i("view", "show_hud") != 0),
            "id_showriderstand" => check(self.i("view", "show_riderstand") != 0),
            "id_extviewmode" => check(self.i("ext_view", "mode") == 0),
            "id_extviewdynamic" => check(self.i("ext_view", "dynamic") != 0),
            "id_extviewfollowbike" => check(self.i("ext_view", "follow_bike") != 0),
            // INFERRED: 200 steps (the step count argument is lost in the decompile); the
            // position is distance * 200 + 100, shown as position - 100 percent.
            "id_extviewdist" => slider((self.f("ext_view", "distance") * 200.0).round() as i64 + 100, 200),
            "id_extviewdist_label" => pct((self.f("ext_view", "distance") * 200.0).round() as i64),
            "id_extviewheight" => slider((self.f("ext_view", "height") * 100.0).round() as i64, 100),
            "id_extviewheight_label" => pct((self.f("ext_view", "height") * 100.0).round() as i64),
            "id_extviewlatoff" => slider((self.f("ext_view", "latoffset") / 1.5 * 100.0).round() as i64 + 100, 200),
            "id_extviewlatoff_label" => pct((self.f("ext_view", "latoffset") / 1.5 * 100.0).round() as i64),
            "id_leanhelp" => check(self.i("aids", "leanhelp") >> 1 & 1 != 0),
            "id_autoshift" => check(self.i("aids", "autoshift") != 0),
            // Shift help shows ticked while autoshift is on.
            "id_shifthelp" => check(self.i("aids", "autoshift") != 0 || self.i("aids", "shifthelp") != 0),
            "id_autoclutch" => check(self.i("aids", "autoclutch") != 0),
            "id_brakehelp" => check(self.i("aids", "brakehelp") != 0),
            "id_tractionhelp" => check(self.i("aids", "tractionhelp") != 0),
            "id_autoriderfblean" => check(self.i("aids", "autoriderfblean") != 0),
            "id_autoriderlrlean" => check(self.i("aids", "autoriderlrlean") != 0),
            "id_autoriderdab" => check(self.i("aids", "autoriderdab") != 0),
            "id_autoridersit" => check(self.i("aids", "autoridersit") != 0),
            "id_autotyreschange" => check(self.i("aids", "autotyreschange") != 0),
            // Testing trainer page (`FUN_1400e6550`). INFERRED: 200 lead steps (the step count
            // argument is lost); position (lead + 1) * 100.
            "id_enable" => check(self.i("testing", "trainer") != 0),
            // Profiles page (`FUN_1400b3350`, `FUN_1400b2cb0`). INFERRED: a new profile is named
            // "Player" (the game asks for a name on first start); offline there is no GUID.
            "id_curprofile" => {
                pull(vec![self.name()], 0)
            }
            "id_name" => text(self.name()),
            "id_curracenum" => text(format!("{}", self.i("info", "race_number"))),
            "id_cursuitname" => text(self.str("info", "suit_name")),
            "id_nickname" => text(self.str("profile", "nickname")),
            "id_guid" => text(String::new()),
            "id_language" => pull(vec!["english".into()], 0),
            // Input pages (`FUN_1400cce50`, `FUN_1400cff70`).
            "id_rumble" => check(self.i("input", "rumble") != 0),
            "id_combinedbrakes" => check(self.i("input", "combined_brakes") != 0),
            "id_preload" => check(self.i("input", "gearbox_preload") != 0),
            "id_sitdirect" => check(self.i("input", "sit_direct") != 0),
            "id_lead" => slider(((self.f("testing", "trainer_lead") + 1.0) * 100.0).round() as i64, 200),
            _ => {
                if let Some(n) = id.strip_prefix("id_autochat").and_then(|n| n.parse::<u32>().ok()) {
                    return text(self.str("net", &format!("autochat_{n:02}")));
                }
                None
            }
        }
    }

    /// Check box toggled.
    pub fn toggle(&mut self, id: &str) -> bool {
        let (s, k) = match id.to_ascii_lowercase().as_str() {
            "id_vsync" => ("gfx", "vsync"),
            "id_3dgrass" => ("gfx", "3d_grass"),
            "id_trackscreens" => ("gfx", "screens_disable"),
            "id_bikesshadow" => ("gfx", "shadow_disable"),
            "id_shaders" => ("gfx", "shaders_disable"),
            "id_bikesreflection" => ("gfx", "reflection_disable"),
            "id_pitenvironmentsounds" => ("sound", "pit_environment_sounds"),
            "id_skipintro" => ("misc", "skip_intro"),
            "id_drivingchat" => ("misc", "disable_driving_chat"),
            "id_hidenames" => ("misc", "hide_names"),
            "id_freelook" => ("view", "free_look"),
            "id_corneranticipation" => {
                let v = self.i("view", "corner_anticipation");
                self.set("view", "corner_anticipation", if v == 0 { 1 } else { 0 });
                return true;
            }
            "id_viewmode" => ("view", "mode"),
            "id_showhud" => ("view", "show_hud"),
            "id_showriderstand" => ("view", "show_riderstand"),
            "id_extviewmode" => ("ext_view", "mode"),
            "id_extviewdynamic" => ("ext_view", "dynamic"),
            "id_extviewfollowbike" => ("ext_view", "follow_bike"),
            "id_leanhelp" => {
                let v = self.i("aids", "leanhelp");
                self.set("aids", "leanhelp", v ^ 2);
                return true;
            }
            "id_autoshift" => ("aids", "autoshift"),
            "id_shifthelp" => ("aids", "shifthelp"),
            "id_autoclutch" => ("aids", "autoclutch"),
            "id_brakehelp" => ("aids", "brakehelp"),
            "id_tractionhelp" => ("aids", "tractionhelp"),
            "id_autoriderfblean" => ("aids", "autoriderfblean"),
            "id_autoriderlrlean" => ("aids", "autoriderlrlean"),
            "id_autoriderdab" => ("aids", "autoriderdab"),
            "id_autoridersit" => ("aids", "autoridersit"),
            "id_autotyreschange" => ("aids", "autotyreschange"),
            "id_enable" => ("testing", "trainer"),
            "id_rumble" => ("input", "rumble"),
            "id_combinedbrakes" => ("input", "combined_brakes"),
            "id_preload" => ("input", "gearbox_preload"),
            "id_sitdirect" => ("input", "sit_direct"),
            _ => return false,
        };
        self.flip(s, k);
        true
    }

    /// Radio button picked.
    pub fn radio(&mut self, id: &str) -> bool {
        let (s, k, v) = match id.to_ascii_lowercase().as_str() {
            "id_unitsmetric" => ("misc", "units", 0),
            "id_unitsus" => ("misc", "units", 1),
            "id_unitsuk" => ("misc", "units", 2),
            "id_date1" => ("misc", "date", 0),
            "id_date2" => ("misc", "date", 1),
            "id_date3" => ("misc", "date", 2),
            "id_mouselook" => ("view", "free_look_input", 0),
            "id_tracking" => ("view", "free_look_input", 1),
            "id_facetracking" => ("view", "free_look_input", 2),
            "id_joystick" => ("view", "free_look_input", 3),
            "id_trackir" => ("view", "free_look_input", 4),
            "id_looktoapex" => ("view", "corner_anticipation", 1),
            "id_leanheading" => ("view", "corner_anticipation", 2),
            _ => return false,
        };
        self.set(s, k, v);
        true
    }

    /// Spin arrow.
    pub fn spin(&mut self, id: &str, dir: i64) -> bool {
        const RESOLUTIONS: &[(i64, i64)] = &[
            (800, 600),
            (1024, 768),
            (1280, 720),
            (1280, 800),
            (1280, 1024),
            (1366, 768),
            (1440, 900),
            (1600, 900),
            (1680, 1050),
            (1920, 1080),
            (1920, 1200),
            (2560, 1440),
            (3440, 1440),
            (3840, 2160),
        ];
        const BPPS: &[i64] = &[16, 32];
        const REFRESH_RATES: &[i64] = &[0, 60, 75, 120, 144, 165, 240, 360];

        let id_lc = id.to_ascii_lowercase();
        match id_lc.as_str() {
            "id_resolution" => {
                let cur = (self.i("gfx", "x"), self.i("gfx", "y"));
                let idx = RESOLUTIONS
                    .iter()
                    .position(|&r| r == cur)
                    .unwrap_or_else(|| {
                        RESOLUTIONS
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, &(w, h))| (w - cur.0).abs() + (h - cur.1).abs())
                            .map(|(i, _)| i)
                            .unwrap_or(0)
                    });
                let next_idx = (idx as i64 + dir).clamp(0, RESOLUTIONS.len() as i64 - 1) as usize;
                self.set("gfx", "x", RESOLUTIONS[next_idx].0);
                self.set("gfx", "y", RESOLUTIONS[next_idx].1);
                true
            }
            "id_bpp" => {
                let cur = self.i("gfx", "bpp");
                let idx = BPPS.iter().position(|&b| b == cur).unwrap_or(1);
                let next_idx = (idx as i64 + dir).clamp(0, BPPS.len() as i64 - 1) as usize;
                self.set("gfx", "bpp", BPPS[next_idx]);
                true
            }
            "id_frequency" => {
                let cur = self.i("gfx", "refresh");
                let idx = REFRESH_RATES.iter().position(|&r| r == cur).unwrap_or(0);
                let next_idx = (idx as i64 + dir).clamp(0, REFRESH_RATES.len() as i64 - 1) as usize;
                self.set("gfx", "refresh", REFRESH_RATES[next_idx]);
                true
            }
            "id_conn_type" => {
                let v = (self.i("net", "connection") + dir).clamp(0, 4);
                self.set("net", "connection", v);
                true
            }
            "id_trackhelp" => {
                let v = (self.i("misc", "track_help") + dir).clamp(0, 2);
                self.set("misc", "track_help", v);
                true
            }
            "id_texturefilter" => {
                let v = (self.i("gfx", "texturefilter") + dir).clamp(0, 1);
                self.set("gfx", "texturefilter", v);
                true
            }
            _ => false,
        }
    }

    /// Slider moved to `pos`.
    pub fn slide(&mut self, id: &str, pos: i64) -> bool {
        let p = pos as f32;
        match id.to_ascii_lowercase().as_str() {
            "id_antialias" => self.set("gfx", "multisample", if pos == 0 { 0 } else { 1 << pos }),
            "id_drawdist" => self.set("gfx", "drawdistance", [500, 700, 1000][pos.clamp(0, 2) as usize]),
            "id_texturesize" => self.set("gfx", "texturesize", [80, 40, 0][pos.clamp(0, 2) as usize]),
            "id_aniso" => self.set("gfx", "anisotropic", pos),
            "id_detailbias" => self.set("gfx", "detail_bias", [0.4, 0.2, 0.0][pos.clamp(0, 2) as usize]),
            "id_terraindetailbias" => self.set("gfx", "terrain_detail_bias", [0.4, 0.2, 0.0][pos.clamp(0, 2) as usize]),
            "id_mastervolume" => self.set("sound", "master_volume", p / 100.0),
            "id_uivolume" => self.set("sound", "ui_volume", p / 100.0),
            "id_brakesvolume" => self.set("sound", "brakes_volume", p / 100.0),
            "id_aerovolume" => self.set("sound", "aero_volume", p / 100.0),
            "id_remotebikesvolume" => self.set("sound", "remotebikes_volume", p / 100.0),
            "id_fov" => self.set("view", "fov", pos + 50),
            "id_tilt" => self.set("view", "tilt", p / 100.0),
            "id_pitch" => self.set("view", "pitch", p / 100.0),
            "id_corneranticipationscale" => self.set("view", "corner_anticipation_scale", p / 100.0),
            "id_leanheadingscale" => self.set("view", "lean_heading_scale", p / 100.0 * 40.0),
            "id_extviewdist" => self.set("ext_view", "distance", (p - 100.0) / 200.0),
            "id_extviewheight" => self.set("ext_view", "height", p / 100.0),
            "id_extviewlatoff" => self.set("ext_view", "latoffset", (p - 100.0) / 100.0 * 1.5),
            "id_lead" => self.set("testing", "trainer_lead", p / 100.0 - 1.0),
            _ => return false,
        }
        true
    }

    /// Pull entry picked.
    pub fn pull_select(&mut self, id: &str, index: usize) -> bool {
        match id.to_ascii_lowercase().as_str() {
            "id_fullscreen" => self.set("gfx", "fullscreen", [1, 2, 0][index.min(2)]),
            "id_screenratio" => self.set("gfx", "display_ratio", index.min(5)),
            "id_flags" => self.set("misc", "flags", index.min(2)),
            // Entries are sorted profile names; the name is stored.
            "id_controllerprofile" => return false,
            _ => return false,
        }
        true
    }
}

fn log2(v: i64) -> i64 {
    if v <= 1 {
        0
    } else {
        63 - v.leading_zeros() as i64
    }
}

/// Nearest of the draw distance steps 500 / 700 / 1000 (`FUN_1400d4580`).
fn drawdist_pos(d: f32) -> i64 {
    [500.0, 700.0, 1000.0]
        .iter()
        .enumerate()
        .min_by(|a, b| (d - a.1).abs().total_cmp(&(d - b.1).abs()))
        .map(|(i, _)| i as i64)
        .unwrap_or(2)
}

/// Detail bias position: (bias < 0.4) + (bias < 0.2).
fn bias_pos(b: f32) -> i64 {
    i64::from(b < 0.4) + i64::from(b < 0.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_defaults_and_labels() {
        let p = Profile::default();
        let s = StringTable::default();
        let v = |id: &str| p.item(id, &s, (1600, 900)).unwrap();
        assert_eq!(v("id_drawdist").slider, Some((2, 2)));
        assert_eq!(v("id_aniso_label").text.as_deref(), Some("8x"));
        assert_eq!(v("id_mastervolumelabel").text.as_deref(), Some("50%"));
        assert_eq!(v("id_fov").slider, Some((40, 80)));
        assert_eq!(v("id_autoshift").checked, Some(true));
        assert_eq!(v("id_trackhelp").text.as_deref(), Some("On"));
        assert_eq!(v("id_curresolution").text.as_deref(), Some("1600x900"));
    }

    #[test]
    fn graphics_spin_resolution_and_refresh() {
        let mut p = Profile::default();
        assert_eq!((p.i("gfx", "x"), p.i("gfx", "y")), (800, 600));
        assert!(p.spin("id_resolution", 1));
        assert_eq!((p.i("gfx", "x"), p.i("gfx", "y")), (1024, 768));
        assert!(p.spin("id_resolution", 1));
        assert_eq!((p.i("gfx", "x"), p.i("gfx", "y")), (1280, 720));
        assert!(p.spin("id_bpp", 1));
        assert_eq!(p.i("gfx", "bpp"), 32);
        assert!(p.spin("id_frequency", 1));
        assert_eq!(p.i("gfx", "refresh"), 60);
    }
}
