//! Applies the profile's graphics settings to the window, camera and textures.
//!
//! The game reads them in its graphics setup (`FUN_1400b4260`, `FUN_1400d7870`,
//! `FUN_1400ef210`). They are applied once at start and again when the settings page is closed
//! with Done.

use crate::profile::Profile;
use crate::ui::ApplyGraphicsSettings;
use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode};
use std::sync::atomic::{AtomicU32, Ordering};

/// Texture filter (0 bilinear, 1 trilinear) and anisotropy exponent, for textures created later.
static TEXTURE_FILTER: AtomicU32 = AtomicU32::new(1);
static ANISOTROPY_LOG2: AtomicU32 = AtomicU32::new(3);
/// Mip levels dropped from world textures: `gfx/texturesize` 0 / 40 / 80 drop 0 / 1 / 2.
static TEXTURE_SKIP: AtomicU32 = AtomicU32::new(0);
/// `gfx/detail_bias` (0, 0.2, 0.4) in thousandths.
static DETAIL_BIAS_MILLI: AtomicU32 = AtomicU32::new(0);

/// Window mode for `gfx/fullscreen` (1 fullscreen, 2 borderless, 0 windowed).
pub fn window_mode(profile: &Profile) -> WindowMode {

    match profile.i("gfx", "fullscreen") {
        1 | 2 => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        _ => WindowMode::Windowed,
    }
}

/// MSAA for `gfx/multisample`, which stores the sample count (0, 2, 4, 8).
pub fn msaa(profile: &Profile) -> Msaa {
    match profile.i("gfx", "multisample") {
        2 => Msaa::Sample2,
        4 => Msaa::Sample4,
        8.. => Msaa::Sample8,
        _ => Msaa::Off,
    }
}

pub fn present_mode(profile: &Profile) -> PresentMode {
    if profile.i("gfx", "vsync") != 0 {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    }
}

/// Stores the texture-related settings for textures loaded from now on.
pub fn store_texture_settings(profile: &Profile) {
    TEXTURE_FILTER.store(profile.i("gfx", "texturefilter").clamp(0, 1) as u32, Ordering::Relaxed);
    ANISOTROPY_LOG2.store(profile.i("gfx", "anisotropic").clamp(0, 4) as u32, Ordering::Relaxed);
    let skip = match profile.i("gfx", "texturesize") {
        v if v >= 80 => 2,
        v if v >= 40 => 1,
        _ => 0,
    };
    TEXTURE_SKIP.store(skip, Ordering::Relaxed);
    DETAIL_BIAS_MILLI.store((profile.f("gfx", "detail_bias").clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
}

/// Mip levels to drop from world textures (texture size setting).
pub fn texture_skip() -> u32 {
    TEXTURE_SKIP.load(Ordering::Relaxed)
}

/// Scale on model LOD switch distances: lower detail switches earlier. INFERRED: the game passes
/// the bias to its renderer; it is applied here as distance * (1 - bias).
pub fn lod_distance_scale() -> f32 {
    1.0 - DETAIL_BIAS_MILLI.load(Ordering::Relaxed) as f32 / 1000.0
}

/// Mip filter and anisotropy from the settings. Anisotropy needs linear mip filtering, so the
/// bilinear filter runs without it.
pub fn filtering() -> (ImageFilterMode, u16) {
    if TEXTURE_FILTER.load(Ordering::Relaxed) == 0 {
        (ImageFilterMode::Nearest, 1)
    } else {
        (ImageFilterMode::Linear, 1 << ANISOTROPY_LOG2.load(Ordering::Relaxed))
    }
}

/// Fixed aspect ratio for `gfx/display_ratio` (0 = from the resolution).
fn display_aspect(profile: &Profile) -> Option<f32> {
    match profile.i("gfx", "display_ratio") {
        1 => Some(4.0 / 3.0),
        2 => Some(16.0 / 9.0),
        3 => Some(15.0 / 9.0),
        4 => Some(16.0 / 10.0),
        5 => Some(5.0 / 4.0),
        _ => None,
    }
}

pub fn request_initial_apply(mut events: EventWriter<ApplyGraphicsSettings>) {
    events.send(ApplyGraphicsSettings);
}

#[allow(clippy::type_complexity)]
pub fn apply_graphics_settings(
    mut events: EventReader<ApplyGraphicsSettings>,
    profile: Option<Res<Profile>>,
    mut windows: Query<&mut Window>,
    monitors: Query<&bevy::window::Monitor, With<bevy::window::PrimaryMonitor>>,
    mut cameras: Query<&mut Msaa, With<Camera3d>>,
    mut lights: Query<&mut DirectionalLight>,
    mut images: ResMut<Assets<Image>>,
) {
    if events.read().count() == 0 {
        return;
    }
    let Some(profile) = profile else { return };
    let filter_key = || (TEXTURE_FILTER.load(Ordering::Relaxed), ANISOTROPY_LOG2.load(Ordering::Relaxed));
    let filter_changed = {
        let before = filter_key();
        store_texture_settings(&profile);
        before != filter_key()
    };
    if let Ok(mut window) = windows.get_single_mut() {
        let mode = window_mode(&profile);
        // Fullscreen (borderless) always covers the monitor; the profile resolution only sizes a
        // window, and never beyond the monitor, so the settings stay reachable.
        if matches!(mode, WindowMode::Windowed) {
            let (mut width, mut height) = (profile.f("gfx", "x"), profile.f("gfx", "y"));
            if let Ok(m) = monitors.get_single() {
                let scale = m.scale_factor as f32;
                width = width.min(m.physical_width as f32 / scale);
                height = height.min(m.physical_height as f32 / scale);
            }
            if width >= 640.0 && height >= 480.0 && (window.resolution.width() != width || window.resolution.height() != height) {
                window.resolution.set(width, height);
            }
        }
        if window.mode != mode {
            window.mode = mode;
        }
        let present = present_mode(&profile);
        if window.present_mode != present {
            window.present_mode = present;
        }
    }
    let target_msaa = msaa(&profile);
    for mut m in &mut cameras {
        if *m != target_msaa {
            *m = target_msaa;
        }
    }
    let shadows = profile.i("gfx", "shadow_disable") == 0;
    for mut light in &mut lights {
        if light.shadows_enabled != shadows {
            light.shadows_enabled = shadows;
        }
    }
    // Re-sample loaded mipmapped textures with the new filter (UI images have one level).
    if filter_changed {
        let (mip, aniso) = filtering();
        let ids: Vec<_> = images
            .iter()
            .filter(|(_, img)| img.texture_descriptor.mip_level_count > 1 && matches!(img.sampler, ImageSampler::Descriptor(_)))
            .map(|(id, _)| id)
            .collect();
        for id in ids {
            if let Some(img) = images.get_mut(id) {
                if let ImageSampler::Descriptor(d) = &mut img.sampler {
                    d.mipmap_filter = mip;
                    d.anisotropy_clamp = aniso;
                }
            }
        }
    }
}

/// Keeps the 3D view at the fixed screen ratio when one is set (for displays with non-square
/// pixels), after Bevy has set the aspect from the window.
pub fn apply_display_ratio(profile: Option<Res<Profile>>, mut cameras: Query<&mut Projection, With<Camera3d>>) {
    let Some(aspect) = profile.as_ref().and_then(|p| display_aspect(p)) else { return };
    for mut projection in &mut cameras {
        if let Projection::Perspective(p) = projection.as_mut() {
            if (p.aspect_ratio - aspect).abs() > 1e-4 {
                p.aspect_ratio = aspect;
            }
        }
    }
}
