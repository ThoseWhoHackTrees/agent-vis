// hello world
mod agent;
mod fs_model;
mod galaxy;
mod watcher;
mod ws_client;

use agent::{
    AgentArrivedEvent, AgentRegistry, WsClientState, agent_despawn_system, agent_state_machine,
    agent_transform_system, file_highlight_system, process_ws_events,
};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_fontmesh::FontMeshPlugin;

#[derive(Component)]
struct AmbientStar {
    speed: f32,
    color_offset: f32,
}
use crossbeam_channel::Receiver;
use fs_model::{FileSystemModel, GitignoreChecker, get_valid_paths};
use galaxy::{FileLabel, spawn_star};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use watcher::{FileSystemEvent, start_file_watcher, watch_directory};
use ws_client::start_ws_client;

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
    root_path: PathBuf,
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

    // Start WebSocket client
    let (ws_rx, _ws_handle) = start_ws_client();

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
        .insert_resource(ClearColor(Color::srgb(0.05, 0.02, 0.15))) // Deep purple background
        .insert_resource(CameraController {
            mode: CameraMode::Auto,
            orbit_distance: 40.0,
            orbit_angle: 0.0,
            orbit_height: 20.0,
            is_dragging: false,
            last_mouse_pos: None,
        })
        .insert_resource(WsClientState { receiver: ws_rx })
        .insert_resource(AgentRegistry::default())
        .add_message::<AgentArrivedEvent>()
        .add_systems(
            Startup,
            (setup_camera, setup_lighting, setup_galaxy, setup_ui, setup_ambient_stars),
        )
        .add_systems(
            Update,
            (
                update_file_system,
                handle_camera_mode_buttons,
                update_camera,
                handle_manual_camera_input,
                billboard_labels,
                (
                    process_ws_events,
                    agent_state_machine,
                    agent_transform_system,
                    agent_despawn_system,
                    file_highlight_system,
                )
                    .chain(),
                animate_ambient_stars,
            ),
        )
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
            illuminance: 800.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Add ambient colored point lights for gradient feel
    // Pink light from top
    commands.spawn((
        PointLight {
            color: Color::srgb(1.0, 0.3, 0.7),
            intensity: 100000.0,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(0.0, 30.0, 0.0),
    ));

    // Blue light from bottom left
    commands.spawn((
        PointLight {
            color: Color::srgb(0.2, 0.5, 1.0),
            intensity: 80000.0,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(-30.0, -10.0, -30.0),
    ));

    // Purple light from right
    commands.spawn((
        PointLight {
            color: Color::srgb(0.6, 0.2, 0.9),
            intensity: 90000.0,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(30.0, 0.0, 30.0),
    ));
}

fn setup_ambient_stars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Spawn dim colored stars in the background
    let star_count = 100;
    let range = 80.0;

    for i in 0..star_count {
        let t = i as f32 / star_count as f32;

        // Random-ish position using pseudo-random distribution
        let angle1 = t * std::f32::consts::TAU * 7.0;
        let angle2 = t * std::f32::consts::TAU * 13.0;
        let radius = 50.0 + (t * 30.0);

        let x = radius * angle1.cos() * angle2.sin();
        let y = (t - 0.5) * range * 2.0;
        let z = radius * angle1.sin() * angle2.cos();

        // Color from palette: pinks, purples, yellows, blues
        let color_choice = (i % 4) as f32 / 4.0;
        let base_color = if color_choice < 0.25 {
            Color::srgb(1.0, 0.4, 0.7) // Pink
        } else if color_choice < 0.5 {
            Color::srgb(0.6, 0.3, 1.0) // Purple
        } else if color_choice < 0.75 {
            Color::srgb(1.0, 0.9, 0.4) // Yellow
        } else {
            Color::srgb(0.4, 0.7, 1.0) // Blue
        };

        commands.spawn((
            AmbientStar {
                speed: 0.3 + t * 0.2,
                color_offset: t * std::f32::consts::TAU,
            },
            Mesh3d(meshes.add(Sphere::new(0.15))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                emissive: LinearRgba::from(base_color) * 0.3,
                ..default()
            })),
            Transform::from_translation(Vec3::new(x, y, z)),
        ));
    }
}

