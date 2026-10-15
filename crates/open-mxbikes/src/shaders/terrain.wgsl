#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

struct TerrainParams {
    tiling_u: vec4<f32>,
    tiling_v: vec4<f32>,
    extent: vec4<f32>,
    layers: u32,
}

@group(2) @binding(100) var<uniform> params: TerrainParams;
@group(2) @binding(101) var layer0: texture_2d<f32>;
@group(2) @binding(102) var layer_sampler: sampler;
@group(2) @binding(103) var layer1: texture_2d<f32>;
@group(2) @binding(104) var layer2: texture_2d<f32>;
@group(2) @binding(105) var layer3: texture_2d<f32>;
@group(2) @binding(106) var mask1: texture_2d<f32>;
@group(2) @binding(107) var mask_sampler: sampler;
@group(2) @binding(108) var mask2: texture_2d<f32>;
@group(2) @binding(109) var mask3: texture_2d<f32>;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Terrain UV in the asset frame (engine Z is mirrored).
    let p = in.world_position.xyz;
    let uv = vec2<f32>(
        (p.x - params.extent.x) / params.extent.z,
        (-p.z - params.extent.y) / params.extent.w,
    );

    var color = textureSample(layer0, layer_sampler, uv * vec2(params.tiling_u.x, params.tiling_v.x)).rgb;
    if (params.layers > 1u) {
        let m = textureSample(mask1, mask_sampler, uv).r;
        let c = textureSample(layer1, layer_sampler, uv * vec2(params.tiling_u.y, params.tiling_v.y)).rgb;
        color = mix(color, c, m);
    }
    if (params.layers > 2u) {
        let m = textureSample(mask2, mask_sampler, uv).r;
        let c = textureSample(layer2, layer_sampler, uv * vec2(params.tiling_u.z, params.tiling_v.z)).rgb;
        color = mix(color, c, m);
    }
    if (params.layers > 3u) {
        let m = textureSample(mask3, mask_sampler, uv).r;
        let c = textureSample(layer3, layer_sampler, uv * vec2(params.tiling_u.w, params.tiling_v.w)).rgb;
        color = mix(color, c, m);
    }
    pbr_input.material.base_color = vec4<f32>(color, 1.0);
    pbr_input.material.perceptual_roughness = 0.92;
    pbr_input.material.metallic = 0.0;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}
