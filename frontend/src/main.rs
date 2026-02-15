// hello world
mod agent;
mod fs_model;
mod galaxy;
mod planet_material;
mod watcher;
mod ws_client;

use agent::{
    AgentArrivedEvent, AgentRegistry, FileEventHistory, HoveredFile, WsClientState,
    agent_despawn_system, agent_state_machine, agent_transform_system, file_highlight_system,
    on_file_star_out, on_file_star_over, process_spaceship_materials, process_ws_events,
};
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_fontmesh::FontMeshPlugin;

#[derive(Component)]
struct AmbientStar {
    speed: f32,
    color_offset: f32,
    initial_pos: Vec3,
    orbit_radius: f32,
    orbit_speed: f32,
}

#[derive(Component)]
struct OrbitCircle {
    fade_speed: f32,
    phase_offset: f32,
}
use crossbeam_channel::Receiver;
use fs_model::{FileSystemModel, GitignoreChecker, get_valid_paths};
use galaxy::{FileLabel, FileStar, spawn_star};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use watcher::{FileSystemEvent, start_file_watcher, watch_directory};
use ws_client::start_ws_client;

#[derive(Component)]
struct CameraModeButton {
    mode: CameraMode,
}

#[derive(Component)]
struct AgentActionsContainer;

#[derive(Component)]
struct FileStatsContainer;

#[derive(Component)]
struct ColorLegendContainer;

#[derive(Resource, Default)]
struct FileStats {
    visits: HashMap<PathBuf, usize>,
}

#[derive(Component)]
struct FileHoverPanel;

#[derive(Component)]
struct HoverPanelAnim {
    progress: f32,
    last_node: Option<usize>,
}

#[derive(Component)]
struct HoverGlow {
    progress: f32,
    base_emissive: LinearRgba,
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

    // Build file system model eagerly so the resource is available to all startup systems
    println!("Building file system model...");
    let model = FileSystemModel::build_initial(watch_path.clone());
    println!("Found {} files/directories", model.total_nodes());

    let gitignore_checker = GitignoreChecker::new(&watch_path);

