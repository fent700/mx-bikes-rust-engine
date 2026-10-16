//! In-game HUD matching PiBoSo's MX Bikes layout.
//!
//! The game's HUD is driven by `misc\hud\hud.cfg` (loaded in `FUN_140010bb0`). In the default
//! configuration the only visible elements during riding are:
//!
//! - **Gear indicator**: a dark box at (0.894, 0.88) with the gear number centred at (0.9, 0.9)
//!   in `lcd.fnt` at size 0.07, black text.
//! - **Camera mode label**: centred at (0.5, ~0.85), white text over a dark bar, fading out over
//!   1 second after switching views (`FUN_140010600`).
//! - **Speed and lap timing**: shown below/beside the gear box in the full HUD (when
//!   `view/show_HUD` is enabled for onboard, always visible for chase).
//! - **FPS counter**: top-left, only when `misc/showfps` is set.

use crate::bike_entity::BikeSim;
use crate::camera::{CameraMode, FollowCamera};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Marker components
// ---------------------------------------------------------------------------

/// The gear number text ("1", "2", "N").
#[derive(Component)]
pub struct GearText;

/// Speed readout ("123 km/h").
#[derive(Component)]
pub struct SpeedText;

/// Lap / timing readout.
#[derive(Component)]
pub struct TimingText;

/// FPS counter (top-left, toggleable).
#[derive(Component)]
pub struct FpsText;

/// Camera mode label ("External View", etc.), centred, fades out.
#[derive(Component)]
pub struct CameraLabel;

/// Dark background behind the gear indicator.
#[derive(Component)]
pub struct GearBackground;

/// Dark background behind the camera label.
#[derive(Component)]
pub struct CameraLabelBg;

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Tracks which camera mode was active last frame so we can detect switches.
#[derive(Resource)]
pub struct CameraLabelState {
    pub prev_mode: Option<CameraMode>,
    /// Remaining fade time in seconds (starts at 1.0).
    pub fade_timer: f32,
}

impl Default for CameraLabelState {
    fn default() -> Self {
        Self { prev_mode: None, fade_timer: 0.0 }
    }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

pub fn setup_hud(mut commands: Commands) {
    commands.init_resource::<CameraLabelState>();

    // --- Gear background (dark semi-transparent box, bottom-right) ---
    // The game's `hud.cfg` places the container at (0.894, 0.88), size (0.03, 0.06).
    // We convert normalised screen coords to Bevy absolute positioning by using percentages.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            // 0.894 from left edge → right: ~10.6%, and 0.88 from top → bottom: ~12%
            right: Val::Percent(7.0),
            bottom: Val::Percent(6.0),
            width: Val::Percent(5.0),
            height: Val::Percent(10.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        GearBackground,
    ))
    .with_children(|parent| {
        // Gear number — large, centred inside the box.
        parent.spawn((
            Text::new("N"),
            TextFont {
                font_size: 48.0,
                ..default()
            },
            TextColor(Color::WHITE),
            GearText,
        ));
    });

    // --- Speed text (to the left of the gear box) ---
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Percent(13.0),
            bottom: Val::Percent(9.0),
            ..default()
        },
        SpeedText,
    ));

    // --- Lap timing (bottom-centre) ---
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            bottom: Val::Percent(2.0),
            ..default()
        },
        TimingText,
    ));

    // --- FPS counter (top-left, hidden by default) ---
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 0.0, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        },
        FpsText,
    ));

    // --- Camera mode label background (centred, hidden until a switch) ---
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(83.0),
            padding: UiRect::axes(Val::Px(16.0), Val::Px(6.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            // Shift left by half its own width (approximation via negative margin).
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        CameraLabelBg,
    ))
    .with_children(|parent| {
        parent.spawn((
            Text::new(""),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
            CameraLabel,
        ));
    });
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

fn format_time(t: f32) -> String {
    let mins = (t / 60.0) as u32;
    let secs = (t % 60.0) as u32;
    let millis = ((t * 1000.0) % 1000.0) as u32;
    if mins > 0 {
        format!("{mins}'{secs:02}.{millis:03}")
    } else {
        format!("{secs}.{millis:03}")
    }
}

