//! The testing session choice: track and category (`testsetup.mnu` / `idd_testing_setup`,
//! `FUN_140119970`), track picture (`idd_track_image`), conditions (`idd_testing_set`,
//! `FUN_1401191e0`) and the bike chooser (`idd_bikechoose`).
//!
//! Lists are the game's data: track names from each track's `.ini` `[info] name` filtered by the
//! chosen category ("--------" for all), bikes by their `.ini` `[info] name`, paints by the name
//! stored in each `.pnt` with "-Base-" first, rider gear by the folders of `rider.pkz`.
//! Conditions use the game's ranges and names: temperature 10..38 C, `CC_Clear` / `CC_Cloudy` /
//! `CC_Rainy`, compass names, deformation slider 0..100 for 0..1x and 100..200 for 1..20x.
//!
//! INFERRED: a track's category is its folder under `tracks/`; Start relaunches the program with
//! the chosen track, bike and paints (the game loads the session in place).

use crate::ui::ItemValue;
use crate::world::{LaunchOptions, Vfs};
use bevy::prelude::*;
use mxbikes_formats::{assets, PaintFile, StringTable};
use mxbikes_vfs::MxBikesVfs;

#[derive(Debug, Clone)]
pub struct TrackEntry {
    pub stem: String,
    pub name: String,
    pub category: String,
    pub picture: Option<String>,
}

#[derive(Resource, Debug, Clone)]
pub struct SessionChoice {
    pub tracks: Vec<TrackEntry>,
    pub categories: Vec<String>,
    pub category: Option<String>,
    pub track: usize,
    /// (id, display name, category) of the bikes.
    pub bikes: Vec<(String, String, String)>,
    /// (series name, bike category) of the classes in `series/*/*/<name>.ini` that have bikes.
    pub classes: Vec<(String, String)>,
    pub bike: usize,
    /// Paint file stem per part (`bike`, `rider`, `helmet`, `boots`) or none for -Base-.
    pub paints: std::collections::HashMap<String, Option<String>>,
    pub realistic: bool,
    pub temperature: i64,
    pub conditions: usize,
    pub wind_direction: usize,
    pub wind_speed: i64,
    pub deformation: f32,
    pub crash_mode: bool,
}

fn read_ini(vfs: &MxBikesVfs, path: &str) -> Option<mxbikes_formats::ConfigNode> {
    assets::read_cfg(vfs, path)
}

impl SessionChoice {
    pub fn load(vfs: &MxBikesVfs, options: &LaunchOptions) -> Self {
        let mut tracks: Vec<TrackEntry> = assets::track_paths(vfs)
            .into_iter()
            .filter(|stem| vfs.exists(&format!("{stem}.ini")))
            .map(|stem| {
                let ini = read_ini(vfs, &format!("{stem}.ini")).unwrap_or_default();
                let folder = stem.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default();
                TrackEntry {
                    name: ini.get_child("info").and_then(|i| i.get_str("name")).unwrap_or(&stem).to_string(),
                    category: stem.split('/').nth(1).unwrap_or("").to_string(),
                    picture: ini.get_child("ui").and_then(|u| u.get_str("pic")).map(|p| format!("{folder}/{p}")),
                    stem,
                }
            })
            .collect();
        tracks.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        let mut categories: Vec<String> = tracks.iter().map(|t| t.category.clone()).collect();
        categories.sort();
        categories.dedup();
        let current = assets::find_track(vfs, &options.track).unwrap_or_default();
        let track = tracks.iter().position(|t| t.stem == current).unwrap_or(0);
        let bikes: Vec<(String, String, String)> = assets::bike_ids(vfs)
            .into_iter()
            .map(|id| {
                let ini = read_ini(vfs, &format!("bikes/{id}/{id}.ini")).unwrap_or_default();
                let name = ini.get_child("info").and_then(|n| n.get_str("name")).map(str::to_string).unwrap_or_else(|| id.clone());
                let cat = ini.get_child("data").and_then(|n| n.get_str("cat")).unwrap_or("").to_string();
                (id, name, cat)
            })
            .collect();
        // Classes: series with at least one bike of their category (`FUN_140091570`).
        let mut classes: Vec<(String, String)> = vfs
            .list_files("series/")
            .into_iter()
            .filter(|p| p.ends_with(".ini"))
            .filter_map(|p| {
                let ini = read_ini(vfs, &p)?;
                let name = ini.get_child("info")?.get_str("name")?.to_string();
                let cat = ini.get_child("data")?.get_str("cat")?.to_string();
                bikes.iter().any(|b| b.2.eq_ignore_ascii_case(&cat)).then_some((name, cat))
            })
            .collect();
        classes.sort();
        let bike = bikes.iter().position(|b| b.0 == options.bike).unwrap_or(0);
        let paints = options.paints.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect();
        Self {
            tracks,
            categories,
            category: None,
            track,
            bikes,
            classes,
            bike,
            paints,
            realistic: true,
            temperature: 20,
            conditions: 0,
            wind_direction: 0,
            wind_speed: 0,
            deformation: 1.0,
            crash_mode: true,
        }
    }

