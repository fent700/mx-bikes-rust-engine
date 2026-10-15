//! Menus drawn from the game's own layout files (`ui/*.mnu`, `ui/ui.ui`), strings
//! (`ui/english.str`), bitmap fonts (`ui/*.fnt`) and sprites (`ui/*.tga`).
//!
//! Rects and text positions are screen fractions of the 16:9 layout `ui.ui` declares. Every item
//! type of the menu loader (`FUN_140203450`) is drawn: darkbox, bitmap, text, button, tab, spin,
//! slider, checkbox, radio, editbox, list and pull. Item values come from the screen's data
//! (the garage from [`crate::setup::BikeSetup`]), as the game's dialog functions fill them through
//! the engine's item operations (set text 0x112, pull entries 0x12a / select 299, check 0x10f,
//! slider position 0x12f, hide 0x10c).
//!
//! INFERRED (the engine's item drawing is not decoded): pull lists open below the item with
//! `visiblerows` rows of `spacing` height; the slider knob (`sprite2`, `size` wide) travels between
//! the two buttons over `sprite1`; the pointer shadow is drawn under the pointer at the same spot.

use crate::profile::Profile;
use crate::setup::{BikeSetup, Page};
use crate::world::{LaunchOptions, Vfs};
use bevy::app::AppExit;
use bevy::audio::{AudioPlayer, AudioSource};
use bevy::prelude::*;
use mxbikes_formats::{FontFile, MenuNode, StringTable};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    /// Riding, HUD visible.
    None,
    /// `test.mnu` / `test_panel`: the panel Esc opens on track.
    InGameMenu,
    /// `test.mnu` / `testing_pit`: the pit screen of a testing session, with its tab pages.
    Pit,
    /// `garage.mnu` / `idd_garage` with its pages.
    Garage,
    /// `main.mnu` / `main_menu`.
    MainMenu,
    /// `options.mnu` / `idd_options` settings.
    Settings,
    /// `profiles.mnu` / `idd_profiles`.
    Profiles,
    /// `viewreplays.mnu` / `idd_viewreplays`.
    Replays,
    /// `testsetup.mnu` / `idd_testing_setup` with the track picture and conditions.
    TestSetup,
    /// `testsetup.mnu` / `idd_bikechoose`.
    BikeChoose,
}

#[derive(Resource, Debug, Clone)]
pub struct UiState {
    pub garage_page: Page,
    pub pit_tab: String,
    /// Where the settings return to.
    pub settings_return: Option<UiMode>,
    pub settings_tab: String,
    /// The pull item whose list is open.
    pub open_pull: Option<String>,
    /// Check boxes and radio buttons without a data source keep their state here.
    pub checks: HashMap<String, bool>,
    /// Slider positions (position, steps) without a data source.
    pub sliders: HashMap<String, (i64, i64)>,
    /// Selected radio item per `<menu file>/<group>`.
    pub radios: HashMap<String, String>,
    /// Control being captured (settings item, key step 0 / 1) while `idd_wait_control` shows.
    pub capture: Option<(String, usize)>,
    pub edit_focus: Option<String>,
    pub edit_buffer: String,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            garage_page: Page::General,
            pit_tab: "idd_eventinfo".to_string(),
            settings_return: None,
            settings_tab: "idd_graphic_options".to_string(),
            open_pull: None,
            checks: HashMap::new(),
            sliders: HashMap::new(),
            radios: HashMap::new(),
            capture: None,
            edit_focus: None,
            edit_buffer: String::new(),
        }
    }
}

/// Set when the garage closes with Done: the world rebuilds the bike from the setup
/// (`FUN_14009c330` applies the setup on Done).
#[derive(Resource, Default)]
pub struct ApplySetup(pub bool);

/// Set by To Pit: the bike goes back to its pit stall.
#[derive(Resource, Default)]
pub struct ToPits(pub bool);

/// Triggered when the user applies settings (e.g. `id_done` in Settings)
/// to dynamically reconfigure resolution, window mode, vsync, shadows, MSAA, and FOV.
#[derive(Event, Default)]
pub struct ApplyGraphicsSettings;

/// Root of every drawn dialog; despawned when the mode changes.
#[derive(Component)]
pub struct MenuRoot;

/// What pressing a widget does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Button(String),
    Tab(String),
    Spin(String, i64),
    PullSwitch(String),
    PullRow(String, usize),
    Check(String),
    Radio(String, String),
    Slider(String, i64),
    EditBox(String),
}

/// A pressable widget with its per-state sprites (normal, hover, pressed) and text colours.
#[derive(Component)]
pub struct MnuButton {
    pub action: Action,
    pub sprites: [Option<Handle<Image>>; 3],
    pub colors: [Color; 3],
    pub text: Option<Entity>,
    pub sample1: Option<Handle<AudioSource>>,
    pub sample2: Option<Handle<AudioSource>>,
}

/// Mouse pointer from `ui.ui` `pointer/normal`.
#[derive(Component)]
pub struct MnuCursor;

/// Popup dialog layered over the current menu (e.g. `exit_confirm`).
#[derive(Resource, Default)]
pub struct Popup(pub Option<(String, String)>);

#[derive(Resource)]
pub struct UiData {
    pub strings: StringTable,
    pub menus: HashMap<String, MenuNode>,
    pub fonts: HashMap<String, FontFile>,
    pub sprites: HashMap<String, Option<Handle<Image>>>,
    pub sounds: HashMap<String, Option<Handle<AudioSource>>>,
    /// Window height in pixels; text is rasterised at its on-screen size.
    pub screen_h: f32,
}

impl UiData {
    fn menu(&self, file: &str, dialog: &str) -> Option<&MenuNode> {
        self.menus
            .get(file)?
            .children
            .iter()
            .find(|d| d.get_str("name").is_some_and(|n| n.eq_ignore_ascii_case(dialog)))
    }

    pub fn text(&self, id: &str) -> String {
        self.strings.get(id).unwrap_or(id).to_string()
    }
}

/// The value an item shows, as the dialog functions set it.
#[derive(Debug, Clone, Default)]
pub struct ItemValue {
    pub text: Option<String>,
    pub checked: Option<bool>,
    pub entries: Vec<String>,
    pub selected: Option<String>,
    pub slider: Option<(i64, i64)>,
    pub rows: Vec<Vec<String>>,
    pub hidden: bool,
    /// Bitmap from the game files (VFS path) in place of the item's sprite.
    pub image: Option<String>,
}

/// Values for the items of the open screen.
struct Values<'a> {
    mode: UiMode,
    setup: Option<&'a BikeSetup>,
    profile: Option<&'a Profile>,
    strings: &'a StringTable,
    window: (u32, u32),
    state: &'a UiState,
    track: Option<&'a crate::world::TrackInfo>,
    laps: Option<&'a crate::world::LapTimer>,
    session: Option<&'a crate::session::SessionChoice>,
    vfs: &'a mxbikes_vfs::MxBikesVfs,
    bindings: Option<&'a crate::input::Bindings>,
}

/// Lap and sector times as the game prints them (`FUN_140001090`): s.mmm or m'ss.mmm.
pub fn lap_time(t: f32) -> String {
    if t <= 0.0 {
        return "-'--.---".into();
    }
    let ms = ((t % 1.0) * 1000.0) as i64;
    if (t / 60.0) as i64 == 0 {
        format!("{}.{ms:03}", (t % 60.0) as i64)
    } else {
        format!("{}'{:02}.{ms:03}", (t / 60.0) as i64, (t % 60.0) as i64)
    }
}

impl Values<'_> {
    fn get(&self, name: &str) -> ItemValue {
        let id = name.to_ascii_lowercase();
        let mut v = ItemValue::default();
        if self.mode == UiMode::Garage {
            if let Some(s) = self.setup {
                v.text = s.text(&id);
                if let Some((entries, sel)) = s.pull(&id) {
                    v.entries = entries;
                    v.selected = Some(sel);
                }
                v.hidden = s.hidden(&id);
                if id == "id_setupname" || id == "id_edit" {
                    v.text = Some(self.state.edit_buffer.clone());
                } else if id == "id_list" {
                    v.rows = s.list_setup_names().into_iter().filter(|n| n != "-Default-").map(|n| vec![n]).collect();
                } else if id == "id_ridersag" {
                    v.checked = Some(s.sag_mode == crate::setup::SagMode::Race);
                } else if id == "id_staticsag" {
                    v.checked = Some(s.sag_mode == crate::setup::SagMode::Static);
                }
            }
        }
        if matches!(self.mode, UiMode::Settings | UiMode::Pit | UiMode::Profiles) {
            if let Some(pv) = self.profile.and_then(|p| p.item(&id, self.strings, self.window)) {
                v = pv;
            }
        }
        if self.mode == UiMode::Settings {
            // Control buttons show their keys (`FUN_1400cc440`); the capture dialog names the key
            // it waits for (`FUN_1400cb6b0`).
            if let Some(t) = self.bindings.and_then(|b| b.item_text(&id)) {
                v.text = Some(t);
            }
            if id == "id_controllerprofile" {
                // Controller profiles: the `profile/name` of each `controllers/*.cfg` (op 0x305).
                let names = controller_profiles(self.vfs);
                let current = self.profile.map(|p| p.str("input", "controller_profile")).unwrap_or_default();
                v.selected = Some(if names.is_empty() { "--------".into() } else { current });
                v.entries = if names.is_empty() { vec!["--------".into()] } else { names };
            }
            if let Some((item, step)) = &self.state.capture {
                match id.as_str() {
                    "id_title" => {
                        let entry = crate::input::CONTROL_ITEMS.iter().find(|c| c.0 == item);
                        let title = entry.map(|c| if *step == 0 { c.2 } else { c.3 }).unwrap_or("");
                        v.text = Some(self.strings.get(title).unwrap_or(title).to_string());
                    }
                    "id_control" => v.text = Some(String::new()),
                    _ => {}
                }
            }
        }
        if matches!(self.mode, UiMode::InGameMenu | UiMode::Pit) {
            if let Some(tv) = self.session(&id) {
                v = tv;
            }
        }
        if matches!(self.mode, UiMode::TestSetup | UiMode::BikeChoose) {
            if let Some(sv) = self.session.and_then(|s| s.item(&id, self.vfs, self.strings)) {
                v = sv;
            } else if id.starts_with("id_track") {
                // The track info popup describes the chosen track.
                let stem = self.session.and_then(|s| s.tracks.get(s.track)).map(|t| t.stem.clone());
                if let Some(info) = stem.map(|st| crate::world::TrackInfo::load(self.vfs, &st)) {
                    let me = Values { track: Some(&info), ..*self };
                    if let Some(tv) = me.session_track(&id) {
                        v = tv;
                    }
                }
            }
        }
        if v.checked.is_none() {
            v.checked = self.state.checks.get(&id).copied();
        }
        if v.slider.is_none() {
            v.slider = self.state.sliders.get(&id).copied();
        }
        v
    }
}

