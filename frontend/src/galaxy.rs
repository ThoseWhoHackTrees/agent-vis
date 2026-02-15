// hello world
use bevy::prelude::*;
use bevy_fontmesh::{TextMesh, TextMeshBundle, TextMeshStyle};
use crate::fs_model::{FileNode, FileSystemModel};
use crate::planet_material::{PlanetMaterial, PlanetMaterialExtension};
use std::f32::consts::PI;

#[derive(Component)]
pub struct FileStar {
    pub node_index: usize,
}

#[derive(Component)]
pub struct StarGlow;

#[derive(Component)]
pub struct FileLabel {
    pub star_entity: Entity,
    pub offset: Vec3,
}

/// Calculate position for a node - folders in spiral, files cluster around parent
pub fn calculate_galaxy_position(model: &FileSystemModel, node_idx: usize) -> Vec3 {
    let node = &model.nodes[node_idx];

    // Root at center
    if node.depth == 0 {
        return Vec3::new(0.0, 0.0, 0.0);
    }

    let golden_ratio = 1.618033988749;

    // Get index within parent's children
    let index_in_parent = if let Some(parent_idx) = node.parent {
        model.nodes[parent_idx]
            .children
            .iter()
            .position(|&idx| idx == node_idx)
            .unwrap_or(0)
    } else {
        0
    };

    if node.is_dir {
        // Directories: spiral pattern based on depth
        // Higher in the tree (lower depth) = slightly higher in space
        let angle = (node_idx as f32 * golden_ratio * 2.0 * PI) + (index_in_parent as f32 * 0.5);
        let radius = (node.depth as f32) * 8.0 + (index_in_parent as f32) * 1.5;

        // Root slightly above origin, everything else slightly below
        // Much smaller variation: root at ~2, depth 1 at ~0, depth 2 at ~-2, etc.
        let y = 2.0 - (node.depth as f32) * 2.0;

        let x = radius * angle.cos();
        let z = radius * angle.sin();

        Vec3::new(x, y, z)
    } else {
        // Files: cluster around and below parent folder, more spread out
        if let Some(parent_idx) = node.parent {
            let parent_pos = calculate_galaxy_position(model, parent_idx);

            // Distribute files in a circle around parent, more spread out
            let angle = index_in_parent as f32 * golden_ratio * 2.0 * PI;
            let cluster_radius = 3.5; // Increased from 2.0 for more spread

            let offset_x = cluster_radius * angle.cos();
            let offset_z = cluster_radius * angle.sin();
            let offset_y = -2.0 - (index_in_parent as f32 * 0.3).min(3.0); // Below parent, more vertical spread

            Vec3::new(
                parent_pos.x + offset_x,
                parent_pos.y + offset_y,
                parent_pos.z + offset_z,
            )
        } else {
            // Fallback if no parent (shouldn't happen)
            Vec3::new(0.0, -5.0, 0.0)
        }
    }
}

/// Calculate star size based on node properties
pub fn calculate_star_size(node: &FileNode) -> f32 {
    if node.is_dir {
        // Directories are larger, and slightly bigger the higher they are in the tree (lower depth)
        let depth_size_bonus = if node.depth == 0 {
            0.3 // Root is slightly bigger
        } else if node.depth == 1 {
            0.2
        } else {
            0.1 // Deeper levels just a bit bigger than files
        };

        let base_size = 0.5 + depth_size_bonus;
        let children_bonus = (node.children.len() as f32 * 0.05).min(0.3);

        base_size + children_bonus
    } else {
        // Files: size based on line count
        let line_count = count_file_lines(&node.path);
        let base_size = 0.2;

        // Scale size based on line count (logarithmic scaling)
        // 0 lines = 0.2, 100 lines = 0.3, 1000 lines = 0.5, 10000 lines = 0.7
        let size_bonus = if line_count > 0 {
            ((line_count as f32).log10() * 0.15).min(0.5)
        } else {
            0.0
        };

        base_size + size_bonus
    }
}