    fn visible_tracks(&self) -> Vec<usize> {
        (0..self.tracks.len()).filter(|&i| self.category.as_ref().is_none_or(|c| self.tracks[i].category == *c)).collect()
    }

    /// Paint files (stem, display name) of a part (`paints`, or `goggles` / `gloves` folders).
    fn paint_list(vfs: &MxBikesVfs, dir: &str) -> Vec<(String, String)> {
        let folder = if dir.ends_with("/goggles") || dir.ends_with("/gloves") { dir.to_string() } else { format!("{dir}/paints") };
        let mut out: Vec<(String, String)> = vfs
            .list_files(&format!("{folder}/"))
            .into_iter()
            .filter(|p| p.ends_with(".pnt"))
            .filter_map(|p| {
                let stem = p.rsplit('/').next()?.strip_suffix(".pnt")?.to_string();
                let name = vfs.read(&p).ok().and_then(|d| PaintFile::parse(&d).ok()).map(|f| f.name).unwrap_or_else(|| stem.clone());
                Some((stem, name))
            })
            .collect();
        out.sort_by(|a, b| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase()));
        out
    }

    fn part_dir(&self, part: &str) -> String {
        match part {
            "bike" => format!("bikes/{}", self.bikes.get(self.bike).map(|b| b.0.as_str()).unwrap_or("")),
            "rider" => "rider/riders/default_mx".into(),
            "helmet" => "rider/helmets/default".into(),
            "boots" => "rider/boots/default".into(),
            "protection" => "rider/protections/full".into(),
            _ => String::new(),
        }
    }

    fn folders(vfs: &MxBikesVfs, dir: &str) -> Vec<String> {
        let mut out: Vec<String> = vfs
            .list_files(&format!("{dir}/"))
            .into_iter()
            .filter_map(|p| p.strip_prefix(&format!("{dir}/")).and_then(|r| r.split('/').next()).map(str::to_string))
            .filter(|s| !s.is_empty() && !s.contains('.'))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    pub fn item(&self, id: &str, vfs: &MxBikesVfs, strings: &StringTable) -> Option<ItemValue> {
        let t = |k: &str| strings.get(k).unwrap_or(k).to_string();
        let text = |s: String| Some(ItemValue { text: Some(s), ..Default::default() });
        let pull = |entries: Vec<String>, selected: String| Some(ItemValue { entries, selected: Some(selected), ..Default::default() });
        let paint_pull = |part: &str| {
            let list = Self::paint_list(vfs, &self.part_dir(part));
            let mut entries = vec!["-Base-".to_string()];
            entries.extend(list.iter().map(|p| p.1.clone()));
            let sel = self
                .paints
                .get(part)
                .cloned()
                .flatten()
                .and_then(|s| list.iter().find(|p| p.0 == s).map(|p| p.1.clone()))
                .unwrap_or_else(|| "-Base-".into());
            pull(entries, sel)
        };
        let folder_pull = |dir: &str, current: &str| pull(Self::folders(vfs, dir), current.to_string());
        let track = self.tracks.get(self.track);
        match id {
            "id_changetrack" => pull(self.visible_tracks().iter().map(|&i| self.tracks[i].name.clone()).collect(), track.map(|t| t.name.clone()).unwrap_or_default()),
            "id_category" => {
                let mut entries = vec!["--------".to_string()];
                entries.extend(self.categories.iter().cloned());
                pull(entries, self.category.clone().unwrap_or_else(|| "--------".into()))
            }
            "id_track_pic" => Some(ItemValue { image: track.and_then(|t| t.picture.clone()), ..Default::default() }),
            "id_track" => text(track.map(|t| t.name.clone()).unwrap_or_default()),
            "id_realistic" => Some(ItemValue { checked: Some(self.realistic), ..Default::default() }),
            "id_crashmode" => Some(ItemValue { checked: Some(self.crash_mode), ..Default::default() }),
            "id_temperature" => text(format!("{} C", self.temperature)),
            "id_conditions" => text(t(["CC_Clear", "CC_Cloudy", "CC_Rainy"][self.conditions.min(2)])),
            "id_winddirection" => text(t(["CC_North", "CC_Northeast", "CC_East", "CC_Southeast", "CC_South", "CC_Southwest", "CC_West", "CC_Northwest"][self.wind_direction % 8])),
            "id_windspeed" => text(format!("{} km/h", self.wind_speed)),
            "id_terraindeformation" => {
                let pos = if self.deformation >= 1.0 { 100 + (((self.deformation - 1.0) / 19.0) * 100.0) as i64 } else { (self.deformation * 100.0) as i64 };
                Some(ItemValue { slider: Some((pos.min(200), 200)), ..Default::default() })
            }
            "id_terraindeformation_label" => text(if self.deformation >= 1.0 { format!("{:.1}x", self.deformation) } else { format!("{:.2}x", self.deformation) }),
            // Bike chooser.
            "id_curbike" => pull(self.bikes.iter().map(|b| b.1.clone()).collect(), self.bikes.get(self.bike).map(|b| b.1.clone()).unwrap_or_default()),
            "id_curpaint" => paint_pull("bike"),
            "id_cursuitpaint" => paint_pull("rider"),
            "id_curhelmetpaint" => paint_pull("helmet"),
            "id_curbootspaint" => paint_pull("boots"),
            "id_curgogglespaint" => {
                let list = Self::paint_list(vfs, "rider/helmets/default/goggles");
                let mut entries = vec!["-Base-".to_string()];
                entries.extend(list.iter().map(|p| p.1.clone()));
                pull(entries, "-Base-".into())
            }
            "id_curglovespaint" => {
                let list = Self::paint_list(vfs, "rider/riders/default_mx/gloves");
                let mut entries = vec!["-Base-".to_string()];
                entries.extend(list.iter().map(|p| p.1.clone()));
                pull(entries, "-Base-".into())
            }
            "id_curprotectionpaint" => paint_pull("protection"),
            "id_curclass" => {
                let cat = self.bikes.get(self.bike).map(|b| b.2.clone()).unwrap_or_default();
                let sel = self.classes.iter().find(|c| c.1.eq_ignore_ascii_case(&cat)).map(|c| c.0.clone()).unwrap_or_default();
                pull(self.classes.iter().map(|c| c.0.clone()).collect(), sel)
            }
            "id_curtyres" => {
                // The tyre set of the bike's tyre type (`[data] type` of the set's .ini).
                let id = self.bikes.get(self.bike).map(|b| b.0.clone()).unwrap_or_default();
                let kind = read_ini(vfs, &format!("bikes/{id}/{id}.ini"))
                    .and_then(|i| i.get_child("data").and_then(|d| d.get_str("tyres_type")).map(str::to_string))
                    .unwrap_or_default();
                let name = assets::tyre_set_for(vfs, &kind)
                    .and_then(|set| read_ini(vfs, &format!("tyres/{set}/{set}.ini")))
                    .and_then(|i| i.get_child("info").and_then(|n| n.get_str("name")).map(str::to_string))
                    .unwrap_or_default();
                pull(vec![name.clone()], name)
            }
            "id_currider" => folder_pull("rider/riders", "default_mx"),
            "id_curhelmet" => folder_pull("rider/helmets", "default"),
            "id_curboots" => folder_pull("rider/boots", "default"),
            "id_curprotection" => folder_pull("rider/protections", "full"),
            "id_curhelmetcam" => folder_pull("rider/helmetcams", "default"),
            "id_curbikefont" | "id_cursuitfont" => folder_pull("rider/fonts", "default_black"),
            "id_curridingstyle" => folder_pull("rider/animations", "mx"),
            _ => None,
        }
    }

    pub fn spin(&mut self, id: &str, dir: i64) -> bool {
        match id {
            "id_temperature" => self.temperature = (self.temperature + dir).clamp(10, 38),
            "id_conditions" => self.conditions = (self.conditions as i64 + dir).clamp(0, 2) as usize,
            "id_winddirection" => self.wind_direction = (self.wind_direction as i64 + dir).rem_euclid(8) as usize,
            "id_windspeed" => self.wind_speed = (self.wind_speed + dir).clamp(0, 50),
            _ => return false,
        }
        true
    }

    pub fn toggle(&mut self, id: &str) -> bool {
        match id {
            "id_realistic" => self.realistic = !self.realistic,
            "id_crashmode" => self.crash_mode = !self.crash_mode,
            _ => return false,
        }
        true
    }

    pub fn slide(&mut self, id: &str, pos: i64) -> bool {
        if id != "id_terraindeformation" {
            return false;
        }
        self.deformation = if pos <= 100 { pos as f32 / 100.0 } else { (pos - 100) as f32 / 100.0 * 19.0 + 1.0 };
        true
    }

    pub fn pull_select(&mut self, id: &str, index: usize, vfs: &MxBikesVfs) -> bool {
        match id {
            "id_changetrack" => {
                if let Some(&i) = self.visible_tracks().get(index) {
                    self.track = i;
                }
            }
            "id_category" => {
                self.category = if index == 0 { None } else { self.categories.get(index - 1).cloned() };
                if let Some(c) = &self.category {
                    if self.tracks.get(self.track).is_none_or(|t| t.category != *c) {
                        if let Some(&i) = self.visible_tracks().first() {
                            self.track = i;
                        }
                    }
                }
            }
            "id_curbike" => {
                self.bike = index.min(self.bikes.len().saturating_sub(1));
                self.paints.remove("bike");
            }
            "id_curclass" => {
                // Picking a class picks its first bike.
                if let Some((_, cat)) = self.classes.get(index) {
                    if let Some(i) = self.bikes.iter().position(|b| b.2.eq_ignore_ascii_case(cat)) {
                        self.bike = i;
                        self.paints.remove("bike");
                    }
                }
            }
            "id_curpaint" | "id_cursuitpaint" | "id_curhelmetpaint" | "id_curbootspaint" => {
                let part = match id {
                    "id_curpaint" => "bike",
                    "id_cursuitpaint" => "rider",
                    "id_curhelmetpaint" => "helmet",
                    _ => "boots",
                };
                let list = Self::paint_list(vfs, &self.part_dir(part));
                let stem = if index == 0 { None } else { list.get(index - 1).map(|p| p.0.clone()) };
                self.paints.insert(part.to_string(), stem);
            }
            _ => return false,
        }
        true
    }

    /// Returns the session parameters (track stem/folder, bike ID, paints) for in-place reload.
    pub fn session_params(&self) -> Option<(String, String, std::collections::HashMap<String, String>)> {
        let track = self.tracks.get(self.track)?;
        let bike = self.bikes.get(self.bike)?;
        let folder = track.stem.rsplit('/').next().unwrap_or("").to_string();
        let mut paints = std::collections::HashMap::new();
        for (part, paint) in &self.paints {
            if let Some(p) = paint {
                paints.insert(part.clone(), p.clone());
            }
        }
        Some((folder, bike.0.clone(), paints))
    }

    /// Starts the session: relaunches with the chosen track, bike and paints.
    pub fn start(&self) {
        let (Some(track), Some(bike)) = (self.tracks.get(self.track), self.bikes.get(self.bike)) else { return };
        let Ok(exe) = std::env::current_exe() else { return };
        let mut args: Vec<String> = Vec::new();
        let mut it = std::env::args().skip(1);
        // Keep the program options that are not part of the session choice.
        while let Some(a) = it.next() {
            match a.as_str() {
                "--track" | "--bike" | "--paint" | "--menu" => {
                    it.next();
                }
                _ => args.push(a),
            }
        }
        let folder = track.stem.rsplit('/').next().unwrap_or("").to_string();
        args.extend(["--track".into(), folder, "--bike".into(), bike.0.clone(), "--menu".into(), "pit".into()]);
        for (part, paint) in &self.paints {
            if let Some(p) = paint {
                args.extend(["--paint".into(), format!("{part}={p}")]);
            }
        }
        match std::process::Command::new(exe).args(&args).spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => warn!("could not start the session: {e}"),
        }
    }
}