pub fn update_hud(
    sim: Res<BikeSim>,
    timer: Option<Res<crate::world::LapTimer>>,
    ui_mode: Option<Res<crate::ui::UiMode>>,
    profile: Option<Res<crate::profile::Profile>>,
    cams: Query<&FollowCamera>,
    time: Res<Time>,
    mut fps_avg: Local<f32>,
    mut cam_label_state: ResMut<CameraLabelState>,
    // A single query over all HUD-marked entities.
    mut texts: ParamSet<(
        Query<&mut Text, With<GearText>>,
        Query<&mut Text, With<SpeedText>>,
        Query<&mut Text, With<TimingText>>,
        Query<&mut Text, With<FpsText>>,
        Query<(&mut Text, &mut TextColor), With<CameraLabel>>,
    )>,
    mut bg_q: ParamSet<(
        Query<&mut BackgroundColor, With<CameraLabelBg>>,
        Query<&mut Visibility, With<GearBackground>>,
        Query<&mut Visibility, With<SpeedText>>,
        Query<&mut Visibility, With<TimingText>>,
    )>,
) {
    let dt = time.delta_secs();
    let pi = |s: &str, k: &str| profile.as_ref().map(|p| p.i(s, k)).unwrap_or(0);

    // Determine if HUD should be hidden (menu open).
    let in_menu = ui_mode.as_ref().is_some_and(|m| **m != crate::ui::UiMode::None);

    // Determine camera mode.
    let cam = cams.get_single().ok();
    let onboard = cam.is_some_and(|c| matches!(c.mode, CameraMode::Onboard));
    let in_showroom = cam.is_some_and(|c| matches!(c.mode, CameraMode::Showroom(_)));

    // The HUD is hidden in menus and in the showroom view.
    let hud_hidden = in_menu || in_showroom;
    // In the helmet (onboard) view, the HUD only shows if `view/show_HUD` is set.
    let hud_visible = !hud_hidden && (!onboard || pi("view", "show_hud") != 0);

    let b = &sim.0;

    // --- Gear indicator ---
    {
        let mut q = texts.p0();
        if let Ok(mut gear_text) = q.get_single_mut() {
            gear_text.0 = match b.powertrain.gear {
                0 => "N".into(),
                g => g.to_string(),
            };
        }
    }

    // --- Speed ---
    let (speed, unit) = if pi("misc", "units") == 0 {
        (b.speed_kmh, "km/h")
    } else {
        (b.speed_kmh / 1.609_344, "mph")
    };
    {
        let mut q = texts.p1();
        if let Ok(mut speed_text) = q.get_single_mut() {
            speed_text.0 = format!("{:.0} {unit}", speed);
        }
    }

    // --- Lap timing ---
    {
        let mut q = texts.p2();
        if let Ok(mut timing_text) = q.get_single_mut() {
            if let Some(t) = timer.as_ref() {
                let cur = format_time(t.current_time);
                let best = t.best_time.map_or("--:--.---".into(), format_time);
                let last = t.last_lap_time.map_or("--:--.---".into(), format_time);
                timing_text.0 = format!("Lap {}  {cur}  Best {best}  Last {last}", t.lap);
            } else {
                timing_text.0.clear();
            }
        }
    }

    // --- FPS ---
    if dt > 0.0 {
        *fps_avg = if *fps_avg <= 0.0 { 1.0 / dt } else { *fps_avg * 0.95 + 0.05 / dt };
    }
    {
        let mut q = texts.p3();
        if let Ok(mut fps_text) = q.get_single_mut() {
            if pi("misc", "showfps") != 0 && !in_menu {
                fps_text.0 = format!("{:.0} fps", *fps_avg);
            } else {
                fps_text.0.clear();
            }
        }
    }

    // --- Camera mode label (fade logic from FUN_140010600) ---
    // Detect camera switches.
    if let Some(c) = cam {
        let current = c.mode;
        let switched = cam_label_state.prev_mode.as_ref().is_some_and(|prev| {
            std::mem::discriminant(prev) != std::mem::discriminant(&current)
        });
        if switched {
            cam_label_state.fade_timer = 1.5;
        }
        cam_label_state.prev_mode = Some(current);
    }

    // Tick the fade timer.
    if cam_label_state.fade_timer > 0.0 {
        cam_label_state.fade_timer -= dt;
        if cam_label_state.fade_timer < 0.0 {
            cam_label_state.fade_timer = 0.0;
        }
    }

    {
        let mut q = texts.p4();
        if let Ok((mut label_text, mut label_color)) = q.get_single_mut() {
            if cam_label_state.fade_timer > 0.0 && !in_menu {
                let label = match cam.map(|c| &c.mode) {
                    Some(CameraMode::Chase) => "External View",
                    Some(CameraMode::Onboard) => "Static Onboard View",
                    Some(CameraMode::Side) => "Side View",
                    Some(CameraMode::Left) => "Left View",
                    Some(CameraMode::Showroom(_)) => "",
                    None => "",
                };
                label_text.0 = format!(" {label} ");
                let alpha = if cam_label_state.fade_timer > 0.5 {
                    1.0
                } else {
                    cam_label_state.fade_timer / 0.5
                };
                label_color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
            } else {
                label_text.0.clear();
                label_color.0 = Color::srgba(1.0, 1.0, 1.0, 0.0);
            }
        }
    }

    // Camera label background alpha.
    {
        let mut q = bg_q.p0();
        if let Ok(mut bg_color) = q.get_single_mut() {
            let alpha = if cam_label_state.fade_timer > 0.5 {
                0.5
            } else if cam_label_state.fade_timer > 0.0 {
                cam_label_state.fade_timer / 0.5 * 0.5
            } else {
                0.0
            };
            bg_color.0 = Color::srgba(0.0, 0.0, 0.0, alpha);
        }
    }

    // --- Visibility ---
    let vis = if hud_visible { Visibility::Inherited } else { Visibility::Hidden };
    {
        let mut q = bg_q.p1();
        if let Ok(mut v) = q.get_single_mut() { *v = vis; }
    }
    {
        let mut q = bg_q.p2();
        if let Ok(mut v) = q.get_single_mut() { *v = vis; }
    }
    {
        let mut q = bg_q.p3();
        if let Ok(mut v) = q.get_single_mut() { *v = vis; }
    }
}

