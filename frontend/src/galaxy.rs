use bevy::prelude::*;
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

/// Calculate position for a node in a galaxy spiral pattern
pub fn calculate_galaxy_position(model: &FileSystemModel, node_idx: usize) -> Vec3 {
    let node = &model.nodes[node_idx];

    // Root at center
    if node.depth == 0 {
        return Vec3::new(0.0, 0.0, 0.0);
    }

    // Use golden ratio for spiral distribution
    let golden_ratio = 1.618033988749;

    // Use node index for consistent positioning
    let index_in_parent = if let Some(parent_idx) = node.parent {
        model.nodes[parent_idx]
            .children
            .iter()
            .position(|&idx| idx == node_idx)
            .unwrap_or(0)
    } else {
        0
    };

    // Create spiral arms based on depth
    let angle = (node_idx as f32 * golden_ratio * 2.0 * PI) + (index_in_parent as f32 * 0.5);
    let radius = (node.depth as f32) * 8.0 + (index_in_parent as f32) * 1.5;

    // Add some vertical spread based on depth
    let y = (node.depth as f32) * 2.0 - 5.0;

    // Spiral coordinates
    let x = radius * angle.cos();
    let z = radius * angle.sin();

    Vec3::new(x, y, z)
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

/// Calculate star color based on node properties
pub fn calculate_star_color(node: &FileNode) -> Color {
    if node.is_dir {
        // Directories are blue-white
        Color::srgb(0.7, 0.8, 1.0)
    } else {
        // Files colored by extension
        let extension = node.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match extension {
            "rs" => Color::srgb(1.0, 0.6, 0.3),      // Rust - orange
            "toml" | "yaml" | "yml" | "json" => Color::srgb(0.9, 0.9, 0.5), // Config - yellow
            "md" | "txt" => Color::srgb(0.8, 0.8, 0.8), // Text - white
            "js" | "ts" => Color::srgb(0.9, 0.9, 0.3), // JS - bright yellow
            "py" => Color::srgb(0.3, 0.6, 1.0),      // Python - blue
            "html" | "css" => Color::srgb(1.0, 0.4, 0.6), // Web - pink
            _ => Color::srgb(0.6, 0.6, 0.7),         // Unknown - gray
        }
    }
}

/// Spawn a star entity for a file system node
pub fn spawn_star(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    model: &FileSystemModel,
    node_idx: usize,
) -> Entity {
    let node = &model.nodes[node_idx];
    let position = calculate_galaxy_position(model, node_idx);
    let size = calculate_star_size(node);
    let color = calculate_star_color(node);

    // Create glowing sphere
    let mesh = meshes.add(Sphere::new(size));
    let material = materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * 2.0,
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
        Text2d(node.name.clone()),
        TextFont {
            font_size: 50.0,
            ..default()
        },
        TextColor(Color::srgb(1.0, 1.0, 1.0)),
        TextLayout::new_with_justify(Justify::Center),
        Transform::from_translation(label_pos)
            .with_scale(Vec3::splat(0.1)),
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
    model: &FileSystemModel,
) {
    for node_idx in 0..model.total_nodes() {
        spawn_star(commands, meshes, materials, model, node_idx);
    }
}
