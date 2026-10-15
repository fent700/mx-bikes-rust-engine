//! Telemetry overlay.

use crate::bike_entity::BikeSim;
use bevy::prelude::*;

#[derive(Component)]
pub struct HudText;

pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(14.0),
            bottom: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
}

fn format_time(t: f32) -> String {
    let mins = (t / 60.0) as u32;
    let secs = (t % 60.0) as u32;
    let centis = ((t * 100.0) % 100.0) as u32;
    format!("{mins:02}:{secs:02}.{centis:02}")
}

pub fn update_hud(
    sim: Res<BikeSim>,
    timer: Option<Res<crate::world::LapTimer>>,
    ui_mode: Option<Res<crate::ui::UiMode>>,
    mut q: Query<&mut Text, With<HudText>>,
    profile: Option<Res<crate::profile::Profile>>,
    cams: Query<&crate::camera::FollowCamera>,
    time: Res<Time>,
    mut fps: Local<f32>,
) {
    let Ok(mut text) = q.get_single_mut() else { return };
    let dt = time.delta_secs();
    if dt > 0.0 {
        *fps = if *fps <= 0.0 { 1.0 / dt } else { *fps * 0.95 + 0.05 / dt };
    }
    let pi = |s: &str, k: &str| profile.as_ref().map(|p| p.i(s, k)).unwrap_or(0);
    let fps_line = if pi("misc", "showfps") != 0 { format!("{:.0} fps
", *fps) } else { String::new() };
    // In the helmet view the HUD shows only with `view/show_HUD` (FUN_1400f32b0).
    let onboard = cams.get_single().is_ok_and(|c| matches!(c.mode, crate::camera::CameraMode::Onboard));
    if onboard && pi("view", "show_hud") == 0 {
        text.0 = fps_line;
        return;
    }
    if let Some(mode) = ui_mode {
        if *mode != crate::ui::UiMode::None {
            text.0.clear();
            return;
        }
    }
    let b = &sim.0;
    let gear = match b.powertrain.gear {
        0 => "N".to_string(),
        g => g.to_string(),
    };
    let surface = b
        .rear_telemetry()
        .surface
        .map(|s| s.tyre_key())
        .unwrap_or("air");

    let timing_str = if let Some(t) = timer {
        let cur = format_time(t.current_time);
        let best = t.best_time.map_or("--:--.--".into(), format_time);
        let last = t.last_lap_time.map_or("--:--.--".into(), format_time);
        let splits: String = t
            .split_times
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|st| format!(" S{}: {}", i + 1, format_time(st))))
            .collect();
        format!("\nlap {}   time {cur}   best {best}   last {last}{splits}", t.lap)
    } else {
        String::new()
    };

    let dirt_str = if b.dirt_level() > 0 {
        format!("   mud stage {}", b.dirt_level())
    } else {
        String::new()
    };

    let stand_str = if pi("view", "show_riderstand") != 0 {
        if b.rider.stand > 0.5 { "   [STAND]" } else { "   [SIT]" }
    } else {
        ""
    };

    // `misc/units`: 0 metric, 1 US and 2 UK both show miles per hour.
    let (speed, unit) = if pi("misc", "units") == 0 { (b.speed_kmh, "km/h") } else { (b.speed_kmh / 1.609_344, "mph") };
    text.0 = fps_line + &format!(
        "{:5.1} {unit}   gear {gear}   {:5.0} rpm{}{}{}{}\nlean {:5.1} deg   fork {:3.0} mm   shock {:3.0} mm   rear slip {:+.2}   {surface}{}{timing_str}",
        speed,
        b.powertrain.rpm(),
        if b.powertrain.stalled { "   STALLED (Enter)" } else { "" },
        if b.powertrain.limiter_cut { "   LIMITER" } else { "" },
        stand_str,
        dirt_str,
        b.lean_angle_deg,
        b.fork_compression * 1000.0,
        b.shock_compression() * 1000.0,
        b.rear_telemetry().slip_ratio,
        if b.crashed { "\nCRASHED - press R" } else { "" },
    );
}