fn count_file_lines(path: &std::path::Path) -> usize {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    if let Ok(file) = File::open(path) {
        BufReader::new(file).lines().count()
    } else {
        0
    }
}

/// Calculate star color based on node properties - HackMIT color scheme
pub fn calculate_star_color(node: &FileNode) -> Color {
    if node.is_dir {
        // Directories are warm whitish-yellow
        Color::srgb(1.0, 0.95, 0.7) // Whitish yellow
    } else {
        // Files colored by extension - pastel but vibrant
        let extension = node.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match extension {
            "rs" => Color::srgb(1.0, 0.75, 0.6),      // Rust - pastel coral
            "toml" | "yaml" | "yml" | "json" => Color::srgb(1.0, 0.95, 0.6), // Config - pastel yellow
            "md" | "txt" => Color::srgb(0.9, 0.8, 1.0), // Text - pastel lavender
            "js" | "ts" => Color::srgb(1.0, 0.98, 0.7), // JS - pastel cream yellow
            "py" => Color::srgb(0.7, 0.85, 1.0),      // Python - pastel sky blue
            "html" | "css" => Color::srgb(1.0, 0.7, 0.85), // Web - pastel pink
            "java" | "cpp" | "c" => Color::srgb(0.85, 0.75, 1.0), // Compiled - pastel purple
            "go" => Color::srgb(0.7, 0.9, 1.0),      // Go - pastel cyan
            _ => Color::srgb(0.9, 0.8, 0.95),         // Unknown - pastel lilac
        }
    }
}

/// Spawn a star entity for a file system node
pub fn spawn_star(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    planet_materials: &mut ResMut<Assets<PlanetMaterial>>,
    asset_server: &Res<AssetServer>,
    model: &FileSystemModel,
    node_idx: usize,
) -> Entity {
    let node = &model.nodes[node_idx];
    let position = calculate_galaxy_position(model, node_idx);
    let size = calculate_star_size(node);
    let color = calculate_star_color(node);

    // Create sphere - both folders and files bloom
    let mesh = meshes.add(Sphere::new(size));

    // Directories get higher emissive, files get moderate emissive
    let emissive_strength = if node.is_dir {
        // Directories are bright stars with strong bloom
        6.0 + (node.children.len() as f32 * 0.5).min(10.0)
    } else {
        // Files have moderate emissive for subtle bloom
        2.5
    };

    // Use planet material with crescent shadow effect
    let material = planet_materials.add(PlanetMaterial {
        base: StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * emissive_strength,
            ..default()
        },
        extension: PlanetMaterialExtension {
            base_color: LinearRgba::from(color),
            noise_scale: 1.0,
            noise_intensity: 0.0, // No noise, just shadow
        },
    });

    // Spawn the star
    let star_entity = commands
        .spawn((
            FileStar { node_index: node_idx },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ))
        .id();

    // Spawn label as a separate entity (not a child)
    let label_offset = Vec3::new(0.0, size + 1.5, 0.0);
    let label_pos = position + label_offset;

    commands.spawn((
        TextMeshBundle {
            text_mesh: TextMesh {
                text: node.name.clone(),
                font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                style: TextMeshStyle {
                    depth: 0.2,
                    subdivision: 10,
                    ..default()
                },
            },
            material: MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                ..default()
            })),
            transform: Transform::from_translation(label_pos)
                .with_scale(Vec3::splat(0.5)),
            ..default()
        },
        FileLabel {
            star_entity,
            offset: label_offset,
        },
    ));

    star_entity
}

/// Spawn all stars for the initial file system
pub fn spawn_galaxy(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    planet_materials: &mut ResMut<Assets<PlanetMaterial>>,
    asset_server: &Res<AssetServer>,
    model: &FileSystemModel,
) {
    for node_idx in 0..model.total_nodes() {
        spawn_star(commands, meshes, materials, planet_materials, asset_server, model, node_idx);
    }
}