impl Values<'_> {
    /// Track info values (`FUN_140118940`).
    fn session_track(&self, id: &str) -> Option<ItemValue> {
        let text = |s: String| Some(ItemValue { text: Some(s), ..Default::default() });
        let units = self.profile.map(|p| p.i("misc", "units")).unwrap_or(0);
        let track = self.track?;
        match id {
            "id_track_author" => text(track.author.clone()),
            "id_track_location" => Some(ItemValue { hidden: track.location.is_empty(), text: Some(track.location.clone()), ..Default::default() }),
            "id_location_text" => Some(ItemValue { hidden: track.location.is_empty(), ..Default::default() }),
            "id_tracklength" => text(if units == 1 { format!("{:.2} mi", track.length / 1609.0) } else { format!("{} m", track.length as i64) }),
            "id_trackaltitude" => text(if units == 1 { format!("{} ft", (track.altitude / 0.3048) as i64) } else { format!("{} m", track.altitude as i64) }),
            "id_track_pits" => text(format!("{}", track.pits)),
            "id_trackmap" => Some(ItemValue { image: track.map.clone(), ..Default::default() }),
            "id_track_pic" => Some(ItemValue { image: track.picture.clone(), ..Default::default() }),
            _ => None,
        }
    }

    /// Testing session and track values with the profile's testing defaults (`FUN_1400ef210`:
    /// damage 0, realistic weather 1, clear, 20 C, wind 0 from the north, deformation 1.0x).
    fn session(&self, id: &str) -> Option<ItemValue> {
        let t = |k: &str| self.strings.get(k).unwrap_or(k).to_string();
        let text = |s: String| Some(ItemValue { text: Some(s), ..Default::default() });
        let units = self.profile.map(|p| p.i("misc", "units")).unwrap_or(0);
        let track = self.track;
        match id {
            // Pit bar weather (`FUN_1400e2640`): conditions, air temperature, wind.
            "id_weather" => {
                let (temp, wind, dir) = (20, 0.0f32, 0usize);
                let air = t("CC_airtemp");
                let compass = t(["CC_North", "CC_Northeast", "CC_East", "CC_Southeast", "CC_South", "CC_Southwest", "CC_West", "CC_Northwest"][dir]);
                let (tt, ww) = match units {
                    1 => (format!("{air}: {} F", 32 + (temp as f32 * 1.8) as i64), format!("{} mph {compass}", (wind * 2.23694) as i64)),
                    2 => (format!("{air}: {temp} C"), format!("{} mph {compass}", (wind * 2.23694) as i64)),
                    _ => (format!("{air}: {temp} C"), format!("{} km/h {compass}", (wind * 3.6) as i64)),
                };
                text(format!("{}, {tt}, {ww}", t("CC_Clear")))
            }
            // Testing event info (`FUN_1400e2e20`): On / Off texts.
            "id_damage" | "id_crashmode" | "id_realisticweather" => text("On".into()),
            "id_deformation" => {
                let d = self.profile.map(|p| p.f("testing", "deformation_multiplier")).unwrap_or(1.0);
                text(if d >= 1.0 { format!("{d:.1}x") } else { format!("{d:.2}x") })
            }
            id if id.starts_with("id_track") || id == "id_location_text" => self.session_track(id),
            "id_track_author_unused" => text(track?.author.clone()),
            "id_track_location" => {
                let l = track?.location.clone();
                Some(ItemValue { hidden: l.is_empty(), text: Some(l), ..Default::default() })
            }
            "id_location_text" => Some(ItemValue { hidden: track?.location.is_empty(), ..Default::default() }),
            "id_tracklength" => text(if units == 1 { format!("{:.2} mi", track?.length / 1609.0) } else { format!("{} m", track?.length as i64) }),
            "id_trackaltitude" => text(if units == 1 { format!("{} ft", (track?.altitude / 0.3048) as i64) } else { format!("{} m", track?.altitude as i64) }),
            "id_track_pits" => text(format!("{}", track?.pits)),
            "id_trackmap" => Some(ItemValue { image: track?.map.clone(), ..Default::default() }),
            // Laps page (`FUN_1400d93e0`): lap, lap time, sector times, (speed: empty).
            "id_lapstable" => {
                let laps = self.laps?;
                let rows = laps
                    .laps
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let mut row = vec![format!("{}", i + 1), lap_time(l.time)];
                        let mut prev = Some(0.0);
                        for k in 0..3 {
                            let end = if k < l.splits.len() { l.splits[k] } else if k == l.splits.len() { Some(l.time) } else { None };
                            row.push(match (prev, end) {
                                (Some(a), Some(b)) => lap_time(b - a),
                                _ => lap_time(0.0),
                            });
                            prev = end;
                        }
                        row.push(String::new());
                        row
                    })
                    .collect();
                Some(ItemValue { rows, ..Default::default() })
            }
            "id_ideallaptime" => text(self.laps.and_then(|l| l.ideal()).map(lap_time).unwrap_or_default()),
            "id_track_pic" => Some(ItemValue { image: track?.picture.clone(), ..Default::default() }),
            _ => None,
        }
    }
}

