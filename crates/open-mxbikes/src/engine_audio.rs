//! Bike sounds from the bike's `sfx.cfg` and `.scl` files, mixed as the game does
//! ([`mxbikes_formats::sfx`]): engine on / off throttle layers with the limiter, starter,
//! transmission whine and gear shift one-shots.
//!
//! Every sample loops with its volume and playback rate (pitch) set each frame. Distance
//! attenuation is OpenAL's inverse distance clamped model with each sound's `min_distance`, from
//! the camera to the sound's position on the bike. The master level is the profile's
//! `sound/master_volume`.
//!
//! INFERRED: the engine volume input of `FUN_140039640` is taken as the master volume; the
//! limiter amount is 1 while the limiter cuts; the transmission is driven by the gearbox output
//! speed and the rear wheel torque within the sfx `torque` range; sound positions are not
//! panned (Bevy's non-spatial playback).

use crate::bike_entity::BikeSim;
use crate::profile::Profile;
use crate::world::{LaunchOptions, Vfs};
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;
use mxbikes_formats::sfx::{
    distance_gain, tyre_set_for_surface, BikeSfxConfig, ImpactsConfig, SoundCurveLayout, SoundPos, TyreSound, TYRE_SETS,
};
use mxbikes_formats::ConfigNode;
use std::collections::HashMap;
use std::sync::Arc;

/// Which sound a looping voice belongs to.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum Voice {
    Engine { layer: usize, sample: usize },
    Limiter,
    Starter,
    Transmission { layer: usize, sample: usize },
    /// Brake sample of wheel 0 (front) or 1 (rear).
    Brake { wheel: usize, sample: usize },
    /// Tyre sound sample: wheel, surface set, wet, slide (else roll).
    Tyre { wheel: usize, set: usize, wet: bool, slide: bool, sample: usize },
    /// Aerodynamic wind noise layer.
    Aero { sample: usize },
    /// Chassis/rider ground scrape sound.
    Scrape,
}

#[derive(Resource)]
pub struct BikeSounds {
    cfg: BikeSfxConfig,
    engine: SoundCurveLayout,
    transmission: Option<SoundCurveLayout>,
    brakes: Option<SoundCurveLayout>,
    /// Tyre sounds per set: [dry roll, dry slide, wet roll, wet slide].
    tyres: Vec<[Option<TyreSound>; 4]>,
    shift: Option<Handle<AudioSource>>,
    shift_out: Option<Handle<AudioSource>>,
    last_gear: usize,
    aero: Option<SoundCurveLayout>,
    #[allow(dead_code)]
    impacts: Option<ImpactsConfig>,
    impact_handles: HashMap<String, Vec<Handle<AudioSource>>>,
    #[allow(dead_code)]
    scrape_handles: HashMap<String, Handle<AudioSource>>,
}

fn load(vfs: &mxbikes_vfs::MxBikesVfs, audio: &mut Assets<AudioSource>, path: &str) -> Option<Handle<AudioSource>> {
    let bytes = vfs.read(path).ok()?;
    // Only RIFF/WAVE data reaches the decoder; anything else would stop Bevy's audio thread.
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        warn!("sound {path}: not a WAVE file");
        return None;
    }
    Some(audio.add(AudioSource { bytes: Arc::from(bytes.into_boxed_slice()) }))
}

fn looping(commands: &mut Commands, handle: Handle<AudioSource>, voice: Voice) {
    commands.spawn((
        AudioPlayer::new(handle),
        PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::new(0.0), ..default() },
        voice,
        crate::world::WorldEntity,
    ));
}

pub fn setup_bike_sounds(mut commands: Commands, vfs: Res<Vfs>, options: Res<LaunchOptions>, mut audio: ResMut<Assets<AudioSource>>) {
    setup_bike_sounds_internal(&mut commands, &vfs.0, &options.bike, &mut audio);
}

