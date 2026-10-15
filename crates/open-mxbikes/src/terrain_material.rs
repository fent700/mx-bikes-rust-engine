//! Terrain splat material: up to four surface layers blended by the `.map` coverage masks.
#![allow(dead_code)]

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderRef, ShaderType};

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
#[allow(dead_code)]
pub struct TerrainParams {
    /// Repeats across the terrain for layers 0..3 (u component).
    #[allow(dead_code)]
    pub tiling_u: Vec4,
    #[allow(dead_code)]
    pub tiling_v: Vec4,
    /// origin x, origin z (asset frame), size x, size z.
    #[allow(dead_code)]
    pub extent: Vec4,
    /// Number of active layers.
    #[allow(dead_code)]
    pub layers: u32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExtension {
    #[uniform(100)]
    pub params: TerrainParams,
    #[texture(101)]
    #[sampler(102)]
    pub layer0: Handle<Image>,
    #[texture(103)]
    pub layer1: Handle<Image>,
    #[texture(104)]
    pub layer2: Handle<Image>,
    #[texture(105)]
    pub layer3: Handle<Image>,
    #[texture(106)]
    #[sampler(107)]
    pub mask1: Handle<Image>,
    #[texture(108)]
    pub mask2: Handle<Image>,
    #[texture(109)]
    pub mask3: Handle<Image>,
}

pub const SHADER_PATH: &str = "embedded://open_mxbikes/shaders/terrain.wgsl";

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
}

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/terrain.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}
