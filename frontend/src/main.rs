mod fs_model;
mod galaxy;
mod watcher;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy_fontmesh::FontMeshPlugin;
use crossbeam_channel::Receiver;
use fs_model::{FileSystemModel, GitignoreChecker};
use galaxy::{spawn_star, FileLabel};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use watcher::{start_file_watcher, watch_directory, FileSystemEvent};

#[derive(Component)]
struct CameraModeButton {
    mode: CameraMode,
}

#[derive(Resource)]
struct FileSystemState {
    model: FileSystemModel,
    event_receiver: Receiver<FileSystemEvent>,
    entity_map: HashMap<usize, Entity>, // node_index -> Entity
    gitignore_checker: GitignoreChecker,
    _watcher_handle: watcher::FileWatcherHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CameraMode {
    Auto,
    Manual,
    Follow,
}

#[derive(Resource)]
struct CameraController {
    mode: CameraMode,
    orbit_distance: f32,
    orbit_angle: f32,
    orbit_height: f32,
    // Manual mode state
    is_dragging: bool,
    last_mouse_pos: Option<Vec2>,
}

fn main() {
    // Get directory to watch from command line args
    let args: Vec<String> = env::args().collect();
    let watch_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        println!("Usage: {} <directory-to-watch>", args[0]);
        println!("No directory specified, watching current directory");
        PathBuf::from(".")
    };

    // Canonicalize the path
    let watch_path = watch_path
        .canonicalize()
        .expect("Failed to resolve watch path");

    println!("Watching directory: {}", watch_path.display());

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "File System Galaxy".to_string(),
                resolution: WindowResolution::new(1920, 1080),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FontMeshPlugin)
        .insert_resource(ClearColor(Color::srgb(0.0, 0.0, 0.02)))
        .insert_resource(CameraController {
            mode: CameraMode::Auto,
            orbit_distance: 40.0,
            orbit_angle: 0.0,
            orbit_height: 20.0,
            is_dragging: false,
            last_mouse_pos: None,
        })
        .add_systems(Startup, (setup_camera, setup_lighting, setup_galaxy, setup_ui))
        .add_systems(Update, update_file_system)
        .add_systems(Update, handle_camera_mode_buttons)
        .add_systems(Update, update_camera)
        .add_systems(Update, handle_manual_camera_input)
        .add_systems(Update, billboard_labels)
        .run();
}

fn setup_camera(mut commands: Commands) {
    // Spawn 3D camera with bloom
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(30.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        Bloom {
            intensity: 0.2,
            low_frequency_boost: 0.3,
            low_frequency_boost_curvature: 0.95,
            high_pass_frequency: 1.0,
            composite_mode: BloomCompositeMode::Additive,
            prefilter: BloomPrefilter {
                threshold: 3.0, // Only emissive values > 3.0 will bloom
                threshold_softness: 0.5,
            },
            ..default()
        },
    ));
}