pub fn setup_bike_sounds_internal(
    commands: &mut Commands,
    vfs: &mxbikes_vfs::MxBikesVfs,
    bike: &str,
    audio: &mut Assets<AudioSource>,
) {
    let dir = format!("bikes/{bike}");
    let Some(cfg) = vfs
        .read(&format!("{dir}/sfx.cfg"))
        .ok()
        .and_then(|d| ConfigNode::parse(&String::from_utf8_lossy(&d)).ok())
        .map(|n| BikeSfxConfig::parse(&n))
    else {
        warn!("{dir}/sfx.cfg not found");
        return;
    };
    // Files the bike folder does not have come from the shared `effects/sounds` folder.
    let scl = |name: &str| {
        vfs.read(&format!("{dir}/{name}"))
            .or_else(|_| vfs.read(&format!("effects/sounds/{name}")))
            .ok()
            .and_then(|d| ConfigNode::parse(&String::from_utf8_lossy(&d)).ok())
            .map(|n| SoundCurveLayout::parse(&n))
    };
    let any = |audio: &mut Assets<AudioSource>, w: &str| {
        if vfs.exists(&format!("{dir}/{w}")) {
            load(vfs, audio, &format!("{dir}/{w}"))
        } else {
            load(vfs, audio, &format!("effects/sounds/{w}"))
        }
    };
    let engine = scl(&cfg.engine.scl_file).unwrap_or(SoundCurveLayout { layers: Vec::new() });
    for (l, layer) in engine.layers.iter().enumerate().take(2) {
        for (s, sample) in layer.samples.iter().enumerate() {
            if let Some(h) = any(audio, &sample.wav) {
                looping(commands, h, Voice::Engine { layer: l, sample: s });
            }
        }
    }
    if let Some(h) = cfg.engine.limiter_wav.as_deref().and_then(|w| any(audio, w)) {
        looping(commands, h, Voice::Limiter);
    }
    if let Some(h) = cfg.starter_wav.as_deref().and_then(|w| any(audio, w)) {
        looping(commands, h, Voice::Starter);
    }
    let transmission = cfg.transmission_scl.as_deref().and_then(scl);
    if let Some(t) = &transmission {
        for (l, layer) in t.layers.iter().enumerate().take(2) {
            for (s, sample) in layer.samples.iter().enumerate() {
                if let Some(h) = any(audio, &sample.wav) {
                    looping(commands, h, Voice::Transmission { layer: l, sample: s });
                }
            }
        }
    }
    let brakes = cfg.brakes_scl.as_deref().and_then(scl);
    if let Some(b) = &brakes {
        for wheel in 0..2 {
            for (s, sample) in b.layers.first().map(|l| l.samples.as_slice()).unwrap_or(&[]).iter().enumerate() {
                if let Some(h) = any(audio, &sample.wav) {
                    looping(commands, h, Voice::Brake { wheel, sample: s });
                }
            }
        }
    }
    // Tyre sound sets (`effects/sounds/sets/<tyres>`), loaded in the game's order.
    let set_cfg = cfg
        .tyres
        .as_deref()
        .and_then(|f| vfs.read(&format!("effects/sounds/sets/{f}")).ok())
        .and_then(|d| ConfigNode::parse(&String::from_utf8_lossy(&d)).ok());
    let mut tyres = Vec::new();
    for (set, name) in TYRE_SETS.iter().enumerate() {
        let mut entry: [Option<TyreSound>; 4] = [None, None, None, None];
        for (k, (cond, kind)) in [("dry", "roll"), ("dry", "slide"), ("wet", "roll"), ("wet", "slide")].iter().enumerate() {
            let Some(n) = set_cfg.as_ref().and_then(|c| c.path(&format!("{name}/{cond}/{kind}"))) else { continue };
            let Some(layout) = n.get_str("scl").and_then(scl) else { continue };
            let f = |key: &str, d: f32| n.get_f32(key).unwrap_or(d);
            let sound = TyreSound {
                scl: layout,
                speed_factor: f("speed_factor", 0.0),
                ref_speed: f("ref_speed", 1.0),
                pitch_factor: f("pitch_factor", 0.0),
                longslip_factor: f("longslip_factor", 1.0),
                latslip_factor: if *kind == "slide" { f("latslip_factor", 1.0) } else { 0.0 },
                min_distance: f("min_distance", 1.0),
                rolloff: f("rolloff_factor", 1.0),
            };
            for wheel in 0..2 {
                for (si, sample) in sound.scl.layers.first().map(|l| l.samples.as_slice()).unwrap_or(&[]).iter().enumerate() {
                    if let Some(h) = any(audio, &sample.wav) {
                        looping(commands, h, Voice::Tyre { wheel, set, wet: k >= 2, slide: k % 2 == 1, sample: si });
                    }
                }
            }
            entry[k] = Some(sound);
        }
        tyres.push(entry);
    }
    let aero_scl = scl("aerodynamic_onboard.scl");
    if let Some(aero) = &aero_scl {
        for (s, sample) in aero.layers.first().map(|l| l.samples.as_slice()).unwrap_or(&[]).iter().enumerate() {
            if let Some(h) = any(audio, &sample.wav) {
                looping(commands, h, Voice::Aero { sample: s });
            }
        }
    }
    let impacts_cfg = vfs
        .read("effects/sounds/sets/impacts.cfg")
        .ok()
        .and_then(|d| ConfigNode::parse(&String::from_utf8_lossy(&d)).ok())
        .map(|n| ImpactsConfig::parse(&n));
    let mut impact_handles: HashMap<String, Vec<Handle<AudioSource>>> = HashMap::new();
    let mut scrape_handles: HashMap<String, Handle<AudioSource>> = HashMap::new();
    if let Some(imp) = &impacts_cfg {
        for (set_name, set) in &imp.sets {
            let mut list = Vec::new();
            for s in &set.impacts {
                if let Some(h) = any(audio, &s.wav) {
                    list.push(h);
                }
            }
            if !list.is_empty() {
                impact_handles.insert(set_name.clone(), list);
            }
            if let Some(sc) = &set.scrape {
                if let Some(h) = any(audio, &sc.wav) {
                    scrape_handles.insert(set_name.clone(), h);
                }
            }
        }
    }
    let default_scrape = scrape_handles
        .get("concrete")
        .or_else(|| scrape_handles.get("grass"))
        .or_else(|| scrape_handles.values().next())
        .cloned();
    if let Some(h) = default_scrape {
        looping(commands, h, Voice::Scrape);
    }
    let shift = cfg.gearshift_wav.as_deref().and_then(|w| any(audio, w));
    let shift_out = cfg.gearout_wav.as_deref().and_then(|w| any(audio, w));
    info!(
        "bike sounds: {} engine samples, transmission {}, brakes {}, tyre sets {}, aero {}, impacts {}, shift {}",
        engine.layers.iter().map(|l| l.samples.len()).sum::<usize>(),
        transmission.is_some(),
        brakes.is_some(),
        tyres.iter().filter(|t| t.iter().any(Option::is_some)).count(),
        aero_scl.is_some(),
        impacts_cfg.is_some(),
        shift.is_some()
    );
    commands.insert_resource(BikeSounds {
        cfg,
        engine,
        transmission,
        brakes,
        tyres,
        shift,
        shift_out,
        last_gear: 0,
        aero: aero_scl,
        impacts: impacts_cfg,
        impact_handles,
        scrape_handles,
    });
}

