// hello world
mod agent;
mod fs_model;
mod galaxy;
mod watcher;
mod ws_client;

use agent::{
    agent_despawn_system, agent_state_machine, agent_transform_system, file_highlight_system,
    process_ws_events, AgentArrivedEvent, AgentRegistry, WsClientState,
};
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_fontmesh::FontMeshPlugin;
use crossbeam_channel::Receiver;
use fs_model::{FileSystemModel, GitignoreChecker, get_valid_paths};
use galaxy::{spawn_star, FileLabel};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use watcher::{start_file_watcher, watch_directory, FileSystemEvent};
use ws_client::start_ws_client;

#[derive(Resource)]
struct FileSystemState {
    model: FileSystemModel,
    event_receiver: Receiver<FileSystemEvent>,
    entity_map: HashMap<usize, Entity>, // node_index -> Entity
    gitignore_checker: GitignoreChecker,
    root_path: PathBuf,
    _watcher_handle: watcher::FileWatcherHandle,
}

#[derive(Resource)]
struct CameraController {
    orbit_distance: f32,
    orbit_angle: f32,
    orbit_height: f32,
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
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.05)))
        .insert_resource(CameraController {
            orbit_distance: 40.0,
            orbit_angle: 0.0,
            orbit_height: 20.0,
        })
        .insert_resource(WsClientState { receiver: ws_rx })
        .insert_resource(AgentRegistry::default())
        .add_message::<AgentArrivedEvent>()
        .add_systems(Startup, (setup_camera, setup_lighting, setup_galaxy))
        .add_systems(
            Update,
            (
                update_file_system,
                camera_orbit,
                billboard_labels,
                (
                    process_ws_events,
                    agent_state_machine,
                    agent_transform_system,
                    agent_despawn_system,
                    file_highlight_system,
                )
                    .chain(),
            ),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    // Spawn 3D camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(30.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn setup_lighting(mut commands: Commands) {
    // Directional light
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
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
        let entity = spawn_star(&mut commands, &mut meshes, &mut materials, &asset_server, &model, node_idx);
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

                println!("Created: {} ({})", path.display(), if is_dir { "dir" } else { "file" });

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

fn camera_orbit(
    time: Res<Time>,
    mut controller: ResMut<CameraController>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    // Slowly orbit camera
    controller.orbit_angle += time.delta_secs() * 0.1;

    let x = controller.orbit_distance * controller.orbit_angle.cos();
    let z = controller.orbit_distance * controller.orbit_angle.sin();
    let y = controller.orbit_height;

    if let Ok(mut transform) = camera_query.single_mut() {
        *transform = Transform::from_xyz(x, y, z).looking_at(Vec3::ZERO, Vec3::Y);
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
