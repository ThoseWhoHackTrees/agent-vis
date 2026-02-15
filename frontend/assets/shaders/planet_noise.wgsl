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

struct PlanetMaterialExtension {
    base_color: vec4<f32>,
    noise_scale: f32,
    noise_intensity: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> extension: PlanetMaterialExtension;

// Simple 3D noise function using sine waves
fn noise3d(p: vec3<f32>) -> f32 {
    let p_scaled = p * extension.noise_scale;

    // Multiple layers of sine-based noise
    let n1 = sin(p_scaled.x * 3.0 + sin(p_scaled.y * 2.0));
    let n2 = sin(p_scaled.y * 2.5 + sin(p_scaled.z * 3.5));
    let n3 = sin(p_scaled.z * 2.0 + sin(p_scaled.x * 2.5));

    // Combine and normalize to 0-1 range
    return (n1 + n2 + n3) * 0.33 + 0.5;
}

// Fractal noise for more detail
fn fractal_noise(p: vec3<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    var p_var = p;

    // 3 octaves of noise
    for (var i = 0; i < 3; i = i + 1) {
        value += noise3d(p_var * frequency) * amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }

    return value;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Get the standard PBR input
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Use world position for consistent noise across the sphere
    let world_pos = in.world_position.xyz;

    // Generate fractal noise
    let noise = fractal_noise(world_pos);

    // Map noise to darkening factor
    let darken_min = 1.0 - extension.noise_intensity * 0.3;
    let darken = mix(darken_min, 1.0, noise);

    // Apply darkening to base color
    pbr_input.material.base_color = pbr_input.material.base_color * vec4<f32>(darken, darken, darken, 1.0);

    // Also darken the emissive a bit
    pbr_input.material.emissive = pbr_input.material.emissive * darken;

    // Alpha discard (takes material + color, returns discarded color)
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
