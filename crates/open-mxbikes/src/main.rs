pub mod bike_entity;
pub mod camera;
pub mod engine_audio;
pub mod hud;
pub mod input;
pub mod particles;
pub mod plate;
pub mod profile;
pub mod rider;
pub mod scene;
pub mod session;
pub mod setup;
pub mod terrain_material;
pub mod ui;
pub mod world;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{PresentMode, WindowResolution};
use camera::FollowCamera;
use mxbikes_vfs::MxBikesVfs;
use std::sync::Arc;
use world::{Atmosphere, LaunchOptions, Vfs};

fn parse_args() -> (LaunchOptions, Option<String>) {
    let mut opts = LaunchOptions {
        // Like the game, start at the main menu over the menu background scene.
        track: "bg".into(),
        bike: "250xf".into(),
        screenshot: None,
        camera: "chase".into(),
        autopilot: false,
        autopilot_steer: 0.0,
        autopilot_throttle: 0.6,
        autoshift: profile::Profile::load().i("aids", "autoshift") != 0,
        menu: None,
        paints: Default::default(),
        number: String::new(),
        rider_name: String::new(),
        font: "default_black".into(),
        dirt: None,
    };
    let mut game_dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--track" => opts.track = args.next().unwrap_or(opts.track),
            "--bike" => opts.bike = args.next().unwrap_or(opts.bike),
            "--game-dir" => game_dir = args.next(),
            "--number" => opts.number = args.next().unwrap_or(opts.number),
            "--name" => opts.rider_name = args.next().unwrap_or(opts.rider_name),
            "--font" => opts.font = args.next().unwrap_or(opts.font),
            "--dirt" => opts.dirt = args.next().and_then(|v| v.parse().ok()),
            "--screenshot" => {
                let path = args.next().unwrap_or_else(|| "screenshot.png".into());
                let after = args.next().and_then(|s| s.parse().ok()).unwrap_or(5.0);
                opts.screenshot = Some((path, after));
            }
            "--camera" => opts.camera = args.next().unwrap_or(opts.camera),
            "--autopilot" => opts.autopilot = true,
            "--autopilot-throttle" => opts.autopilot_throttle = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.6),
            "--autopilot-steer" => opts.autopilot_steer = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            "--autoshift" => opts.autoshift = true,
            "--menu" => opts.menu = args.next(),
            "--manual-shift" | "--no-autoshift" => opts.autoshift = false,
            "--paint" => match args.next().as_deref().and_then(|v| v.split_once('=')) {
                Some((part, name)) => {
                    opts.paints.insert(part.to_ascii_lowercase(), name.to_string());
                }
                None => eprintln!("--paint expects <part>=<name>, e.g. --paint bike=blue"),
            },
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    (opts, game_dir)
}