/// Controller profile names: `profile/name` of each `controllers/*.cfg` of the game folder.
fn controller_profiles(vfs: &mxbikes_vfs::MxBikesVfs) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(vfs.game_dir().join("controllers"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "cfg"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| mxbikes_formats::ConfigNode::parse(&t).ok())
                .filter_map(|n| n.get_child("profile").and_then(|p| p.get_str("name")).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub fn setup_ui(mut commands: Commands, options: Res<LaunchOptions>, vfs: Res<Vfs>, mut images: ResMut<Assets<Image>>) {
    let initial_mode = match options.menu.as_deref().map(|m| m.split(':').next().unwrap_or(m)) {
        Some("main") => UiMode::MainMenu,
        Some("pit") => UiMode::Pit,
        Some("panel") => UiMode::InGameMenu,
        Some("garage") => UiMode::Garage,
        Some("settings") => UiMode::Settings,
        Some("profiles") => UiMode::Profiles,
        Some("replays") => UiMode::Replays,
        Some("testsetup") => UiMode::TestSetup,
        Some("bikechoose") => UiMode::BikeChoose,
        _ if options.track == "bg" || options.track == "menu" || options.camera == "showroom" => UiMode::MainMenu,
        _ => UiMode::None,
    };
    commands.insert_resource(initial_mode);
    commands.init_resource::<Popup>();
    // `--menu garage:<page>` / `settings:<dialog>` / `pit:<dialog>` open a given page.
    let mut state = UiState::default();
    match options.menu.as_deref().and_then(|m| m.split_once(':')) {
        Some(("garage", page)) => {
            state.garage_page = match page {
                "suspensions" => Page::Suspensions,
                "drivetrain" => Page::Drivetrain,
                "others" => Page::Others,
                _ => Page::General,
            }
        }
        Some(("settings", dialog)) => state.settings_tab = format!("idd_{dialog}_options"),
        Some(("pit", dialog)) => {
            state.pit_tab = match dialog {
                "track" => "idd_track_info_pit".into(),
                d => format!("idd_{d}"),
            }
        }
        _ => {}
    }
    commands.insert_resource(state);
    commands.init_resource::<ApplySetup>();
    commands.init_resource::<ToPits>();
    let profile = Profile::load();
    commands.insert_resource(crate::input::Bindings::from_profile(&profile));
    commands.insert_resource(profile);

    let read = |p: &str| vfs.0.read(p).ok().map(|d| String::from_utf8_lossy(&d).into_owned());
    let strings = read("ui/english.str").map(|t| StringTable::parse(&t)).unwrap_or_default();
    let mut menus = HashMap::new();
    // The menu list of `ui.ui` (uidef / addmenu).
    let ui_def = read("ui/ui.ui").and_then(|t| MenuNode::parse(&t).ok());
    let menu_files: Vec<String> = ui_def
        .as_ref()
        .and_then(|u| u.child("uidef"))
        .map(|d| d.values.iter().filter(|(k, _)| k == "addmenu").filter_map(|(_, v)| v.first().cloned()).collect())
        .unwrap_or_default();
    for file in menu_files {
        match read(&format!("ui/{file}")).map(|t| MenuNode::parse(&t)) {
            Some(Ok(m)) => {
                menus.insert(file.to_ascii_lowercase(), m);
            }
            Some(Err(e)) => warn!("ui/{file}: {e}"),
            None => warn!("ui/{file} not found"),
        }
    }

    let load = |images: &mut Assets<Image>, name: &str| {
        vfs.0
            .read(&format!("ui/{name}"))
            .ok()
            .and_then(|d| mxbikes_formats::tga::decode_tga(&d))
            .map(|img| images.add(crate::scene::rgba_image(&img, true, false)))
    };
    let pointer = ui_def.as_ref().and_then(|u| u.child("pointer")).and_then(|p| p.child("normal"));
    let size = pointer
        .map(|p| {
            (
                p.floats("sizex").and_then(|v| v.first().copied()).unwrap_or(0.0225),
                p.floats("sizey").and_then(|v| v.first().copied()).unwrap_or(0.04),
            )
        })
        .unwrap_or((0.0225, 0.04));
    let sprite_name = pointer.and_then(|p| p.get_str("sprite")).unwrap_or("pointer.tga").to_string();
    let shadow_name = pointer.and_then(|p| p.get_str("shadow")).map(str::to_string);
    if let Some(ptr) = load(&mut images, &sprite_name) {
        let node = Node {
            position_type: PositionType::Absolute,
            width: Val::Vw(size.0 * 100.0),
            height: Val::Vh(size.1 * 100.0),
            display: Display::None,
            ..default()
        };
        let cursor = commands.spawn((MnuCursor, node.clone(), GlobalZIndex(100))).id();
        if let Some(shadow) = shadow_name.and_then(|s| load(&mut images, &s)) {
            let mut n = node.clone();
            n.display = Display::Flex;
            n.left = Val::Px(0.0);
            n.top = Val::Px(0.0);
            let s = commands.spawn((n, ImageNode::new(shadow))).id();
            commands.entity(cursor).add_child(s);
        }
        let mut n = node;
        n.display = Display::Flex;
        n.left = Val::Px(0.0);
        n.top = Val::Px(0.0);
        let p = commands.spawn((n, ImageNode::new(ptr))).id();
        commands.entity(cursor).add_child(p);
    }

    commands.insert_resource(UiData {
        strings,
        menus,
        fonts: HashMap::new(),
        sprites: HashMap::new(),
        sounds: HashMap::new(),
        screen_h: 900.0,
    });
}

fn sound(data: &mut UiData, vfs: &Vfs, audio_assets: &mut Assets<AudioSource>, name: &str) -> Option<Handle<AudioSource>> {
    let key = name.to_ascii_lowercase();
    if let Some(h) = data.sounds.get(&key) {
        return h.clone();
    }
    let handle = vfs
        .0
        .read(&format!("ui/{name}"))
        .ok()
        .map(|bytes| audio_assets.add(AudioSource { bytes: Arc::from(bytes.into_boxed_slice()) }));
    if handle.is_none() {
        warn!("ui sound {name} not loaded");
    }
    data.sounds.insert(key, handle.clone());
    handle
}

/// Esc toggles the pit bar while riding and steps back out of menus.
pub fn handle_menu_inputs(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_mode: ResMut<UiMode>,
    mut ui_state: ResMut<UiState>,
    mut popup: ResMut<Popup>,
    options: Res<LaunchOptions>,
) {
    let in_menu_scene = options.track == "bg" || options.track == "menu";
    if keys.just_pressed(KeyCode::Escape) {
        if popup.0.is_some() {
            popup.0 = None;
            ui_state.edit_focus = None;
            return;
        }
        if ui_state.open_pull.is_some() {
            ui_state.open_pull = None;
            return;
        }
        *ui_mode = match *ui_mode {
            UiMode::None => UiMode::InGameMenu,
            UiMode::InGameMenu => UiMode::None,
            UiMode::Pit => UiMode::Pit,
            UiMode::Garage if in_menu_scene => UiMode::MainMenu,
            UiMode::Garage => UiMode::Pit,
            UiMode::Settings => ui_state.settings_return.take().unwrap_or(if in_menu_scene { UiMode::MainMenu } else { UiMode::Pit }),
            UiMode::Profiles | UiMode::Replays | UiMode::TestSetup => UiMode::MainMenu,
            UiMode::BikeChoose => UiMode::TestSetup,
            UiMode::MainMenu => UiMode::MainMenu,
        };
    }
}

fn color(v: [f32; 4]) -> Color {
    Color::srgba(v[0], v[1], v[2], v[3])
}

fn rect_node(r: [f32; 4]) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Vw(r[0] * 100.0),
        top: Val::Vh(r[1] * 100.0),
        width: Val::Vw((r[2] - r[0]) * 100.0),
        height: Val::Vh((r[3] - r[1]) * 100.0),
        ..default()
    }
}

/// A node at `r` (screen fractions) placed inside a parent whose top-left is `origin`.
fn rect_in(r: [f32; 4], origin: [f32; 2]) -> Node {
    rect_node([r[0] - origin[0], r[1] - origin[1], r[2] - origin[0], r[3] - origin[1]])
}

fn sprite(data: &mut UiData, vfs: &Vfs, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let key = name.to_ascii_lowercase();
    if let Some(h) = data.sprites.get(&key) {
        return h.clone();
    }
    let handle = vfs
        .0
        .read(&format!("ui/{name}"))
        .ok()
        .and_then(|d| mxbikes_formats::tga::decode_tga(&d))
        .map(|img| images.add(crate::scene::rgba_image(&img, true, false)));
    if handle.is_none() {
        warn!("ui sprite {name} not loaded");
    }
    data.sprites.insert(key, handle.clone());
    handle
}

/// Rasterises `text` with a game font: white texels with glyph coverage as alpha.
fn render_text(font: &FontFile, text: &str, target_h: f32) -> Option<Image> {
    let bytes: Vec<u8> = text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect();
    let height = font.size.max(1);
    let width = (font.measure(&bytes).max(1) + 4) as u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for i in 0..(width * height) as usize {
        rgba[i * 4..i * 4 + 3].copy_from_slice(&[255, 255, 255]);
    }
    let mut pen = 0i32;
    for (i, &ch) in bytes.iter().enumerate() {
        let Some(g) = font.glyph(ch) else { continue };
        let x = pen + g.a;
        for gy in 0..(g.y1 - g.y0).max(0) {
            for gx in 0..(g.x1 + 2 - g.x0).max(0) {
                let (sx, sy) = (g.x0 + gx, g.y0 + gy);
                let (dx, dy) = (x + gx, gy);
                if sx < 0 || sy < 0 || sx as u32 >= font.width || sy as u32 >= font.height {
                    continue;
                }
                if dx < 0 || dx as u32 >= width || dy as u32 >= height {
                    continue;
                }
                let cov = font.coverage[(sy as u32 * font.width + sx as u32) as usize];
                let d = ((dy as u32 * width + dx as u32) * 4 + 3) as usize;
                rgba[d] = rgba[d].max(cov);
            }
        }
        pen += g.a + g.b + g.c;
        if let Some(&next) = bytes.get(i + 1) {
            pen += font.kern(ch, next);
        }
    }
    let th = (target_h.round() as u32).clamp(1, height);
    let tw = ((width as f32 * th as f32 / height as f32).round() as u32).max(1);
    let mut sum = vec![0u32; (tw * th) as usize];
    let mut count = vec![0u32; (tw * th) as usize];
    for y in 0..height {
        let ty = (y * th / height) as usize;
        for x in 0..width {
            let tx = ((x * tw / width) as usize).min(tw as usize - 1);
            let i = ty * tw as usize + tx;
            sum[i] += rgba[((y * width + x) * 4 + 3) as usize] as u32;
            count[i] += 1;
        }
    }
    let mut out = vec![255u8; (tw * th * 4) as usize];
    for i in 0..(tw * th) as usize {
        out[i * 4 + 3] = (sum[i] / count[i].max(1)) as u8;
    }
    let img = mxbikes_formats::texture::DecodedImage { width: tw, height: th, layers: 1, rgba: out };
    Some(crate::scene::rgba_image(&img, true, false))
}

/// Spawns `label` inside `parent` with the font, size, position and alignment of text block `t`.
fn spawn_text_block(
    commands: &mut Commands,
    parent: Entity,
    t: &MenuNode,
    label: &str,
    tint: Color,
    data: &mut UiData,
    vfs: &Vfs,
    images: &mut Assets<Image>,
) -> Option<Entity> {
    if label.is_empty() {
        return None;
    }
    let font_name = t.get_str("font").unwrap_or("main.fnt").to_ascii_lowercase();
    if !data.fonts.contains_key(&font_name) {
        let font = vfs
            .0
            .read(&format!("ui/{font_name}"))
            .ok()
            .and_then(|d| FontFile::parse(&d).map_err(|e| warn!("{font_name}: {e}")).ok())?;
        data.fonts.insert(font_name.clone(), font);
    }
    let font = &data.fonts[&font_name];
    let size = t.floats("fontsize").and_then(|v| v.first().copied()).unwrap_or(0.03);
    let img = render_text(font, label, size * data.screen_h)?;
    let aspect = img.width() as f32 / img.height() as f32;
    let pos = t.floats("pos").unwrap_or_default();
    let (px, py) = (pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0));
    let justify = match t.get_str("align").unwrap_or("left") {
        "right" => JustifyContent::FlexEnd,
        "center" => JustifyContent::Center,
        _ => JustifyContent::FlexStart,
    };
    let row = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: justify,
            align_items: AlignItems::FlexStart,
            padding: UiRect {
                left: Val::Vw(px * 100.0),
                right: Val::Vw(px * 100.0),
                top: Val::Vh(py * 100.0),
                bottom: Val::Px(0.0),
            },
            ..default()
        })
        .id();
    let text = commands
        .spawn((
            Node {
                width: Val::Vh(size * 100.0 * aspect),
                height: Val::Vh(size * 100.0),
                flex_shrink: 0.0,
                ..default()
            },
            ImageNode::new(images.add(img)).with_color(tint),
        ))
        .id();
    commands.entity(row).add_child(text);
    commands.entity(parent).add_child(row);
    Some(text)
}