fn setup_lighting(mut commands: Commands) {
    // Dim directional light to let stars bloom
    commands.spawn((
        DirectionalLight {
            illuminance: 1000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn setup_ui(mut commands: Commands) {
    // Root UI container in bottom left
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(20.0),
            bottom: Val::Px(20.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("Camera Mode"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
            ));

            // Button container
            parent.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                ..default()
            }).with_children(|buttons| {
                // Auto button
                buttons
                    .spawn((
                        Button,
                        Node {
                            padding: UiRect::all(Val::Px(10.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.3, 0.5, 0.8)),
                        BorderColor::all(Color::srgb(0.5, 0.5, 0.5)),
                        CameraModeButton { mode: CameraMode::Auto },
                    ))
                    .with_child((
                        Text::new("Auto"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));

                // Manual button
                buttons
                    .spawn((
                        Button,
                        Node {
                            padding: UiRect::all(Val::Px(10.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
                        BorderColor::all(Color::srgb(0.5, 0.5, 0.5)),
                        CameraModeButton { mode: CameraMode::Manual },
                    ))
                    .with_child((
                        Text::new("Manual"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));

                // Follow button
                buttons
                    .spawn((
                        Button,
                        Node {
                            padding: UiRect::all(Val::Px(10.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.2, 0.2, 0.2)),
                        BorderColor::all(Color::srgb(0.5, 0.5, 0.5)),
                        CameraModeButton { mode: CameraMode::Follow },
                    ))
                    .with_child((
                        Text::new("Follow"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
            });
        });
}

fn setup_galaxy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Get watch path from command line
    let args: Vec<String> = env::args().collect();
    let watch_path = if args.len() > 1 {
        PathBuf::from(&args[1])
            .canonicalize()
            .expect("Failed to resolve watch path")
    } else {
        PathBuf::from(".")
            .canonicalize()
            .expect("Failed to resolve current directory")
    };

    println!("Building file system model...");
    let model = FileSystemModel::build_initial(watch_path.clone());
    println!("Found {} files/directories", model.total_nodes());

    // Create gitignore checker
    let gitignore_checker = GitignoreChecker::new(&watch_path);

    // Start file watcher
    let (rx, handle) = start_file_watcher(watch_path.clone());
    let handle = watch_directory(handle, watch_path);

    // Spawn initial galaxy
    let mut entity_map = HashMap::new();
    for node_idx in 0..model.total_nodes() {
        let entity = spawn_star(&mut commands, &mut meshes, &mut materials, &asset_server, &model, node_idx);
        entity_map.insert(node_idx, entity);
    }

    commands.insert_resource(FileSystemState {
        model,
        event_receiver: rx,
        entity_map,
        gitignore_checker,
        _watcher_handle: handle,
    });
}

fn update_file_system(
    mut fs_state: ResMut<FileSystemState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Process all pending file system events
    while let Ok(event) = fs_state.event_receiver.try_recv() {
        match event {
            FileSystemEvent::Created(path, is_dir) => {
                // Skip if ignored by gitignore
                if fs_state.gitignore_checker.is_ignored(&path) {
                    continue;
                }

                println!("Created: {} ({})", path.display(), if is_dir { "dir" } else { "file" });

                if let Some(node_idx) = fs_state.model.add_node(path, is_dir) {
                    // Spawn new star
                    let entity = spawn_star(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &asset_server,
                        &fs_state.model,
                        node_idx,
                    );
                    fs_state.entity_map.insert(node_idx, entity);
                }
            }
            FileSystemEvent::Deleted(path) => {
                // Skip if ignored by gitignore
                if fs_state.gitignore_checker.is_ignored(&path) {
                    continue;
                }

                println!("Deleted: {}", path.display());

                if let Some(node_idx) = fs_state.model.remove_node(&path) {
                    // Despawn star
                    if let Some(entity) = fs_state.entity_map.remove(&node_idx) {
                        commands.entity(entity).despawn();
                    }
                }
            }
            FileSystemEvent::Modified(path) => {
                // Skip if ignored by gitignore
                if fs_state.gitignore_checker.is_ignored(&path) {
                    continue;
                }

                println!("Modified: {}", path.display());
                // Could update star appearance here
            }
        }
    }
}

fn handle_camera_mode_buttons(
    mut controller: ResMut<CameraController>,
    mut interaction_query: Query<
        (&Interaction, &CameraModeButton, &mut BackgroundColor),
        Changed<Interaction>,
    >,
) {
    for (interaction, button, mut bg_color) in interaction_query.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                controller.mode = button.mode;
            }
            Interaction::Hovered => {
                if controller.mode != button.mode {
                    *bg_color = BackgroundColor(Color::srgb(0.3, 0.3, 0.3));
                }
            }
            Interaction::None => {
                if controller.mode == button.mode {
                    *bg_color = BackgroundColor(Color::srgb(0.3, 0.5, 0.8));
                } else {
                    *bg_color = BackgroundColor(Color::srgb(0.2, 0.2, 0.2));
                }
            }
        }
    }
}

fn update_camera(
    time: Res<Time>,
    controller: Res<CameraController>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    match controller.mode {
        CameraMode::Auto | CameraMode::Follow => {
            // Auto orbit (Follow will do the same for now)
            let angle = controller.orbit_angle;
            let x = controller.orbit_distance * angle.cos();
            let z = controller.orbit_distance * angle.sin();
            let y = controller.orbit_height;

            if let Ok(mut transform) = camera_query.single_mut() {
                *transform = Transform::from_xyz(x, y, z).looking_at(Vec3::ZERO, Vec3::Y);
            }
        }
        CameraMode::Manual => {
            // Manual mode - camera position is controlled by input
            let x = controller.orbit_distance * controller.orbit_angle.cos();
            let z = controller.orbit_distance * controller.orbit_angle.sin();
            let y = controller.orbit_height;

            if let Ok(mut transform) = camera_query.single_mut() {
                *transform = Transform::from_xyz(x, y, z).looking_at(Vec3::ZERO, Vec3::Y);
            }
        }
    }
}

fn handle_manual_camera_input(
    mut controller: ResMut<CameraController>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Auto mode updates angle automatically
    if controller.mode == CameraMode::Auto || controller.mode == CameraMode::Follow {
        controller.orbit_angle += time.delta_secs() * 0.1;
        return;
    }

    // Manual mode controls
    if controller.mode != CameraMode::Manual {
        return;
    }

    // Arrow keys for navigation
    let move_speed = 20.0 * time.delta_secs();
    let rotate_speed = 2.0 * time.delta_secs();

    // Up/Down arrows: zoom in/out
    if keyboard.pressed(KeyCode::ArrowUp) {
        controller.orbit_distance -= move_speed;
        controller.orbit_distance = controller.orbit_distance.clamp(10.0, 100.0);
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        controller.orbit_distance += move_speed;
        controller.orbit_distance = controller.orbit_distance.clamp(10.0, 100.0);
    }

    // Left/Right arrows: rotate around
    if keyboard.pressed(KeyCode::ArrowLeft) {
        controller.orbit_angle -= rotate_speed;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        controller.orbit_angle += rotate_speed;
    }

    // W/S keys: adjust height
    if keyboard.pressed(KeyCode::KeyW) {
        controller.orbit_height += move_speed * 0.5;
        controller.orbit_height = controller.orbit_height.clamp(5.0, 50.0);
    }
    if keyboard.pressed(KeyCode::KeyS) {
        controller.orbit_height -= move_speed * 0.5;
        controller.orbit_height = controller.orbit_height.clamp(5.0, 50.0);
    }
}

fn billboard_labels(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    star_query: Query<&Transform, With<galaxy::FileStar>>,
    mut label_query: Query<(&mut Transform, &FileLabel), Without<galaxy::FileStar>>,
) {
    if let Ok(camera_transform) = camera_query.single() {
        // Get camera rotation
        let (_, camera_rotation, _) = camera_transform.to_scale_rotation_translation();

        for (mut label_transform, file_label) in label_query.iter_mut() {
            // Update label position to follow its star
            if let Ok(star_transform) = star_query.get(file_label.star_entity) {
                label_transform.translation = star_transform.translation + file_label.offset;
            }

            // Make the label face the camera
            label_transform.rotation = camera_rotation;
        }
    }
}
