// hello world
use bevy::prelude::*;
use bevy_fontmesh::{FontMesh, TextMesh, TextMeshBundle, TextMeshStyle};
use crate::fs_model::{FileNode, FileSystemModel};
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
        let angle = (node_idx as f32 * golden_ratio * 2.0 * PI) + (index_in_parent as f32 * 0.5);
        let radius = (node.depth as f32) * 8.0 + (index_in_parent as f32) * 1.5;
        let y = (node.depth as f32) * 2.0 - 5.0;

        let x = radius * angle.cos();
        let z = radius * angle.sin();

        Vec3::new(x, y, z)
    } else {
        // Files: cluster around and below parent folder
        if let Some(parent_idx) = node.parent {
            let parent_pos = calculate_galaxy_position(model, parent_idx);

            // Distribute files in a circle around parent
            let angle = index_in_parent as f32 * golden_ratio * 2.0 * PI;
            let cluster_radius = 2.0; // How far from parent

            let offset_x = cluster_radius * angle.cos();
            let offset_z = cluster_radius * angle.sin();
            let offset_y = -1.5 - (index_in_parent as f32 * 0.2).min(2.0); // Below parent

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
        // Directories are larger
        0.8 + (node.children.len() as f32 * 0.05).min(1.2)
    } else {
        // Files are smaller
        0.3
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
    asset_server: &Res<AssetServer>,
    model: &FileSystemModel,
    node_idx: usize,
) -> Entity {
    let node = &model.nodes[node_idx];
    let position = calculate_galaxy_position(model, node_idx);
    let size = calculate_star_size(node);
    let color = calculate_star_color(node);

    // Create sphere - only directories bloom
    let mesh = meshes.add(Sphere::new(size));

    // Only directories get high emissive for bloom effect
    let emissive_strength = if node.is_dir {
        // Directories are bright stars with bloom
        5.0 + (node.children.len() as f32 * 0.5).min(10.0)
    } else {
        // Files have no emissive - no bloom
        0.0
    };

    let material = materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * emissive_strength,
        ..default()
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
    asset_server: &Res<AssetServer>,
    model: &FileSystemModel,
) {
    for node_idx in 0..model.total_nodes() {
        spawn_star(commands, meshes, materials, asset_server, model, node_idx);
    }
}