fn animate_ambient_stars(
    time: Res<Time>,
    mut query: Query<(&AmbientStar, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (ambient_star, material_handle) in query.iter_mut() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let t = time.elapsed_secs() * ambient_star.speed + ambient_star.color_offset;

            // Cycle through colors smoothly
            let color = Color::srgb(
                0.5 + 0.5 * (t).sin(),
                0.5 + 0.5 * (t + 2.0).sin(),
                0.5 + 0.5 * (t + 4.0).sin(),
            );

            material.base_color = color;
            material.emissive = LinearRgba::from(color) * 0.3;
        }
    }
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
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|buttons| {
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
                            CameraModeButton {
                                mode: CameraMode::Auto,
                            },
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
                            CameraModeButton {
                                mode: CameraMode::Manual,
                            },
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
                            CameraModeButton {
                                mode: CameraMode::Follow,
                            },
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
    let root_path = watch_path.clone();
    let (rx, handle) = start_file_watcher(watch_path.clone());
    let handle = watch_directory(handle, watch_path);

    // Spawn initial galaxy
    let mut entity_map = HashMap::new();
    for node_idx in 0..model.total_nodes() {
        let entity = spawn_star(
            &mut commands,
            &mut meshes,
            &mut materials,
            &asset_server,
            &model,
            node_idx,
        );
        entity_map.insert(node_idx, entity);
    }

    commands.insert_resource(FileSystemState {
        model,
        event_receiver: rx,
        entity_map,
        gitignore_checker,
        root_path,
        _watcher_handle: handle,
    });
}

fn is_gitignore_file(path: &PathBuf) -> bool {
    path.file_name().map(|n| n == ".gitignore").unwrap_or(false)
}

fn despawn_star_with_label(
    commands: &mut Commands,
    star_entity: Entity,
    label_query: &Query<(Entity, &FileLabel)>,
) {
    commands.entity(star_entity).despawn();
    for (label_entity, file_label) in label_query.iter() {
        if file_label.star_entity == star_entity {
            commands.entity(label_entity).despawn();
            break;
        }
    }
}

fn update_file_system(
    mut fs_state: ResMut<FileSystemState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    label_query: Query<(Entity, &FileLabel)>,
) {
    let mut gitignore_changed = false;

    // Process all pending file system events
    while let Ok(event) = fs_state.event_receiver.try_recv() {
        match event {
            FileSystemEvent::Created(path, is_dir) => {
                if is_gitignore_file(&path) {
                    gitignore_changed = true;
                }

                // Skip if ignored by gitignore
                if fs_state.gitignore_checker.is_ignored(&path) {
                    continue;
                }

                println!(
                    "Created: {} ({})",
                    path.display(),
                    if is_dir { "dir" } else { "file" }
                );

                if let Some(node_idx) = fs_state.model.add_node(path, is_dir) {
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
                if is_gitignore_file(&path) {
                    gitignore_changed = true;
                }

                println!("Deleted: {}", path.display());

                // Always process deletions — the file may have been in the model
                if let Some(node_idx) = fs_state.model.remove_node(&path) {
                    if let Some(entity) = fs_state.entity_map.remove(&node_idx) {
                        despawn_star_with_label(&mut commands, entity, &label_query);
                    }
                }
            }
            FileSystemEvent::Modified(path) => {
                if is_gitignore_file(&path) {
                    gitignore_changed = true;
                }

                println!("Modified: {}", path.display());
            }
        }
    }

    // When .gitignore changes, reconcile: remove now-ignored files, add now-visible files
    if gitignore_changed {
        println!("Gitignore changed, reconciling visualization...");
        let valid_paths = get_valid_paths(&fs_state.root_path);

        // Remove stars for paths that are now gitignored
        let paths_to_remove: Vec<PathBuf> = fs_state
            .model
            .path_to_index
            .keys()
            .filter(|p| !valid_paths.contains(*p))
            .cloned()
            .collect();

        for path in &paths_to_remove {
            println!("Removing now-ignored: {}", path.display());
            if let Some(node_idx) = fs_state.model.remove_node(path) {
                if let Some(entity) = fs_state.entity_map.remove(&node_idx) {
                    despawn_star_with_label(&mut commands, entity, &label_query);
                }
            }
        }

        // Add stars for paths that are now visible (were previously ignored)
        for path in &valid_paths {
            if !fs_state.model.path_to_index.contains_key(path) {
                let is_dir = path.is_dir();
                if let Some(node_idx) = fs_state.model.add_node(path.clone(), is_dir) {
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
        }
    }
}

fn handle_camera_mode_buttons(
    mut controller: ResMut<CameraController>,
    interaction_query: Query<(&Interaction, &CameraModeButton), Changed<Interaction>>,
    mut all_buttons: Query<(&CameraModeButton, &Interaction, &mut BackgroundColor)>,
) {
    // Check for button presses
    for (interaction, button) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            controller.mode = button.mode;
        }
    }

    // Update all button colors based on current mode
    for (button, interaction, mut bg_color) in all_buttons.iter_mut() {
        match *interaction {
            Interaction::Hovered => {
                if controller.mode != button.mode {
                    *bg_color = BackgroundColor(Color::srgb(0.3, 0.3, 0.3));
                } else {
                    *bg_color = BackgroundColor(Color::srgb(0.3, 0.5, 0.8));
                }
            }
            Interaction::Pressed | Interaction::None => {
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