/// Spawns `label` with the item's `text` block (or its `editbox/text`).
fn spawn_text(
    commands: &mut Commands,
    parent: Entity,
    item: &MenuNode,
    label: &str,
    tint: Color,
    data: &mut UiData,
    vfs: &Vfs,
    images: &mut Assets<Image>,
) -> Option<Entity> {
    let t = item.child("text").or_else(|| item.child("editbox").and_then(|e| e.child("text")))?;
    spawn_text_block(commands, parent, t, label, tint, data, vfs, images)
}

/// Items not drawn in single player: the multiplayer join button sits on the To Track rect.
/// Items for features this build does not have: multiplayer (join, connection, chat, names,
/// flags), the trainer, track screens / 3D grass / reflections / shader toggle, refresh rate and
/// colour depth (not selectable with the window library), head tracking and free look, force
/// feedback, rumble, gearbox preload, the intro movie, dates (replays are not implemented), track
/// help, the rider stand indicator, automatic tyre change and look-to-apex. Per dialog, because
/// label names such as `ID_TEXT1` repeat across dialogs.
fn hidden_item(dialog: &str, item: &str) -> bool {
    const ANY: &[&str] = &["id_join", "id_trainer"];
    const LIST: &[(&str, &[&str])] = &[
        (
            "idd_graphic_options",
            &[
                "id_bpptext", "id_bpp", "id_refresh_text", "id_frequency", "id_bikesreflection", "id_shaders", "id_3dgrass",
                "id_trackscreens", "id_text1", "id_terraindetailbias", "id_terraindetailbias_label",
            ],
        ),
        (
            "idd_input_options",
            &[
                "id_text13", "id_ridertracking", "id_rumble", "id_text9", "id_gain_label", "id_gain", "id_text6", "id_ffenable",
                "id_ffstrength_label", "id_text7", "id_ffstrength", "id_ffreverse",
            ],
        ),
        ("idd_input2_options", &["id_preload", "id_chatlabel", "id_chat", "id_chat_state"]),
        (
            "idd_misc_options",
            &[
                "id_multitext", "id_multiconntext", "id_conn_type", "id_autochattext", "id_datetext", "id_date1", "id_date2",
                "id_date3", "id_text", "id_skipintro", "id_drivingchat", "id_text3", "id_flags", "id_text1", "id_hidenames",
                "id_text2", "id_trackhelp", "id_pitenvironmentsounds", "id_remotebikesvolumetext", "id_remotebikesvolumelabel",
                "id_remotebikesvolume",
            ],
        ),
        (
            "idd_others_options",
            &[
                "id_freelook", "id_trackir", "id_tracking", "id_facetracking", "id_mouselook", "id_joystick", "id_looktoapex",
                "id_corneranticipation_label", "id_corneranticipationscale", "id_showriderstand", "id_autotyreschange",
            ],
        ),
    ];
    if ANY.contains(&item) || (dialog == "idd_misc_options" && item.starts_with("id_autochat")) {
        return true;
    }
    LIST.iter().any(|(d, items)| *d == dialog && items.contains(&item))
}

struct Ctx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    data: &'a mut UiData,
    vfs: &'a Vfs,
    images: &'a mut Assets<Image>,
    audio: &'a mut Assets<AudioSource>,
}

impl Ctx<'_, '_, '_> {
    fn sprite(&mut self, name: Option<&str>) -> Option<Handle<Image>> {
        name.and_then(|n| sprite(self.data, self.vfs, self.images, n))
    }

    fn sound(&mut self, name: Option<&str>) -> Option<Handle<AudioSource>> {
        name.and_then(|n| sound(self.data, self.vfs, self.audio, n))
    }

    fn text(&mut self, parent: Entity, item: &MenuNode, label: &str, tint: Color) -> Option<Entity> {
        spawn_text(self.commands, parent, item, label, tint, self.data, self.vfs, self.images)
    }

    fn text_block(&mut self, parent: Entity, t: &MenuNode, label: &str, tint: Color) -> Option<Entity> {
        spawn_text_block(self.commands, parent, t, label, tint, self.data, self.vfs, self.images)
    }

    /// A pressable node with per-state sprites (`sprite1..3`) and samples from `b`; the caller
    /// inserts the returned [`MnuButton`] once its text exists.
    fn button(&mut self, parent: Entity, node: Node, b: &MenuNode, action: Action, colors: [Color; 3]) -> (Entity, MnuButton) {
        let sprites = [b.get_str("sprite1"), b.get_str("sprite2"), b.get_str("sprite3")].map(|s| self.sprite(s));
        self.sprite_button(parent, node, sprites, b.get_str("sample1"), b.get_str("sample2").or_else(|| b.get_str("sample")), action, colors)
    }

    #[allow(clippy::too_many_arguments)]
    fn sprite_button(
        &mut self,
        parent: Entity,
        node: Node,
        sprites: [Option<Handle<Image>>; 3],
        sample1: Option<&str>,
        sample2: Option<&str>,
        action: Action,
        colors: [Color; 3],
    ) -> (Entity, MnuButton) {
        let sample1 = self.sound(sample1);
        let sample2 = self.sound(sample2);
        let image = match &sprites[0] {
            Some(h) => ImageNode::new(h.clone()),
            None => ImageNode::default().with_color(Color::NONE),
        };
        let e = self.commands.spawn((node, Button, image)).id();
        self.commands.entity(parent).add_child(e);
        (e, MnuButton { action, sprites, colors, text: None, sample1, sample2 })
    }
}

fn fl(n: &MenuNode, key: &str, default: f32) -> f32 {
    n.floats(key).and_then(|v| v.first().copied()).unwrap_or(default)
}

