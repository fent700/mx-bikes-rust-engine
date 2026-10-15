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
) {
    let Ok(mut text) = q.get_single_mut() else { return };
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

    text.0 = format!(
        "{:5.1} km/h   gear {gear}   {:5.0} rpm{}{}{}\nlean {:5.1} deg   fork {:3.0} mm   shock {:3.0} mm   rear slip {:+.2}   {surface}{}{timing_str}",
        b.speed_kmh,
        b.powertrain.rpm(),
        if b.powertrain.stalled { "   STALLED (Enter)" } else { "" },
        if b.powertrain.limiter_cut { "   LIMITER" } else { "" },
        dirt_str,
        b.lean_angle_deg,
        b.fork_compression * 1000.0,
        b.shock_compression() * 1000.0,
        b.rear_telemetry().slip_ratio,
        if b.crashed { "\nCRASHED - press R" } else { "" },
    );
}