pub fn setup_session(mut commands: Commands, vfs: Res<Vfs>, options: Res<LaunchOptions>) {
    commands.insert_resource(SessionChoice::load(&vfs.0, &options));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_params_resolution() {
        let mut paints = std::collections::HashMap::new();
        paints.insert("bike".to_string(), Some("custom_livery".to_string()));
        paints.insert("rider".to_string(), None);

        let choice = SessionChoice {
            tracks: vec![TrackEntry {
                stem: "tracks/motocross/club".to_string(),
                name: "Club".to_string(),
                category: "motocross".to_string(),
                picture: None,
            }],
            categories: vec!["motocross".to_string()],
            category: None,
            track: 0,
            bikes: vec![("mxb_250xf".to_string(), "250 XF".to_string(), "mx2".to_string())],
            classes: vec![],
            bike: 0,
            paints,
            realistic: true,
            temperature: 20,
            conditions: 0,
            wind_direction: 0,
            wind_speed: 0,
            deformation: 1.0,
            crash_mode: true,
        };

        let params = choice.session_params();
        assert!(params.is_some());
        let (track, bike, active_paints) = params.unwrap();
        assert_eq!(track, "club");
        assert_eq!(bike, "mxb_250xf");
        assert_eq!(active_paints.get("bike"), Some(&"custom_livery".to_string()));
        assert!(!active_paints.contains_key("rider"));
    }
}