fn spawn_dialog(ctx: &mut Ctx, root: Entity, dialog: &MenuNode, values: &Values, ui_state: &UiState, file: &str) {
    let dialog_name = dialog.get_str("name").unwrap_or("").to_ascii_lowercase();
    for item in &dialog.children {
        let name = item.get_str("name").unwrap_or("").to_ascii_lowercase();
        if hidden_item(&dialog_name, &name) {
            continue;
        }
        let value = values.get(&name);
        if value.hidden {
            continue;
        }
        match item.name.as_str() {
            "item_darkbox" => {
                let (Some(r), Some(c)) = (item.rect(), item.color("color")) else { continue };
                let e = ctx.commands.spawn((rect_node(r), BackgroundColor(color(c)))).id();
                ctx.commands.entity(root).add_child(e);
            }
            "item_bitmap" => {
                let Some(r) = item.rect() else { continue };
                let from_game = value.image.as_deref().and_then(|p| {
                    ctx.vfs
                        .0
                        .read(p)
                        .ok()
                        .and_then(|d| mxbikes_formats::tga::decode_tga(&d))
                        .map(|img| ctx.images.add(crate::scene::rgba_image(&img, true, false)))
                });
                let Some(h) = from_game.or_else(|| ctx.sprite(item.get_str("sprite"))) else { continue };
                let e = ctx.commands.spawn((rect_node(r), ImageNode::new(h))).id();
                ctx.commands.entity(root).add_child(e);
            }
            "item_text" => {
                let Some(r) = item.rect() else { continue };
                let mut node = ctx.commands.spawn(rect_node(r));
                if let Some(b) = item.color("backcolor") {
                    node.insert(BackgroundColor(color(b)));
                }
                let e = node.id();
                ctx.commands.entity(root).add_child(e);
                let label = value.text.clone().unwrap_or_else(|| ctx.data.text(item.get_str("textid").unwrap_or("")));
                let tint = item.color("color").map(color).unwrap_or(Color::WHITE);
                ctx.text(e, item, &label, tint);
            }
            "item_button" => {
                let Some(b) = item.child("button") else { continue };
                let Some(r) = b.rect() else { continue };
                let colors = ["color1", "color2", "color3"].map(|k| item.color(k).map(color).unwrap_or(Color::WHITE));
                let (e, mut mb) = ctx.button(root, rect_node(r), b, Action::Button(name.clone()), colors);
                let label = value.text.clone().unwrap_or_else(|| ctx.data.text(item.get_str("textid").unwrap_or("")));
                mb.text = ctx.text(e, item, &label, colors[0]);
                ctx.commands.entity(e).insert(mb);
            }
            "item_tab" => {
                let Some(r) = item.rect() else { continue };
                let active = tab_active(&name, ui_state, values.mode);
                let c1 = item.color("color1").map(color).unwrap_or(Color::WHITE);
                let c2 = item.color("color2").map(color).unwrap_or(Color::WHITE);
                let s1 = ctx.sprite(item.get_str("sprite1"));
                let s2 = ctx.sprite(item.get_str("sprite2"));
                let shown = if active { s2.clone().or(s1.clone()) } else { s1.clone() };
                let sample2 = ctx.sound(item.get_str("sample2"));
                let image = match &shown {
                    Some(h) => ImageNode::new(h.clone()),
                    None => ImageNode::default().with_color(Color::NONE),
                };
                let e = ctx.commands.spawn((rect_node(r), Button, image)).id();
                ctx.commands.entity(root).add_child(e);
                let tint = if active { c2 } else { c1 };
                let label = ctx.data.text(item.get_str("textid").unwrap_or(""));
                let text = ctx.text(e, item, &label, tint);
                ctx.commands.entity(e).insert(MnuButton {
                    action: Action::Tab(name.clone()),
                    sprites: [shown.clone(), shown.clone(), shown],
                    colors: [tint; 3],
                    text,
                    sample1: None,
                    sample2,
                });
            }
            "item_checkbox" | "item_radio" => {
                let Some(r) = item.rect() else { continue };
                let group = format!("{file}/{}", item.get_str("group").unwrap_or("0"));
                let checked = if item.name == "item_radio" {
                    match ui_state.radios.get(&group) {
                        Some(sel) => *sel == name,
                        None => value.checked.unwrap_or(false),
                    }
                } else {
                    value.checked.unwrap_or(false)
                };
                let e = ctx.commands.spawn((rect_node(r), Button)).id();
                ctx.commands.entity(root).add_child(e);
                let sprite_key = if checked { "sprite2" } else { "sprite1" };
                if let Some(h) = ctx.sprite(item.get_str(sprite_key)) {
                    let sx = fl(item, "spriteposx", 0.0);
                    let sy = fl(item, "spriteposy", 0.0);
                    let sw = fl(item, "spritesizex", 0.0125);
                    let sh = fl(item, "spritesizey", 0.0222);
                    let s = ctx.commands.spawn((rect_node([sx, sy, sx + sw, sy + sh]), ImageNode::new(h))).id();
                    ctx.commands.entity(e).add_child(s);
                }
                let tint = item.color("color").map(color).unwrap_or(Color::WHITE);
                let label = ctx.data.text(item.get_str("textid").unwrap_or(""));
                ctx.text(e, item, &label, tint);
                let action = if item.name == "item_radio" {
                    Action::Radio(name.clone(), group)
                } else {
                    Action::Check(name.clone())
                };
                let sample2 = ctx.sound(item.get_str("sample2"));
                ctx.commands.entity(e).insert(MnuButton {
                    action,
                    sprites: [None, None, None],
                    colors: [tint; 3],
                    text: None,
                    sample1: None,
                    sample2,
                });
                ctx.commands.entity(e).insert(ImageNode::default().with_color(Color::NONE));
            }
            "item_spin" => {
                let Some(r) = item.rect() else { continue };
                let e = ctx.commands.spawn(rect_node(r)).id();
                ctx.commands.entity(root).add_child(e);
                if let Some(spin) = item.child("spin") {
                    for (key, dir) in [("button1", -1), ("button2", 1)] {
                        let Some(b) = spin.child(key) else { continue };
                        let Some(br) = b.rect() else { continue };
                        let (b, mb) = ctx.button(e, rect_in(br, [r[0], r[1]]), b, Action::Spin(name.clone(), dir), [Color::WHITE; 3]);
                        ctx.commands.entity(b).insert(mb);
                    }
                }
                let tint = item.color("color").map(color).unwrap_or(Color::WHITE);
                let label = value.text.clone().unwrap_or_default();
                ctx.text(e, item, &label, tint);
            }
            "item_slider" => {
                let Some(s) = item.child("slider") else { continue };
                let Some(r) = s.rect() else { continue };
                let e = ctx.commands.spawn(rect_node(r)).id();
                ctx.commands.entity(root).add_child(e);
                let bsize = fl(s, "buttonssize", 0.0);
                let knob = fl(s, "size", 0.0125);
                if let Some(h) = ctx.sprite(s.get_str("sprite1")) {
                    let track = ctx
                        .commands
                        .spawn((rect_node([bsize, 0.0, r[2] - r[0] - bsize, r[3] - r[1]]), ImageNode::new(h)))
                        .id();
                    ctx.commands.entity(e).add_child(track);
                }
                let (pos, steps) = value.slider.unwrap_or((0, 0));
                if let Some(h) = ctx.sprite(s.get_str("sprite2")) {
                    let travel = (r[2] - r[0] - 2.0 * bsize - knob).max(0.0);
                    let x = bsize + if steps > 0 { travel * pos as f32 / steps as f32 } else { 0.0 };
                    let k = ctx.commands.spawn((rect_node([x, 0.0, x + knob, r[3] - r[1]]), ImageNode::new(h))).id();
                    ctx.commands.entity(e).add_child(k);
                }
                for (key, dir) in [("button0", -1), ("button1", 1)] {
                    let Some(b) = s.child(key) else { continue };
                    let Some(br) = b.rect() else { continue };
                    let (b, mb) = ctx.button(e, rect_in(br, [r[0], r[1]]), b, Action::Slider(name.clone(), dir), [Color::WHITE; 3]);
                    ctx.commands.entity(b).insert(mb);
                }
            }
            "item_editbox" => {
                let Some(r) = item.rect() else { continue };
                let eb = item.child("editbox");
                let mut node = ctx.commands.spawn((rect_node(r), Button));
                if let Some(b) = eb.and_then(|e| e.color("backcolor")) {
                    node.insert(BackgroundColor(color(b)));
                }
                let e = node.id();
                ctx.commands.entity(root).add_child(e);
                let tint = eb.and_then(|e| e.color("color1")).map(color).unwrap_or(Color::WHITE);
                let label = value.text.clone().unwrap_or_default();
                let text = ctx.text(e, item, &label, tint);
                ctx.commands.entity(e).insert(MnuButton {
                    action: Action::EditBox(name.clone()),
                    sprites: [None, None, None],
                    colors: [tint; 3],
                    text,
                    sample1: None,
                    sample2: None,
                });
            }
            "item_list" => {
                let Some(l) = item.child("list") else { continue };
                let Some(r) = l.rect() else { continue };
                let mut node = ctx.commands.spawn(rect_node(r));
                if let Some(b) = l.color("backcolor") {
                    node.insert(BackgroundColor(color(b)));
                }
                let e = node.id();
                ctx.commands.entity(root).add_child(e);
                let columns = fl(l, "numcolumns", 1.0) as usize;
                let widths: Vec<f32> = (0..columns).map(|i| fl(l, &format!("columnsize{i}"), (r[2] - r[0]) / columns as f32)).collect();
                let spacing = fl(l, "spacing", 0.0222);
                let mut y = 0.0;
                if fl(l, "sortbar", 0.0) != 0.0 {
                    let h = fl(l, "sortbarheight", spacing);
                    let mut bar = ctx.commands.spawn(rect_node([0.0, 0.0, r[2] - r[0], h]));
                    if let Some(b) = l.color("sortbackcolor") {
                        bar.insert(BackgroundColor(color(b)));
                    }
                    let bar = bar.id();
                    ctx.commands.entity(e).add_child(bar);
                    let tint = l.color("sortbartextcolor").map(color).unwrap_or(Color::BLACK);
                    let mut x = 0.0;
                    for (i, w) in widths.iter().enumerate() {
                        let label = ctx.data.text(l.get_str(&format!("columntextid{i}")).unwrap_or(""));
                        let cell = ctx.commands.spawn(rect_node([x, 0.0, x + w, h])).id();
                        ctx.commands.entity(bar).add_child(cell);
                        if let Some(t) = l.child("sortbartext") {
                            ctx.text_block(cell, t, &label, tint);
                        }
                        x += w;
                    }
                    y = h;
                }
                let tint = l.color("color1").map(color).unwrap_or(Color::BLACK);
                for row in &value.rows {
                    if y + spacing > r[3] - r[1] + 1e-4 {
                        break;
                    }
                    let mut x = 0.0;
                    for (i, w) in widths.iter().enumerate() {
                        let cell = ctx.commands.spawn(rect_node([x, y, x + w, y + spacing])).id();
                        ctx.commands.entity(e).add_child(cell);
                        if let (Some(t), Some(label)) = (l.child("text"), row.get(i)) {
                            ctx.text_block(cell, t, label, tint);
                        }
                        x += w;
                    }
                    y += spacing;
                }
            }
            "item_pull" => {
                let Some(r) = item.rect() else { continue };
                let mut node = ctx.commands.spawn(rect_node(r));
                if let Some(b) = item.color("backcolor") {
                    node.insert(BackgroundColor(color(b)));
                }
                let e = node.id();
                ctx.commands.entity(root).add_child(e);
                let tint = item.color("textcolor").map(color).unwrap_or(Color::WHITE);
                let label = value.selected.clone().unwrap_or_default();
                ctx.text(e, item, &label, tint);
                let open = ui_state.open_pull.as_deref() == Some(name.as_str());
                let sw = fl(item, "switchsize", 0.0125);
                let w = r[2] - r[0];
                // Switch: sprite1 closed, sprite2 open or pressed.
                let closed = ctx.sprite(item.get_str("sprite1"));
                let pressed = ctx.sprite(item.get_str("sprite2"));
                let first = if open { pressed.clone() } else { closed };
                let sprites = [first.clone(), first, pressed];
                let (b, mb) = ctx.sprite_button(
                    e,
                    rect_node([w - sw, 0.0, w, r[3] - r[1]]),
                    sprites,
                    None,
                    item.get_str("sample"),
                    Action::PullSwitch(name.clone()),
                    [Color::WHITE; 3],
                );
                ctx.commands.entity(b).insert(mb);
                if open {
                    let spacing = fl(item, "spacing", r[3] - r[1]);
                    let rows = fl(item, "visiblerows", 10.0) as usize;
                    let pw = item.floats("pullwidth").and_then(|v| v.first().copied()).unwrap_or(w);
                    let shown = value.entries.len().min(rows);
                    let mut list = ctx.commands.spawn((
                        rect_node([r[0], r[3], r[0] + pw, r[3] + spacing * shown as f32]),
                        GlobalZIndex(50),
                    ));
                    if let Some(b) = item.color("pullbackcolor") {
                        list.insert(BackgroundColor(color(b)));
                    }
                    let list = list.id();
                    ctx.commands.entity(root).add_child(list);
                    let c1 = item.color("pulltextcolor1").map(color).unwrap_or(Color::WHITE);
                    let c2 = item.color("pulltextcolor2").map(color).unwrap_or(c1);
                    for (i, entry) in value.entries.iter().take(shown).enumerate() {
                        let y = spacing * i as f32;
                        let row = ctx.commands.spawn((rect_node([0.0, y, pw, y + spacing]), Button, ImageNode::default().with_color(Color::NONE))).id();
                        ctx.commands.entity(list).add_child(row);
                        let text = ctx.text(row, item, entry, c1);
                        ctx.commands.entity(row).insert(MnuButton {
                            action: Action::PullRow(name.clone(), i),
                            sprites: [None, None, None],
                            colors: [c1, c2, c2],
                            text,
                            sample1: None,
                            sample2: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn tab_active(name: &str, ui_state: &UiState, mode: UiMode) -> bool {
    match name {
        "id_general_tab" => ui_state.garage_page == Page::General,
        "id_suspensions_tab" => ui_state.garage_page == Page::Suspensions,
        "id_drivetrain_tab" => ui_state.garage_page == Page::Drivetrain,
        "id_others_tab" if mode == UiMode::Garage => ui_state.garage_page == Page::Others,
        "id_others_tab" => ui_state.settings_tab == "idd_others_options",
        "id_results" => ui_state.pit_tab == "idd_testingday_results",
        "id_eventinfo" => ui_state.pit_tab == "idd_eventinfo",
        "id_laps" => ui_state.pit_tab == "idd_laps",
        "id_trackinfo" => ui_state.pit_tab == "idd_track_info_pit",
        "id_trainer" => ui_state.pit_tab == "idd_trainer",
        "id_gfx_tab" => ui_state.settings_tab == "idd_graphic_options",
        "id_input_tab" => ui_state.settings_tab == "idd_input_options",
        "id_input2_tab" => ui_state.settings_tab == "idd_input2_options",
        "id_input3_tab" => ui_state.settings_tab == "idd_input3_options",
        "id_misc_tab" => ui_state.settings_tab == "idd_misc_options",
        _ => false,
    }
}

/// Rebuilds drawn dialogs when mode, popup, values or active tab change.
#[allow(clippy::too_many_arguments)]
pub fn sync_menu_visibility(
    mut commands: Commands,
    ui_mode: Res<UiMode>,
    ui_state: Res<UiState>,
    popup: Res<Popup>,
    sources: (
        Option<Res<BikeSetup>>,
        Option<Res<Profile>>,
        Option<Res<crate::world::TrackInfo>>,
        Option<Res<crate::world::LapTimer>>,
        Option<Res<crate::session::SessionChoice>>,
        Option<Res<crate::input::Bindings>>,
    ),
    data: Option<ResMut<UiData>>,
    vfs: Res<Vfs>,
    mut images: ResMut<Assets<Image>>,
    mut audio_assets: ResMut<Assets<AudioSource>>,
    roots: Query<Entity, With<MenuRoot>>,
    windows: Query<&Window>,
    mut lap_count: Local<usize>,
) {
    let (setup, profile, track, laps, session, bindings) = sources;
    let Some(mut data) = data else { return };
    if let Ok(w) = windows.get_single() {
        data.screen_h = w.resolution.height();
    }
    let setup_changed = setup.as_ref().is_some_and(|s| s.is_changed())
        || session.as_ref().is_some_and(|s| s.is_changed())
        || bindings.as_ref().is_some_and(|b| b.is_changed())
        || profile.as_ref().is_some_and(|p| p.is_changed())
        || laps.as_ref().is_some_and(|l| std::mem::replace(&mut *lap_count, l.laps.len()) != l.laps.len());
    if !ui_mode.is_changed() && !popup.is_changed() && !ui_state.is_changed() && !setup_changed {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn_recursive();
    }
    let garage_page = setup.as_ref().map(|s| s.page_dialog(ui_state.garage_page)).unwrap_or("idd_garage_general");
    let mut dialogs: Vec<(&str, &str)> = match *ui_mode {
        UiMode::None => vec![],
        UiMode::MainMenu => vec![("main.mnu", "main_menu")],
        UiMode::InGameMenu => vec![("test.mnu", "test_panel")],
        UiMode::Pit => vec![("test.mnu", "testing_pit"), ("test.mnu", ui_state.pit_tab.as_str())],
        UiMode::Garage => vec![("garage.mnu", "idd_garage"), ("garage.mnu", garage_page)],
        UiMode::Settings => vec![("options.mnu", "idd_options"), ("options.mnu", ui_state.settings_tab.as_str())],
        UiMode::Profiles => vec![("profiles.mnu", "idd_profiles")],
        UiMode::Replays => vec![("viewreplays.mnu", "idd_viewreplays")],
        UiMode::TestSetup => vec![
            ("testsetup.mnu", "idd_testing_setup"),
            ("testsetup.mnu", "idd_track_image"),
            ("testsetup.mnu", "idd_testing_set"),
        ],
        UiMode::BikeChoose => vec![("testsetup.mnu", "idd_bikechoose")],
    };
    if let Some((file, dialog)) = &popup.0 {
        dialogs.push((file.as_str(), dialog.as_str()));
    }
    let window = windows.get_single().map(|w| (w.resolution.physical_width(), w.resolution.physical_height())).unwrap_or((1600, 900));
    let strings = data.strings.clone();
    let values = Values { mode: *ui_mode, setup: setup.as_deref(), profile: profile.as_deref(), strings: &strings, window, state: &ui_state, track: track.as_deref(), laps: laps.as_deref(), session: session.as_deref(), vfs: &vfs.0, bindings: bindings.as_deref() };
    for (file, dialog) in dialogs {
        let Some(node) = data.menu(file, dialog).cloned() else {
            warn!("ui dialog {file}/{dialog} not found");
            continue;
        };
        let root = commands
            .spawn((
                MenuRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Vw(100.0),
                    height: Val::Vh(100.0),
                    ..default()
                },
                GlobalZIndex(10),
            ))
            .id();
        let mut ctx = Ctx { commands: &mut commands, data: &mut data, vfs: &vfs, images: &mut images, audio: &mut audio_assets };
        spawn_dialog(&mut ctx, root, &node, &values, &ui_state, file);
    }
}

/// Widget states, sounds and actions.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn handle_menu_interactions(
    mut commands: Commands,
    mut buttons: Query<(&Interaction, &MnuButton, &mut ImageNode), Changed<Interaction>>,
    mut texts: Query<&mut ImageNode, Without<MnuButton>>,
    mut ui_mode: ResMut<UiMode>,
    mut ui_state: ResMut<UiState>,
    mut popup: ResMut<Popup>,
    options: Res<LaunchOptions>,
    (mut exit, mut reload_events, mut apply_graphics, mut apply, mut to_pits): (
        EventWriter<AppExit>,
        EventWriter<crate::world::ReloadSession>,
        EventWriter<ApplyGraphicsSettings>,
        ResMut<ApplySetup>,
        ResMut<ToPits>,
    ),
    mut sim: Option<ResMut<crate::bike_entity::BikeSim>>,
    mut setup: Option<ResMut<BikeSetup>>,
    mut profile: Option<ResMut<Profile>>,
    mut session: Option<ResMut<crate::session::SessionChoice>>,
    vfs: Res<Vfs>,
    data: Option<Res<UiData>>,
) {
    let choosing = matches!(*ui_mode, UiMode::TestSetup | UiMode::BikeChoose);
    // Profile-backed widgets: the settings pages, and the pit's trainer page (saved at once,
    // as the game writes those keys when the page closes).
    let settings = matches!(*ui_mode, UiMode::Settings | UiMode::Pit);
    let save_now = *ui_mode == UiMode::Pit;
    let in_menu_scene = options.track == "bg" || options.track == "menu";
    // Menu sounds play at master x UI volume (`sound/master_volume`, `sound/ui_volume`).
    let ui_gain = profile.as_ref().map(|p| p.f("sound", "master_volume") * p.f("sound", "ui_volume")).unwrap_or(0.5);
    let ui_sound = |h: Handle<AudioSource>| {
        (AudioPlayer::new(h), PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::new(ui_gain)))
    };
    for (interaction, button, mut image) in &mut buttons {
        let state = match interaction {
            Interaction::None => 0,
            Interaction::Hovered => 1,
            Interaction::Pressed => 2,
        };
        if button.sprites.iter().any(Option::is_some) {
            match &button.sprites[state] {
                Some(h) => {
                    image.image = h.clone();
                    image.color = Color::WHITE;
                }
                None => image.color = Color::NONE,
            }
        }
        if let Some(mut t) = button.text.and_then(|t| texts.get_mut(t).ok()) {
            t.color = button.colors[state];
        }
        if *interaction == Interaction::Hovered {
            if let Some(ref s1) = button.sample1 {
                commands.spawn(ui_sound(s1.clone()));
            }
        }
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(ref s2) = button.sample2 {
            commands.spawn(ui_sound(s2.clone()));
        }
        match &button.action {
            Action::EditBox(item) => {
                ui_state.edit_focus = Some(item.clone());
            }
            Action::Spin(item, dir) => {
                if let Some(s) = setup.as_mut() {
                    if *ui_mode == UiMode::Garage {
                        s.spin(item, *dir);
                    }
                }
                if let Some(p) = profile.as_mut().filter(|_| settings) {
                    p.spin(item, *dir);
                }
                if let Some(s) = session.as_mut().filter(|_| choosing) {
                    s.spin(item, *dir);
                }
            }
            Action::PullSwitch(item) => {
                ui_state.open_pull = if ui_state.open_pull.as_deref() == Some(item.as_str()) { None } else { Some(item.clone()) };
            }
            Action::PullRow(item, index) => {
                if let Some(s) = setup.as_mut() {
                    if *ui_mode == UiMode::Garage {
                        s.pull_select(item, *index);
                    }
                }
                if let Some(p) = profile.as_mut().filter(|_| settings) {
                    if item == "id_controllerprofile" {
                        if let Some(name) = controller_profiles(&vfs.0).get(*index) {
                            p.set("input", "controller_profile", name);
                        }
                    } else {
                        p.pull_select(item, *index);
                    }
                }
                if let Some(s) = session.as_mut().filter(|_| choosing) {
                    s.pull_select(item, *index, &vfs.0);
                }
                ui_state.open_pull = None;
            }
            Action::Check(item) => {
                let handled = (settings && profile.as_mut().is_some_and(|p| p.toggle(item)))
                    || (choosing && session.as_mut().is_some_and(|s| s.toggle(item)));
                if !handled {
                    let v = ui_state.checks.get(item).copied().unwrap_or(false);
                    ui_state.checks.insert(item.clone(), !v);
                }
            }
            Action::Radio(item, group) => {
                let handled = settings && profile.as_mut().is_some_and(|p| p.radio(item));
                if handled {
                    ui_state.radios.remove(group);
                } else if *ui_mode == UiMode::Garage {
                    if let Some(s) = setup.as_mut() {
                        match item.to_ascii_lowercase().as_str() {
                            "id_ridersag" => s.sag_mode = crate::setup::SagMode::Race,
                            "id_staticsag" => s.sag_mode = crate::setup::SagMode::Static,
                            _ => {}
                        }
                    }
                    ui_state.radios.insert(group.clone(), item.clone());
                } else {
                    ui_state.radios.insert(group.clone(), item.clone());
                }
            }
            Action::Slider(item, dir) => {
                let strings = data.as_ref().map(|d| d.strings.clone()).unwrap_or_default();
                let current = profile.as_ref().filter(|_| settings).and_then(|p| p.item(item, &strings, (1600, 900))).and_then(|v| v.slider);
                match (current, profile.as_mut().filter(|_| settings)) {
                    (Some((pos, steps)), Some(p)) => {
                        p.slide(item, (pos + dir).clamp(0, steps));
                    }
                    _ if choosing && session.is_some() => {
                        let s = session.as_mut().unwrap();
                        let pos = s.item(item, &vfs.0, &strings).and_then(|v| v.slider).map(|(p, _)| p).unwrap_or(0);
                        s.slide(item, (pos + dir).clamp(0, 200));
                    }
                    _ => {
                        let (pos, steps) = ui_state.sliders.get(item).copied().unwrap_or((0, 0));
                        ui_state.sliders.insert(item.clone(), ((pos + dir).clamp(0, steps), steps));
                    }
                }
            }
            Action::Tab(name) => match name.as_str() {
                "id_general_tab" => ui_state.garage_page = Page::General,
                "id_suspensions_tab" => ui_state.garage_page = Page::Suspensions,
                "id_drivetrain_tab" => ui_state.garage_page = Page::Drivetrain,
                "id_others_tab" if *ui_mode == UiMode::Garage => ui_state.garage_page = Page::Others,
                "id_others_tab" => ui_state.settings_tab = "idd_others_options".into(),
                "id_results" => ui_state.pit_tab = "idd_testingday_results".into(),
                "id_eventinfo" => ui_state.pit_tab = "idd_eventinfo".into(),
                "id_laps" => ui_state.pit_tab = "idd_laps".into(),
                "id_trackinfo" => ui_state.pit_tab = "idd_track_info_pit".into(),
                "id_trainer" => ui_state.pit_tab = "idd_trainer".into(),
                "id_gfx_tab" => ui_state.settings_tab = "idd_graphic_options".into(),
                "id_input_tab" => ui_state.settings_tab = "idd_input_options".into(),
                "id_input2_tab" => ui_state.settings_tab = "idd_input2_options".into(),
                "id_input3_tab" => ui_state.settings_tab = "idd_input3_options".into(),
                "id_misc_tab" => ui_state.settings_tab = "idd_misc_options".into(),
                _ => {}
            },
            Action::Button(name) => {
                let name_lc = name.to_ascii_lowercase();
                match name_lc.as_str() {
                    "id_testing" => {
                        if in_menu_scene {
                            *ui_mode = UiMode::TestSetup;
                        } else {
                            popup.0 = None;
                            *ui_mode = UiMode::Pit;
                        }
                    }
                    "id_start" if choosing => {
                        // The session's deformation is kept in the profile's testing section.
                        if let (Some(s), Some(p)) = (session.as_ref(), profile.as_mut()) {
                            p.set("testing", "deformation_multiplier", s.deformation);
                            p.save();
                        }
                        if let Some(s) = session.as_ref() {
                            if let Some((track, bike, paints)) = s.session_params() {
                                reload_events.send(crate::world::ReloadSession { track, bike, paints });
                                popup.0 = None;
                                *ui_mode = UiMode::Pit;
                            } else {
                                s.start();
                            }
                        }
                    }
                    "id_bikechoose" => *ui_mode = UiMode::BikeChoose,
                    "id_track_info" if *ui_mode == UiMode::TestSetup => popup.0 = Some(("testsetup.mnu".into(), "idd_track_info".into())),
                    "id_totrack" | "id_start" | "id_continue" => {
                        popup.0 = None;
                        *ui_mode = UiMode::None;
                    }
                    "id_topit" => {
                        to_pits.0 = true;
                        *ui_mode = UiMode::Pit;
                    }
                    "id_undo" if *ui_mode == UiMode::Garage => {
                        if let Some(s) = setup.as_mut() {
                            s.reset_to_default();
                        }
                    }
                    "id_notes" if *ui_mode == UiMode::Garage => {
                        if let Some(s) = setup.as_ref() {
                            ui_state.edit_focus = Some("id_edit".into());
                            ui_state.edit_buffer = s.notes.clone();
                            popup.0 = Some(("garage.mnu".into(), "idd_setup_notes".into()));
                        }
                    }
                    "id_save" if *ui_mode == UiMode::Garage => {
                        if let Some(s) = setup.as_mut() {
                            if s.cur_setup_name == "-Default-" {
                                ui_state.edit_focus = Some("id_setupname".into());
                                ui_state.edit_buffer = String::new();
                                popup.0 = Some(("garage.mnu".into(), "idd_setup_save".into()));
                            } else {
                                let stp = s.to_stp();
                                let dir = format!("setups/{}", s.bike_id);
                                let _ = std::fs::create_dir_all(&dir);
                                let path = format!("{}/{}.stp", dir, s.cur_setup_name);
                                let bytes = stp.write();
                                let _ = std::fs::write(&path, bytes);
                            }
                        }
                    }
                    "id_saveas" if *ui_mode == UiMode::Garage => {
                        if let Some(s) = setup.as_ref() {
                            ui_state.edit_focus = Some("id_setupname".into());
                            ui_state.edit_buffer = if s.cur_setup_name != "-Default-" {
                                s.cur_setup_name.clone()
                            } else {
                                String::new()
                            };
                            popup.0 = Some(("garage.mnu".into(), "idd_setup_save".into()));
                        }
                    }
                    "id_delete" if *ui_mode == UiMode::Garage => {
                        if let Some(s) = setup.as_ref() {
                            if s.cur_setup_name != "-Default-" {
                                popup.0 = Some(("garage.mnu".into(), "idd_setup_delete".into()));
                            }
                        }
                    }
                    "id_copyfrom" if *ui_mode == UiMode::Garage => {
                        popup.0 = Some(("garage.mnu".into(), "idd_setup_copyfrom".into()));
                    }
                    "id_ok" => {
                        if popup.0.as_ref().is_some_and(|p| p.1 == "idd_setup_save") {
                            let trimmed = ui_state.edit_buffer.trim().to_string();
                            if !trimmed.is_empty() && trimmed != "-Default-" {
                                if let Some(s) = setup.as_mut() {
                                    s.cur_setup_name = trimmed.clone();
                                    let stp = s.to_stp();
                                    let dir = format!("setups/{}", s.bike_id);
                                    let _ = std::fs::create_dir_all(&dir);
                                    let path = format!("{}/{}.stp", dir, trimmed);
                                    let bytes = stp.write();
                                    let _ = std::fs::write(&path, bytes);
                                }
                            }
                            popup.0 = None;
                            ui_state.edit_focus = None;
                        } else if popup.0.as_ref().is_some_and(|p| p.1 == "idd_setup_notes") {
                            if let Some(s) = setup.as_mut() {
                                s.notes = ui_state.edit_buffer.clone();
                            }
                            popup.0 = None;
                            ui_state.edit_focus = None;
                        } else if popup.0.as_ref().is_some_and(|p| p.1 == "idd_setup_copyfrom") {
                            popup.0 = None;
                        }
                    }
                    "id_done" => {
                        popup.0 = None;
                        if *ui_mode == UiMode::Garage {
                            apply.0 = true;
                        }
                        if settings {
                            if let Some(p) = profile.as_ref() {
                                p.save();
                                if let Some(sim) = sim.as_mut() {
                                    p.apply_to_bike(&mut sim.0);
                                }
                            }
                            apply_graphics.send(ApplyGraphicsSettings);
                        }
                        *ui_mode = match *ui_mode {
                            UiMode::Settings => ui_state.settings_return.take().unwrap_or(if in_menu_scene { UiMode::MainMenu } else { UiMode::Pit }),
                            UiMode::Garage if !in_menu_scene => UiMode::Pit,
                            _ if in_menu_scene => UiMode::MainMenu,
                            _ => UiMode::None,
                        };
                    }
                    "id_garage" => *ui_mode = UiMode::Garage,
                    "id_settings" => {
                        ui_state.settings_return = Some(*ui_mode);
                        *ui_mode = UiMode::Settings;
                    }
                    "id_profiles" => *ui_mode = UiMode::Profiles,
                    "id_viewreplays" | "id_replay" => *ui_mode = UiMode::Replays,
                    "id_view" if *ui_mode == UiMode::Replays => {
                        popup.0 = None;
                        *ui_mode = UiMode::None;
                    }
                    "id_restart" => {
                        to_pits.0 = true;
                        *ui_mode = UiMode::Pit;
                    }
                    "id_stand" => {
                        // In PiBoSo pit bar, id_stand toggles the bike on/off the triangle/scissor stand
                    }
                    "id_photo" => {
                        // Pit photo mode
                    }
                    "id_bikeclean" | "id_resettrack" => {
                        if let Some(sim) = sim.as_mut() {
                            sim.0.dirt_factor = 0.0;
                            sim.0.rider_dirt_factor = 0.0;
                        }
                    }
                    "id_fronttyresetreset" | "id_reartyresetreset" => {
                        // Resets tyre compound wear in garage setup
                    }
                    "id_guidcopy" => {
                        // Copies GUID in profile menu
                    }
                    "id_new" | "id_modify" if *ui_mode == UiMode::Profiles => {
                        popup.0 = Some(("profiles.mnu".into(), "idd_profilemodify".into()));
                    }
                    "id_delete" if *ui_mode == UiMode::Profiles => {
                        popup.0 = Some(("profiles.mnu".into(), "idd_profiledeleteconfirm".into()));
                    }
                    "id_delete" if *ui_mode == UiMode::Replays => {
                        popup.0 = Some(("viewreplays.mnu".into(), "idd_confirm_replay_delete".into()));
                    }
                    "id_back" => {
                        if settings {
                            // Back leaves the settings without keeping the changes.
                            if let Some(p) = profile.as_mut() {
                                **p = Profile::load();
                            }
                        }
                        *ui_mode = match *ui_mode {
                            UiMode::Settings => ui_state.settings_return.take().unwrap_or(if in_menu_scene { UiMode::MainMenu } else { UiMode::Pit }),
                            UiMode::BikeChoose => UiMode::TestSetup,
                            // Leaving the testing session asks first (`idd_testingexit`).
                            UiMode::Pit => {
                                popup.0 = Some(("test.mnu".into(), "idd_testingexit".into()));
                                UiMode::Pit
                            }
                            _ if in_menu_scene => UiMode::MainMenu,
                            _ => UiMode::Pit,
                        };
                    }
                    "id_yes" if popup.0.as_ref().is_some_and(|p| p.1 == "idd_testingexit") => {
                        popup.0 = None;
                        *ui_mode = UiMode::MainMenu;
                    }
                    "id_yes" if popup.0.as_ref().is_some_and(|p| p.1 == "idd_confirm_replay_delete" || p.1 == "idd_profiledeleteconfirm") => {
                        popup.0 = None;
                    }
                    "id_yes" if popup.0.as_ref().is_some_and(|p| p.1 == "idd_setup_delete") => {
                        if let Some(s) = setup.as_mut() {
                            if s.cur_setup_name != "-Default-" {
                                let path = format!("setups/{}/{}.stp", s.bike_id, s.cur_setup_name);
                                let _ = std::fs::remove_file(&path);
                                s.reset_to_default();
                            }
                        }
                        popup.0 = None;
                    }
                    "id_exit" => popup.0 = Some(("main.mnu".into(), "exit_confirm".into())),
                    "id_yes" if popup.0.as_ref().is_some_and(|p| p.1 == "exit_confirm") => {
                        exit.send(AppExit::Success);
                    }
                    "id_no" | "id_close" | "id_cancel" => {
                        popup.0 = None;
                        ui_state.capture = None;
                        ui_state.edit_focus = None;
                    }
                    item if settings && crate::input::CONTROL_ITEMS.iter().any(|c| c.0 == item) => {
                        ui_state.capture = Some((item.to_string(), 0));
                        popup.0 = Some(("options.mnu".into(), "idd_wait_control".into()));
                    }
                    _ => {}
                }
            }
        }
        if save_now {
            if let Some(p) = profile.as_ref().filter(|p| p.is_changed()) {
                p.save();
            }
        }
    }
}

/// Places the pointer at the mouse in menu modes.
pub fn update_cursor(windows: Query<&Window>, ui_mode: Res<UiMode>, mut cursor_query: Query<&mut Node, With<MnuCursor>>) {
    let Ok(window) = windows.get_single() else { return };
    let Ok(mut node) = cursor_query.get_single_mut() else { return };
    if *ui_mode == UiMode::None {
        node.display = Display::None;
        return;
    }
    if let Some(pos) = window.cursor_position() {
        node.display = Display::Flex;
        node.left = Val::Px(pos.x);
        node.top = Val::Px(pos.y);
    }
}

/// Waits for a key while `idd_wait_control` shows and binds it (two keys for two-key axes);
/// Escape cancels. The bindings are saved with the profile.
pub fn capture_control(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut popup: ResMut<Popup>,
    bindings: Option<ResMut<crate::input::Bindings>>,
    profile: Option<ResMut<Profile>>,
) {
    let Some((item, step)) = ui_state.capture.clone() else { return };
    let (Some(mut bindings), Some(mut profile)) = (bindings, profile) else { return };
    let Some(&key) = keys.get_just_pressed().next() else { return };
    if key == KeyCode::Escape {
        ui_state.capture = None;
        popup.0 = None;
        return;
    }
    let Some(code) = crate::input::key_dik(key) else { return };
    bindings.assign(&item, step, code);
    if step == 0 && crate::input::Bindings::is_two_key(&item) {
        ui_state.capture = Some((item, 1));
    } else {
        ui_state.capture = None;
        popup.0 = None;
        bindings.save(&mut profile);
        profile.save();
    }
}

/// Typing input for active editbox (`ui_state.edit_focus`).
pub fn handle_editbox_input(
    mut ui_state: ResMut<UiState>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if ui_state.edit_focus.is_none() {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for &key in keys.get_just_pressed() {
        match key {
            KeyCode::Backspace => {
                ui_state.edit_buffer.pop();
            }
            KeyCode::Space => {
                ui_state.edit_buffer.push(' ');
            }
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                ui_state.edit_buffer.push(if shift { '_' } else { '-' });
            }
            KeyCode::Period | KeyCode::NumpadDecimal => {
                ui_state.edit_buffer.push('.');
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if ui_state.edit_focus.as_deref() == Some("id_edit") {
                    ui_state.edit_buffer.push('\n');
                }
            }
            _ => {
                let ch = match key {
                    KeyCode::KeyA => Some(if shift { 'A' } else { 'a' }),
                    KeyCode::KeyB => Some(if shift { 'B' } else { 'b' }),
                    KeyCode::KeyC => Some(if shift { 'C' } else { 'c' }),
                    KeyCode::KeyD => Some(if shift { 'D' } else { 'd' }),
                    KeyCode::KeyE => Some(if shift { 'E' } else { 'e' }),
                    KeyCode::KeyF => Some(if shift { 'F' } else { 'f' }),
                    KeyCode::KeyG => Some(if shift { 'G' } else { 'g' }),
                    KeyCode::KeyH => Some(if shift { 'H' } else { 'h' }),
                    KeyCode::KeyI => Some(if shift { 'I' } else { 'i' }),
                    KeyCode::KeyJ => Some(if shift { 'J' } else { 'j' }),
                    KeyCode::KeyK => Some(if shift { 'K' } else { 'k' }),
                    KeyCode::KeyL => Some(if shift { 'L' } else { 'l' }),
                    KeyCode::KeyM => Some(if shift { 'M' } else { 'm' }),
                    KeyCode::KeyN => Some(if shift { 'N' } else { 'n' }),
                    KeyCode::KeyO => Some(if shift { 'O' } else { 'o' }),
                    KeyCode::KeyP => Some(if shift { 'P' } else { 'p' }),
                    KeyCode::KeyQ => Some(if shift { 'Q' } else { 'q' }),
                    KeyCode::KeyR => Some(if shift { 'R' } else { 'r' }),
                    KeyCode::KeyS => Some(if shift { 'S' } else { 's' }),
                    KeyCode::KeyT => Some(if shift { 'T' } else { 't' }),
                    KeyCode::KeyU => Some(if shift { 'U' } else { 'u' }),
                    KeyCode::KeyV => Some(if shift { 'V' } else { 'v' }),
                    KeyCode::KeyW => Some(if shift { 'W' } else { 'w' }),
                    KeyCode::KeyX => Some(if shift { 'X' } else { 'x' }),
                    KeyCode::KeyY => Some(if shift { 'Y' } else { 'y' }),
                    KeyCode::KeyZ => Some(if shift { 'Z' } else { 'z' }),
                    KeyCode::Digit0 | KeyCode::Numpad0 => Some('0'),
                    KeyCode::Digit1 | KeyCode::Numpad1 => Some('1'),
                    KeyCode::Digit2 | KeyCode::Numpad2 => Some('2'),
                    KeyCode::Digit3 | KeyCode::Numpad3 => Some('3'),
                    KeyCode::Digit4 | KeyCode::Numpad4 => Some('4'),
                    KeyCode::Digit5 | KeyCode::Numpad5 => Some('5'),
                    KeyCode::Digit6 | KeyCode::Numpad6 => Some('6'),
                    KeyCode::Digit7 | KeyCode::Numpad7 => Some('7'),
                    KeyCode::Digit8 | KeyCode::Numpad8 => Some('8'),
                    KeyCode::Digit9 | KeyCode::Numpad9 => Some('9'),
                    _ => None,
                };
                if let Some(c) = ch {
                    if ui_state.edit_focus.as_deref() != Some("id_setupname") || ui_state.edit_buffer.len() < 31 {
                        ui_state.edit_buffer.push(c);
                    }
                }
            }
        }
    }
}