fn main() {
    let (options, game_dir) = parse_args();
    let vfs = match game_dir.or_else(|| std::env::var("MXB_GAME_DIR").ok()) {
        Some(d) => MxBikesVfs::open(d),
        None => MxBikesVfs::auto_detect(),
    };
    let vfs = match vfs {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Could not mount MX Bikes: {e}. Pass --game-dir <path to MX Bikes>.");
            std::process::exit(1);
        }
    };

    let profile = profile::Profile::load();
    // 0 = desktop size in the game; a 1600 x 900 window stands in for it.
    let (width, height) = match (profile.f("gfx", "x"), profile.f("gfx", "y")) {
        (x, y) if x >= 640.0 && y >= 480.0 => (x, y),
        _ => (1600.0, 900.0),
    };
    let vsync = profile.i("gfx", "vsync") != 0;
    let present_mode = if vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Open-MXBikes".into(),
                resolution: WindowResolution::new(width, height),
                present_mode,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(terrain_material::TerrainMaterialPlugin)
        .insert_resource(options)
        .insert_resource(profile)
        .insert_resource(Vfs(Arc::new(vfs)))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.65, 0.8)))
        .init_resource::<input::ControlState>()
        .add_event::<world::ReloadSession>()
        .add_event::<ui::ApplyGraphicsSettings>()
        .add_systems(
            Startup,
            (
                world::setup_world,
                hud::setup_hud,
                ui::setup_ui,
                spawn_camera.after(world::setup_world),
            ),
        )
        .add_systems(
            Update,
            (
                input::read_controls,
                ui::handle_menu_inputs,
                autopilot,
                bike_entity::step_physics,
                bike_entity::sync_parts,
                bike_entity::update_chain_uv,
                world::update_terrain_ruts,
                particles::update_particles,
                scene::animate_morphs,
                world::update_lap_timing,
                rider::animate_rider,
                scene::update_mud,
                camera::update_camera,
                world::update_lod_switching,
                world::follow_sky,
                hud::update_hud,
                ui::sync_menu_visibility,
                ui::handle_menu_interactions,
                ui::update_cursor,
                auto_screenshot,
            )
                .chain(),
        )
        .add_systems(Update, world::apply_setup.after(ui::handle_menu_interactions))
        .add_systems(Update, world::handle_session_reload.after(ui::handle_menu_interactions))
        .add_systems(Update, ui::apply_graphics_settings.after(ui::handle_menu_interactions))
        .add_systems(Startup, engine_audio::setup_bike_sounds.after(world::setup_world))
        .add_systems(Startup, session::setup_session)
        .add_systems(Update, camera::onboard_visibility.after(camera::update_camera))
        .add_systems(Update, ui::capture_control.before(ui::handle_menu_inputs))
        .add_systems(Update, ui::handle_editbox_input.before(ui::handle_menu_inputs))
        .add_systems(Update, engine_audio::update_bike_sounds.after(bike_entity::step_physics))
        .run();
}

fn spawn_camera(
    mut commands: Commands,
    atmosphere: Res<Atmosphere>,
    options: Res<LaunchOptions>,
    profile: Option<Res<profile::Profile>>,
) {
    let mode = match options.camera.as_str() {
        "onboard" => camera::CameraMode::Onboard,
        "side" => camera::CameraMode::Side,
        "left" => camera::CameraMode::Left,
        "showroom" | "menu" => camera::CameraMode::Showroom(camera::ShowroomCameraConfig::default()),
        _ if options.track == "menu" || options.track == "bg" => {
            camera::CameraMode::Showroom(camera::ShowroomCameraConfig::default())
        }
        _ => camera::CameraMode::Chase,
    };
    let fov_deg = profile.as_ref().map(|p| p.f("view", "fov")).filter(|&f| f >= 30.0 && f <= 150.0).unwrap_or(90.0);
    let msaa = match profile.as_ref().map(|p| p.i("gfx", "multisample")).unwrap_or(0) {
        1 => Msaa::Sample2,
        2 => Msaa::Sample4,
        3 => Msaa::Sample8,
        _ => Msaa::Off,
    };
    commands.spawn((
        Camera3d::default(),
        msaa,
        Projection::Perspective(PerspectiveProjection {
            fov: fov_deg.to_radians(),
            near: 0.05,
            far: 6000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        world::fog_settings(&atmosphere),
        FollowCamera::new(mode),
    ));
}

/// `--screenshot <path> <seconds>`: capture the window after a delay, then exit.
fn auto_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    options: Res<LaunchOptions>,
    mut state: Local<u8>,
    mut exit: EventWriter<AppExit>,
) {
    let Some((path, after)) = &options.screenshot else { return };
    let t = time.elapsed_secs();
    if *state == 0 && t > *after {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        *state = 1;
    } else if *state == 1 && t > *after + 2.0 {
        exit.send(AppExit::Success);
    }
}

/// Scripted input for captures, from 2 s on. `--autopilot`: first gear and part throttle.
/// `--autopilot-steer <v>`: hold the steering input at `v` (alone: stand still with the bars turned).
fn autopilot(time: Res<Time>, options: Res<LaunchOptions>, mut controls: ResMut<input::ControlState>, mut done: Local<bool>) {
    if time.elapsed_secs() < 2.0 {
        return;
    }
    if options.autopilot_steer != 0.0 {
        controls.rider.steer = options.autopilot_steer;
    }
    if !options.autopilot {
        return;
    }
    if !*done {
        controls.rider.shift_up = true;
        *done = true;
    }
    controls.rider.throttle = options.autopilot_throttle;
}