fn gain_at(place: &SoundPos, bike: &mxbikes_physics::Motorcycle, listener: Vec3, listener_right: Dir3) -> f32 {
    let (p, r) = bike.chassis_transform();
    // .cfg positions are left handed: z flips.
    let world = p + r * Vec3::new(place.pos[0], place.pos[1], -place.pos[2]);
    let dist = world.distance(listener);
    let base_gain = distance_gain(dist, place.min_distance.max(0.01), place.rolloff);
    if dist > 0.05 {
        let azimuth = ((world - listener) / dist).dot(*listener_right).clamp(-1.0, 1.0);
        base_gain * (1.0 - 0.15 * azimuth.abs())
    } else {
        base_gain
    }
}

pub fn update_bike_sounds(
    mut commands: Commands,
    sim: Option<Res<BikeSim>>,
    sounds: Option<ResMut<BikeSounds>>,
    profile: Option<Res<Profile>>,
    ui_mode: Option<Res<crate::ui::UiMode>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    voices: Query<(&Voice, &AudioSink)>,
) {
    let (Some(sim), Some(mut sounds)) = (sim, sounds) else { return };
    let bike = &sim.0;
    let pt = &bike.powertrain;
    let cam_tf = camera.get_single();
    let listener = cam_tf.as_ref().map(|c| c.translation()).unwrap_or(Vec3::ZERO);
    let listener_right = cam_tf.as_ref().map(|c| c.right()).unwrap_or(Dir3::X);
    let master = profile.as_ref().map(|p| p.f("sound", "master_volume")).unwrap_or(0.5);
    // The game pauses its sounds with the simulation while a menu is open.
    let paused = ui_mode.is_some_and(|m| *m != crate::ui::UiMode::None);
    let cfg = &sounds.cfg;
    let engine_gain = master * gain_at(&cfg.engine.place, bike, listener, listener_right);
    let has_off = sounds.engine.layers.len() > 1;
    let (on, off, limiter) = cfg.engine_gains(pt.throttle.clamp(0.0, 1.0), if pt.limiter_cut { 1.0 } else { 0.0 }, has_off);
    let v = pt.rpm() / cfg.engine.max_rpm.max(1.0);
    let levels: Vec<_> = sounds.engine.layer_levels(0, v, on).into_iter().chain(sounds.engine.layer_levels(1, v, off)).collect();
    let (t_levels, t_gain) = match &sounds.transmission {
        Some(t) => {
            let final_ratio = bike.def.driveline.final_ratio();
            let shaft_rpm = bike.rear_omega().abs() * final_ratio * 60.0 / std::f32::consts::TAU;
            let tv = shaft_rpm / cfg.transmission_max_rpm.max(1.0);
            let (lo, hi) = cfg.transmission_torque;
            let w = ((pt.wheel_torque.abs() - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0);
            let levels = t.layer_levels(0, tv, w).into_iter().chain(t.layer_levels(1, tv, 1.0 - w)).collect::<Vec<_>>();
            (levels, master * gain_at(&cfg.transmission, bike, listener, listener_right))
        }
        None => (Vec::new(), 0.0),
    };
    // Brakes (`FUN_140051b90`): curve at wheel rev/s / max_rps, gain the brake input times the
    // profile brakes volume. Tyres: the set of the contact surface (dry), roll and slide.
    let brakes_volume = profile.as_ref().map(|p| p.f("sound", "brakes_volume")).unwrap_or(1.0);
    let wheels = [bike.front_telemetry(), bike.rear_telemetry()];
    let wheel_pos = [bike.front_wheel_transform().0, bike.rear_wheel_transform().0];
    let wheel_gain = |i: usize, min_distance: f32, rolloff: f32| distance_gain(wheel_pos[i].distance(listener), min_distance.max(0.01), rolloff);
    let brake_in = [bike.input.front_brake, bike.input.rear_brake];
    let brake_levels: Vec<Vec<_>> = (0..2)
        .map(|w| match &sounds.brakes {
            Some(b) => {
                let rps = if w == 0 { bike.front_omega() } else { bike.rear_omega() }.abs() / std::f32::consts::TAU;
                let v = (rps / sounds.cfg.brakes_max_rps.max(1e-3)).clamp(0.0, 1.0);
                b.layer_levels(0, v, brake_in[w].clamp(0.0, 1.0) * master * brakes_volume * wheel_gain(w, sounds.cfg.brakes.min_distance, sounds.cfg.brakes.rolloff))
            }
            None => Vec::new(),
        })
        .collect();
    let tyre_levels: Vec<Option<(usize, Vec<(bool, Vec<mxbikes_formats::sfx::SampleLevel>, f32)>)>> = (0..2)
        .map(|w| {
            let t = &wheels[w];
            t.contact?;
            let set = tyre_set_for_surface(t.surface?.terrain_id())?;
            let entry = sounds.tyres.get(set)?;
            // INFERRED: speeds in km/h (the sets use ref_speed 100) and slip measures scaled by
            // the load over 1000 N.
            let ground = (t.velocity - t.normal * t.velocity.dot(t.normal)).length() * 3.6;
            let rim = t.rim_speed * 3.6;
            let load = t.load / 1000.0;
            let mut out = Vec::new();
            for (k, slide) in [(0usize, false), (1, true)] {
                if let Some(snd) = &entry[k] {
                    let (gain, pitch, value) = snd.levels(rim, ground, t.slip_ratio, t.slip_angle, load);
                    let g = gain * master * wheel_gain(w, snd.min_distance, snd.rolloff);
                    out.push((slide, snd.scl.layer_levels(0, value, g), pitch));
                }
            }
            Some((set, out))
        })
        .collect();

    // Aerodynamic wind noise
    let speed_kmh = bike.velocity.length() * 3.6;
    let aero_v = (speed_kmh / 200.0).clamp(0.0, 1.0);
    let aero_volume = profile.as_ref().map(|p| p.f("sound", "aero_volume")).unwrap_or(0.8);
    let aero_levels = sounds
        .aero
        .as_ref()
        .map(|a| a.layer_levels(0, aero_v, master * aero_volume))
        .unwrap_or_default();

    for (voice, sink) in &voices {
        let (volume, pitch) = match *voice {
            Voice::Engine { layer, sample } => levels
                .iter()
                .find(|l| l.layer == layer && l.sample == sample)
                .map(|l| (l.volume * engine_gain, l.pitch))
                .unwrap_or((0.0, 1.0)),
            Voice::Limiter => (limiter * engine_gain, 1.0),
            Voice::Starter => (if pt.cranking() { master * gain_at(&cfg.starter, bike, listener, listener_right) } else { 0.0 }, 1.0),
            Voice::Brake { wheel, sample } => brake_levels[wheel].get(sample).map(|l| (l.volume, l.pitch)).unwrap_or((0.0, 1.0)),
            Voice::Tyre { wheel, set, wet, slide, sample } => {
                if wet {
                    (0.0, 1.0)
                } else {
                    match &tyre_levels[wheel] {
                        Some((s, levels)) if *s == set => levels
                            .iter()
                            .find(|(sl, _, _)| *sl == slide)
                            .and_then(|(_, l, p)| l.get(sample).map(|x| (x.volume, x.pitch * p)))
                            .unwrap_or((0.0, 1.0)),
                        _ => (0.0, 1.0),
                    }
                }
            }
            Voice::Transmission { layer, sample } => t_levels
                .iter()
                .find(|l| l.layer == layer && l.sample == sample)
                .map(|l| (l.volume * t_gain, l.pitch))
                .unwrap_or((0.0, 1.0)),
            Voice::Aero { sample } => aero_levels
                .get(sample)
                .map(|l| (l.volume, l.pitch))
                .unwrap_or((0.0, 1.0)),
            Voice::Scrape => {
                if let Some(contact) = &bike.last_body_contact {
                    if contact.scrape_speed > 0.05 && contact.normal_force > 15.0 {
                        let vol = (contact.scrape_speed / 30.0).clamp(0.0, 1.0)
                            * 0.04
                            * (contact.normal_force / 400.0).clamp(0.1, 1.0)
                            * master;
                        let pitch = ((contact.scrape_speed / 30.0).clamp(0.2, 2.5) * 0.3 + 0.8).clamp(0.2, 3.0);
                        (vol, pitch)
                    } else {
                        (0.0, 1.0)
                    }
                } else {
                    (0.0, 1.0)
                }
            }
        };
        sink.set_volume(if paused { 0.0 } else { volume });
        sink.set_speed(pitch.max(0.01));
    }

    // Impact one-shot trigger on significant body collision impulse
    if let Some(contact) = &bike.last_body_contact {
        if contact.impact_impulse > 25.0 {
            let set_key = "concrete";
            if let Some(samples) = sounds.impact_handles.get(set_key) {
                if !samples.is_empty() {
                    let idx = if contact.impact_impulse < 60.0 {
                        0
                    } else if contact.impact_impulse < 150.0 {
                        1.min(samples.len() - 1)
                    } else if contact.impact_impulse < 350.0 {
                        2.min(samples.len() - 1)
                    } else {
                        3.min(samples.len() - 1)
                    };
                    let handle = samples[idx].clone();
                    let gain = ((contact.impact_impulse / 350.0).clamp(0.1, 1.0) * master).clamp(0.05, 1.0);
                    commands.spawn((
                        AudioPlayer::new(handle),
                        PlaybackSettings {
                            mode: PlaybackMode::Despawn,
                            volume: Volume::new(gain),
                            ..default()
                        },
                    ));
                }
            }
        }
    }

    // Gear shift one-shot; the "out" sample when the box goes to neutral.
    if pt.gear != sounds.last_gear {
        let handle = if pt.gear == 0 { sounds.shift_out.clone().or(sounds.shift.clone()) } else { sounds.shift.clone() };
        if let Some(h) = handle {
            let g = master * gain_at(&sounds.cfg.gearbox, bike, listener, listener_right);
            commands.spawn((AudioPlayer::new(h), PlaybackSettings { mode: PlaybackMode::Despawn, volume: Volume::new(g), ..default() }));
        }
        sounds.last_gear = pt.gear;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxbikes_vfs::MxBikesVfs;

    #[test]
    fn test_250xf_audio_assets_load() {
        let Ok(vfs) = MxBikesVfs::auto_detect() else { return };
        let cfg_bytes = vfs.read("bikes/250xf/sfx.cfg").expect("sfx.cfg");
        let cfg_node = ConfigNode::parse(&String::from_utf8_lossy(&cfg_bytes)).expect("parse sfx.cfg");
        let cfg = BikeSfxConfig::parse(&cfg_node);

        let scl_bytes = vfs.read(&format!("bikes/250xf/{}", cfg.engine.scl_file)).expect("engine.scl");
        let scl_node = ConfigNode::parse(&String::from_utf8_lossy(&scl_bytes)).expect("parse engine.scl");
        let engine_scl = SoundCurveLayout::parse(&scl_node);

        let mut loaded_samples = 0;
        for layer in &engine_scl.layers {
            for sample in &layer.samples {
                let path = format!("bikes/250xf/{}", sample.wav);
                let data = vfs.read(&path).unwrap_or_else(|_| vfs.read(&format!("effects/sounds/{}", sample.wav)).unwrap());
                assert!(data.len() >= 12, "sample {} too small", sample.wav);
                assert_eq!(&data[..4], b"RIFF", "sample {} not RIFF", sample.wav);
                assert_eq!(&data[8..12], b"WAVE", "sample {} not WAVE", sample.wav);
                loaded_samples += 1;
            }
        }
        assert_eq!(loaded_samples, 12, "Should load all 12 engine samples");

        if let Some(limiter_wav) = &cfg.engine.limiter_wav {
            let data = vfs.read(&format!("bikes/250xf/{limiter_wav}")).expect("limiter wav");
            assert_eq!(&data[..4], b"RIFF");
        }

        if let Some(starter_wav) = &cfg.starter_wav {
            let data = vfs.read(&format!("effects/sounds/{starter_wav}")).expect("starter wav");
            assert_eq!(&data[..4], b"RIFF");
        }
    }

    #[test]
    fn test_print_wav_stats() {
        let Ok(vfs) = MxBikesVfs::auto_detect() else { return };
        let cfg_bytes = vfs.read("bikes/250xf/sfx.cfg").expect("sfx.cfg");
        let cfg_node = ConfigNode::parse(&String::from_utf8_lossy(&cfg_bytes)).expect("parse sfx.cfg");
        let cfg = BikeSfxConfig::parse(&cfg_node);

        let scl_bytes = vfs.read(&format!("bikes/250xf/{}", cfg.engine.scl_file)).expect("engine.scl");
        let scl_node = ConfigNode::parse(&String::from_utf8_lossy(&scl_bytes)).expect("parse engine.scl");
        let engine_scl = SoundCurveLayout::parse(&scl_node);

        for (rpm, thr) in [(2000.0, 0.0), (3700.0, 0.0), (6682.0, 0.5), (6682.0, 1.0), (12000.0, 1.0)] {
            let (on, off, _lim) = cfg.engine_gains(thr, 0.0, true);
            let v = rpm / cfg.engine.max_rpm;
            let l0 = engine_scl.layer_levels(0, v, on);
            let l1 = engine_scl.layer_levels(1, v, off);
            println!("=== RPM {rpm}, Throttle {thr}: on={on:.3}, off={off:.3} ===");
            for s in l0.iter().chain(l1.iter()) {
                if s.volume > 0.001 {
                    let sample_name = &engine_scl.layers[s.layer].samples[s.sample].wav;
                    println!("  Layer {} Sample {} ({sample_name}): vol={:.3}, pitch={:.3}", s.layer, s.sample, s.volume, s.pitch);
                }
            }
        }
    }

    #[test]
    fn test_secondary_audio_assets() {
        let Ok(vfs) = MxBikesVfs::auto_detect() else { return };
        // Aerodynamic wind noise
        let aero_bytes = vfs.read("effects/sounds/aerodynamic_onboard.scl").expect("aerodynamic_onboard.scl");
        let aero_node = ConfigNode::parse(&String::from_utf8_lossy(&aero_bytes)).expect("parse aero scl");
        let aero_scl = SoundCurveLayout::parse(&aero_node);
        assert_eq!(aero_scl.layers.len(), 1);
        assert_eq!(aero_scl.layers[0].samples.len(), 2);
        assert_eq!(aero_scl.layers[0].samples[0].wav, "aerodynamic_onboard_low.wav");
        assert_eq!(aero_scl.layers[0].samples[1].wav, "aerodynamic_onboard_high.wav");

        // Verify both wavs load
        for sample in &aero_scl.layers[0].samples {
            let data = vfs.read(&format!("effects/sounds/{}", sample.wav)).expect("aero wav");
            assert_eq!(&data[..4], b"RIFF");
        }

        // Impacts and scrape config
        let imp_bytes = vfs.read("effects/sounds/sets/impacts.cfg").expect("impacts.cfg");
        let imp_node = ConfigNode::parse(&String::from_utf8_lossy(&imp_bytes)).expect("parse impacts.cfg");
        let impacts = ImpactsConfig::parse(&imp_node);
        assert!(impacts.sets.contains_key("concrete"));
        assert!(impacts.sets.contains_key("grass"));
        assert!(impacts.sets.contains_key("sand"));
        assert!(impacts.sets.contains_key("armco"));
        assert!(impacts.sets.contains_key("tyreswall"));
        assert!(impacts.sets.contains_key("wirefence"));
        assert!(impacts.sets.contains_key("vehicle"));
        assert!(impacts.sets.contains_key("rider"));

        // Verify concrete impact samples exist in VFS
        let concrete = &impacts.sets["concrete"];
        for imp in &concrete.impacts {
            let data = vfs.read(&format!("effects/sounds/{}", imp.wav)).expect("concrete impact wav");
            assert_eq!(&data[..4], b"RIFF");
        }
        if let Some(sc) = &concrete.scrape {
            let data = vfs.read(&format!("effects/sounds/{}", sc.wav)).expect("concrete scrape wav");
            assert_eq!(&data[..4], b"RIFF");
        }
    }

    #[allow(dead_code)]
    fn print_wav_info(name: &str, bytes: &[u8]) {
        // Find 'fmt ' and 'data'
        let mut fmt_idx = None;
        let mut data_idx = None;
        let mut i = 12;
        while i + 8 <= bytes.len() {
            let chunk_id = &bytes[i..i+4];
            let chunk_size = u32::from_le_bytes(bytes[i+4..i+8].try_into().unwrap()) as usize;
            if chunk_id == b"fmt " {
                fmt_idx = Some(i + 8);
            } else if chunk_id == b"data" {
                data_idx = Some((i + 8, chunk_size));
                break;
            }
            i += 8 + chunk_size;
        }
        if let (Some(fmt), Some((data_start, data_len))) = (fmt_idx, data_idx) {
            let channels = u16::from_le_bytes(bytes[fmt+2..fmt+4].try_into().unwrap());
            let sample_rate = u32::from_le_bytes(bytes[fmt+4..fmt+8].try_into().unwrap());
            let bits = u16::from_le_bytes(bytes[fmt+14..fmt+16].try_into().unwrap());
            let data_end = (data_start + data_len).min(bytes.len());
            let pcm = &bytes[data_start..data_end];
            let mut sum_sq = 0.0f64;
            let mut peak = 0i16;
            let count = pcm.len() / 2;
            for chunk in pcm.chunks_exact(2) {
                let s = i16::from_le_bytes(chunk.try_into().unwrap());
                sum_sq += (s as f64) * (s as f64);
                peak = peak.max(s.abs());
            }
            let rms = (sum_sq / count as f64).sqrt() / 32768.0;
            let peak_f = peak as f64 / 32768.0;
            println!("{name:<25}: ch={channels} rate={sample_rate} bits={bits} Peak={peak_f:.3} RMS={rms:.3}");
        }
    }
}