    // Start file watcher
    let (rx, handle) = start_file_watcher(watch_path.clone());
    let handle = watch_directory(handle, watch_path.clone());

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
        .add_plugins(MeshPickingPlugin)
        .insert_resource(ClearColor(Color::srgb(0.05, 0.02, 0.15))) // Deep purple background
        .insert_resource(CameraController {
            mode: CameraMode::Auto,
            orbit_distance: 40.0,
            orbit_angle: 0.0,
            orbit_height: 20.0,
            is_dragging: false,
            last_mouse_pos: None,
        })
        .insert_resource(FileSystemState {
            model,
            event_receiver: rx,
            entity_map: HashMap::new(),
            gitignore_checker,
            root_path: watch_path,
            _watcher_handle: handle,
        })
        .insert_resource(WsClientState { receiver: ws_rx })
        .insert_resource(AgentRegistry::default())
        .insert_resource(FileStats::default())
        .insert_resource(FileEventHistory::default())
        .insert_resource(HoveredFile::default())
        .add_message::<AgentArrivedEvent>()
        .add_observer(on_file_star_over)
        .add_observer(on_file_star_out)
        .add_systems(
            Startup,
            (setup_camera, setup_lighting, setup_galaxy, setup_ui, setup_ambient_stars, setup_orbit_circles),
        )
        .add_systems(
            Update,
            (
                update_file_system,
                handle_camera_mode_buttons,
                update_camera,
                handle_manual_camera_input,
                billboard_labels,
                update_agent_actions_display,
                update_file_stats_display,
                track_file_visits,
                update_file_hover_panel,
                (
                    process_ws_events,
                    agent_state_machine,
                    agent_transform_system,
                    agent_despawn_system,
                    file_highlight_system,
                    process_spaceship_materials,
                )
                    .chain(),
                animate_ambient_stars,
                animate_orbit_circles,
                hover_glow_system,
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
                threshold: 1.5, // Lower threshold so files can bloom too
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

        let pos = Vec3::new(x, y, z);

        commands.spawn((
            AmbientStar {
                speed: 0.3 + t * 0.2,
                color_offset: t * std::f32::consts::TAU,
                initial_pos: pos,
                orbit_radius: 1.0 + t * 2.0,
                orbit_speed: 0.1 + t * 0.15,
            },
            Mesh3d(meshes.add(Sphere::new(0.15))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                emissive: LinearRgba::from(base_color) * 0.3,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    }
}

fn animate_ambient_stars(
    time: Res<Time>,
    mut query: Query<(&AmbientStar, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (ambient_star, mut transform, material_handle) in query.iter_mut() {
        let t = time.elapsed_secs() * ambient_star.speed + ambient_star.color_offset;

        // Gentle orbital movement around initial position
        let orbit_t = time.elapsed_secs() * ambient_star.orbit_speed;
        let offset = Vec3::new(
            ambient_star.orbit_radius * orbit_t.cos(),
            ambient_star.orbit_radius * (orbit_t * 0.5).sin() * 0.5,
            ambient_star.orbit_radius * orbit_t.sin(),
        );

        transform.translation = ambient_star.initial_pos + offset;

        // Cycle through colors smoothly
        if let Some(material) = materials.get_mut(&material_handle.0) {
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

fn animate_orbit_circles(
    time: Res<Time>,
    query: Query<(&OrbitCircle, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (orbit_circle, material_handle) in query.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let t = time.elapsed_secs() * orbit_circle.fade_speed + orbit_circle.phase_offset;

            // Very gentle fade between almost invisible and barely visible
            let alpha = 0.005 + 0.01 * (t.sin() * 0.5 + 0.5);

            // Preserve the RGB color, just update alpha
            let current_color = material.base_color;
            material.base_color = Color::srgba(
                current_color.to_srgba().red,
                current_color.to_srgba().green,
                current_color.to_srgba().blue,
                alpha
            );
        }
    }
}

fn setup_orbit_circles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Create orbit circles at various radii
    let radii = [10.0, 15.0, 20.0, 28.0, 35.0, 45.0, 60.0];

    for (i, &radius) in radii.iter().enumerate() {
        let t = i as f32 / radii.len() as f32;

        // Subtle color variation - pinks, purples, blues
        let color = if i % 3 == 0 {
            Color::srgba(1.0, 0.7, 0.9, 0.005) // Soft pink
        } else if i % 3 == 1 {
            Color::srgba(0.8, 0.7, 1.0, 0.005) // Soft purple
        } else {
            Color::srgba(0.7, 0.9, 1.0, 0.005) // Soft blue
        };

        // Create a torus with very thin cross-section to look like a circle
        let torus = Torus {
            minor_radius: 0.02,
            major_radius: radius,
        };

        commands.spawn((
            OrbitCircle {
                fade_speed: 0.3 + t * 0.2,
                phase_offset: t * std::f32::consts::TAU,
            },
            Mesh3d(meshes.add(torus)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            // Position at y=0 with no rotation - torus should be horizontal by default
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }
}

fn setup_ui(mut commands: Commands, fs_state: Res<FileSystemState>) {
    // Root UI container in bottom left
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(20.0),
                bottom: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        ))
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("Camera Mode"),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::WHITE),
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

    // Agent actions display at the top left
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(20.0),
                left: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            AgentActionsContainer,
        ));

    // File stats display above camera mode (bottom left, above the camera controls)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(20.0),
                bottom: Val::Px(180.0), // Position above camera mode
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            FileStatsContainer,
        ));

    // File hover panel at the top right (hidden by default)
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(20.0),
            right: Val::Px(20.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Start,
            row_gap: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            min_width: Val::Px(180.0),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.0)),
        BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.0)),
        FileHoverPanel,
        HoverPanelAnim {
            progress: 0.0,
            last_node: None,
        },
    ));

    // Color legend in bottom right
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                bottom: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            ColorLegendContainer,
        ))
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("File Types"),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // File type colors - matching galaxy.rs calculate_star_color
            let legend_items = [
                ("Directories", Color::srgb(1.0, 0.95, 0.7)),
                ("Rust (.rs)", Color::srgb(1.0, 0.75, 0.6)),
                ("Config", Color::srgb(1.0, 0.95, 0.6)),
                ("Docs (.md)", Color::srgb(0.9, 0.8, 1.0)),
                ("JavaScript", Color::srgb(1.0, 0.98, 0.7)),
                ("Python (.py)", Color::srgb(0.7, 0.85, 1.0)),
                ("Web (html/css)", Color::srgb(1.0, 0.7, 0.85)),
                ("Compiled", Color::srgb(0.85, 0.75, 1.0)),
                ("Go", Color::srgb(0.7, 0.9, 1.0)),
            ];

            for (label, color) in legend_items {
                // Create a container for each legend item with square + label
                parent.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                }).with_children(|item| {
                    // Colored square
                    item.spawn((
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));

                    // Label text
                    item.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });
            }
        });
}

