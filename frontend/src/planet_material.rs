use bevy::prelude::*;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::AsBindGroup;

/// Extension to StandardMaterial that adds noise-based darkening effect
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct PlanetMaterialExtension {
    /// The base color of the planet (passed to shader)
    #[uniform(100)]
    pub base_color: LinearRgba,

    /// Scale of the noise pattern
    #[uniform(100)]
    pub noise_scale: f32,

    /// How much to darken (0.0 = no darkening, 1.0 = can be very dark)
    #[uniform(100)]
    pub noise_intensity: f32,
}

impl MaterialExtension for PlanetMaterialExtension {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/planet_noise.wgsl".into()
    }
}

pub type PlanetMaterial = ExtendedMaterial<StandardMaterial, PlanetMaterialExtension>;