fn setup_galaxy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut fs_state: ResMut<FileSystemState>,
) {
    // Spawn initial galaxy stars from the already-built file system model
    for node_idx in 0..fs_state.model.total_nodes() {
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
    _time: Res<Time>,
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

fn update_agent_actions_display(
    mut commands: Commands,
    agents: Query<&agent::Agent>,
    container_query: Query<Entity, With<AgentActionsContainer>>,
    children_query: Query<&Children>,
    windows: Query<&Window>,
    asset_server: Res<AssetServer>,
) {
    // Get the container entity
    let Ok(container) = container_query.single() else {
        return;
    };

    // Get window size for responsive text
    let Ok(window) = windows.single() else {
        return;
    };
    let base_font_size = (window.width() / 80.0).clamp(14.0, 24.0);
    let _title_font_size = base_font_size * 1.5;
    let action_font_size = base_font_size * 0.75;

    // Despawn all existing child text entities
    if let Ok(children) = children_query.get(container) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Collect all active agents with their actions and colors
    let mut agent_actions: Vec<(String, String, Color)> = agents
        .iter()
        .filter_map(|agent| {
            agent.current_action.as_ref().map(|action| {
                (agent.session_id.clone(), action.clone(), agent.color)
            })
        })
        .collect();

    // If there are no active actions, show a placeholder
    if agent_actions.is_empty() {
        commands.entity(container).with_children(|parent| {
            parent.spawn((
                Text::new("No active agents"),
                TextFont {
                    font_size: action_font_size,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
            ));
        });
        return;
    }

    // Sort by session_id for consistent ordering
    agent_actions.sort_by(|a, b| a.0.cmp(&b.0));

    // Add a text entity for each active action
    commands.entity(container).with_children(|parent| {
        parent.spawn((
            Text::new("Agent Activity"),
            TextFont {
                font_size: 22.0,
                ..default()
            },
            TextColor(Color::WHITE),
        ));

        // Greek alphabet symbols
        let greek_symbols = ["α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ",
                             "ν", "ξ", "ο", "π", "ρ", "σ", "τ", "υ", "φ", "χ", "ψ", "ω"];

        // Load a font that supports Greek characters
        let greek_font = asset_server.load("fonts/FiraMono-Medium.ttf");

        // Action list - each agent uses their unique color and Greek symbol
        for (i, (_session_id, action, color)) in agent_actions.iter().enumerate() {
            let symbol = greek_symbols[i % greek_symbols.len()];
            parent.spawn((
                Text::new(format!("{} {}", symbol, action)),
                TextFont {
                    font: greek_font.clone(),
                    font_size: action_font_size,
                    ..default()
                },
                TextColor(*color),
            ));
        }
    });
}

// Generate a consistent color for an agent based on their session_id
fn generate_agent_color(session_id: &str) -> Color {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    let hash = hasher.finish();

    // Use hash to generate vibrant, distinguishable colors
    let hue = (hash % 360) as f32;
    let saturation = 0.7 + ((hash >> 8) % 30) as f32 / 100.0; // 0.7-1.0
    let lightness = 0.6 + ((hash >> 16) % 20) as f32 / 100.0; // 0.6-0.8

    // Convert HSL to RGB
    hsl_to_rgb(hue, saturation, lightness)
}

fn track_file_visits(
    mut file_stats: ResMut<FileStats>,
    mut arrived_events: MessageReader<AgentArrivedEvent>,
    fs_state: Res<FileSystemState>,
) {
    for event in arrived_events.read() {
        // Get the file path for this node
        if let Some(node) = fs_state.model.nodes.get(event.node_index) {
            let path = node.path.clone();
            *file_stats.visits.entry(path).or_insert(0) += 1;
        }
    }
}

fn update_file_stats_display(
    mut commands: Commands,
    file_stats: Res<FileStats>,
    fs_state: Res<FileSystemState>,
    container_query: Query<Entity, With<FileStatsContainer>>,
    children_query: Query<&Children>,
) {
    let Ok(container) = container_query.single() else {
        return;
    };

    // Despawn all existing children
    if let Ok(children) = children_query.get(container) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Get top 6 most visited files
    let mut sorted_visits: Vec<_> = file_stats.visits.iter().collect();
    sorted_visits.sort_by(|a, b| b.1.cmp(a.1));
    let top_6: Vec<_> = sorted_visits.into_iter().take(6).collect();

    commands.entity(container).with_children(|parent| {
        if top_6.is_empty() {
            parent.spawn((
                Text::new("No activity yet"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
            ));
        } else {
            for (path, count) in top_6 {
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                // Get node color from galaxy
                let color = if let Some((node_idx, _)) = fs_state.model.get_node_by_path(path) {
                    let node = &fs_state.model.nodes[node_idx];
                    galaxy::calculate_star_color(node)
                } else {
                    Color::srgb(0.7, 0.7, 0.7)
                };

                parent.spawn((
                    Text::new(format!("{}  {} edits", filename, count)),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(color),
                ));
            }
        }
    });
}

// Convert HSL to RGB color
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color::srgb(r + m, g + m, b + m)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn extract_time_from_rfc3339(ts: &str) -> &str {
    // RFC3339: "2024-01-15T14:30:45.123Z" or "2024-01-15T14:30:45+00:00"
    // Extract HH:MM:SS portion
    if let Some(t_pos) = ts.find('T') {
        let after_t = &ts[t_pos + 1..];
        // Take up to 8 chars for HH:MM:SS
        if after_t.len() >= 8 {
            &after_t[..8]
        } else {
            after_t
        }
    } else {
        ts
    }
}

fn tool_color(tool_name: &str) -> Color {
    match tool_name {
        "Read" => Color::srgb(0.4, 0.9, 0.9),   // Cyan
        "Write" => Color::srgb(1.0, 0.65, 0.3),  // Orange
        "Edit" => Color::srgb(0.4, 0.9, 0.4),    // Green
        _ => Color::srgb(0.7, 0.7, 0.7),          // Gray
    }
}

fn update_file_hover_panel(
    time: Res<Time>,
    mut commands: Commands,
    hovered: Res<HoveredFile>,
    event_history: Res<FileEventHistory>,
    fs_state: Res<FileSystemState>,
    mut panel_query: Query<
        (Entity, &mut Node, &mut BackgroundColor, &mut BorderColor, &mut HoverPanelAnim),
        With<FileHoverPanel>,
    >,
    children_query: Query<&Children>,
    windows: Query<&Window>,
) {
    let Ok((panel_entity, mut panel_node, mut bg_color, mut border_color, mut anim)) =
        panel_query.single_mut()
    else {
        return;
    };

    let dt = time.delta_secs();

    // Track which node to display (keep last hovered for fade-out)
    if hovered.0.is_some() {
        anim.last_node = hovered.0;
    }

    // Animate progress toward target
    let target = if hovered.0.is_some() { 1.0 } else { 0.0 };
    let speed = if target > anim.progress { 6.0 } else { 4.0 };
    if anim.progress < target {
        anim.progress = (anim.progress + dt * speed).min(1.0);
    } else if anim.progress > target {
        anim.progress = (anim.progress - dt * speed).max(0.0);
    }

    let t = ease_out_cubic(anim.progress);

    // Despawn old children
    if let Ok(children) = children_query.get(panel_entity) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Fully hidden
    if anim.progress <= 0.001 {
        panel_node.display = Display::None;
        return;
    }

    panel_node.display = Display::Flex;

    // Animate position (subtle slide down on enter)
    panel_node.top = Val::Px(20.0 + (1.0 - t) * 10.0);

    // Animate background and border alpha
    *bg_color = BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.92 * t));
    *border_color = BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.3 * t));

    // Content
    let Some(node_idx) = anim.last_node else {
        return;
    };

    // Get font sizes
    let base_font_size = if let Ok(window) = windows.single() {
        (window.width() / 80.0).clamp(14.0, 24.0)
    } else {
        16.0
    };
    let title_font_size = base_font_size * 1.2;
    let event_font_size = base_font_size * 0.75;

    // Get file name
    let file_name = if node_idx < fs_state.model.nodes.len() {
        fs_state.model.nodes[node_idx].name.clone()
    } else {
        "Unknown".to_string()
    };

    let alpha = t;

    commands.entity(panel_entity).with_children(|parent| {
        // Title: file name
        parent.spawn((
            Text::new(file_name),
            TextFont {
                font_size: title_font_size,
                ..default()
            },
            TextColor(Color::srgba(1.0, 1.0, 1.0, alpha)),
        ));

        // Thin accent separator
        parent.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.5, 0.3, 0.8, 0.5 * alpha)),
        ));

        // Get events for this file
        if let Some(events) = event_history.map.get(&node_idx) {
            // Show last 3 events, most recent first
            let recent: Vec<_> = events.iter().rev().take(3).collect();
            for event in recent {
                let time_str = event
                    .timestamp
                    .as_deref()
                    .map(extract_time_from_rfc3339)
                    .unwrap_or("--:--:--");

                let base_color = tool_color(&event.tool_name);
                let srgba = base_color.to_srgba();
                let color = Color::srgba(srgba.red, srgba.green, srgba.blue, alpha);
                let label = format!("{} [{}]", event.tool_name, time_str);

                parent.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: event_font_size,
                        ..default()
                    },
                    TextColor(color),
                ));
            }
        } else {
            parent.spawn((
                Text::new("No recent events"),
                TextFont {
                    font_size: event_font_size,
                    ..default()
                },
                TextColor(Color::srgba(0.5, 0.5, 0.5, alpha)),
            ));
        }
    });
}

fn hover_glow_system(
    time: Res<Time>,
    hovered: Res<HoveredFile>,
    fs_state: Res<FileSystemState>,
    mut commands: Commands,
    stars: Query<&MeshMaterial3d<StandardMaterial>, (With<FileStar>, Without<HoverGlow>)>,
    mut glowing: Query<(Entity, &FileStar, &mut HoverGlow, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let hovered_idx = hovered.0;

    // Add HoverGlow to newly hovered star
    if let Some(node_idx) = hovered_idx {
        if let Some(&star_entity) = fs_state.entity_map.get(&node_idx) {
            if let Ok(mat_handle) = stars.get(star_entity) {
                if let Some(material) = materials.get(mat_handle) {
                    commands.entity(star_entity).insert(HoverGlow {
                        progress: 0.0,
                        base_emissive: material.emissive,
                    });
                }
            }
        }
    }

    // Update all glowing stars
    for (entity, star, mut glow, mat_handle) in glowing.iter_mut() {
        let is_hovered = hovered_idx == Some(star.node_index);

        if is_hovered {
            glow.progress = (glow.progress + dt * 4.0).min(1.0);
        } else {
            glow.progress -= dt * 3.0;
        }

        if glow.progress <= 0.0 {
            // Restore original emissive and remove
            if let Some(material) = materials.get_mut(mat_handle) {
                material.emissive = glow.base_emissive;
            }
            commands.entity(entity).remove::<HoverGlow>();
        } else {
            // Apply animated glow with subtle pulse
            if let Some(material) = materials.get_mut(mat_handle) {
                let t = ease_out_cubic(glow.progress);
                let pulse = (time.elapsed_secs() * 3.0).sin() * 0.12 + 0.88;
                let boost = 1.0 + t * 2.5 * pulse;
                material.emissive = glow.base_emissive * boost;
            }
        }
    }
}
