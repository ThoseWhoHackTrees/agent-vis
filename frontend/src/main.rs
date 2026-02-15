// hello world
mod agent;
mod fs_model;
mod galaxy;
mod planet_material;
mod watcher;
mod ws_client;

use agent::{
    AgentArrivedEvent, AgentRegistry, FileEventHistory, HoveredFile, WsClientState,
    agent_despawn_system, agent_state_machine, agent_transform_system, cleanup_agent_labels,
    file_highlight_system, on_file_star_out, on_file_star_over, process_spaceship_materials,
    process_ws_events, update_agent_action_bubble_content, update_agent_action_bubble_transforms,
    update_agent_nameplates,
};
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::WindowResolution;
use bevy_fontmesh::FontMeshPlugin;
use planet_material::PlanetMaterial;

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

#[derive(Component)]
struct PromptContainer;

#[derive(Component)]
struct PromptInputField;

#[derive(Component)]
struct PromptSubmitButton;

#[derive(Component)]
struct HelpButton;

#[derive(Component)]
struct TipsOverlay;

#[derive(Component)]
struct CloseOverlayButton;

#[derive(Component)]
struct IdleSpaceship {
    float_offset: f32,
    pulse_phase: f32,
}

#[derive(Resource, Default)]
struct PromptInputState {
    text: String,
    is_focused: bool,
}

#[derive(Component)]
struct BlinkingCursor {
    timer: f32,
    visible: bool,
}

#[derive(Resource, Default)]
struct PendingAgentTask {
    session_id: Option<String>,
    task_description: String,
}

#[derive(Resource)]
struct TipsState {
    visible: bool,
    has_been_shown: bool,
}

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
                title: "Space Agents!".to_string(),
                resolution: WindowResolution::new(1920, 1080),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FontMeshPlugin)
        .add_plugins(MaterialPlugin::<PlanetMaterial>::default())
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
        .insert_resource(PromptInputState::default())
        .insert_resource(PendingAgentTask::default())
        .insert_resource(TipsState {
            visible: true, // Show on first load
            has_been_shown: false,
        })
        .add_message::<AgentArrivedEvent>()
        .add_observer(on_file_star_over)
        .add_observer(on_file_star_out)
        .add_systems(
            Startup,
            (
                setup_camera,
                setup_lighting,
                setup_galaxy,
                setup_ui,
                setup_vignette,
                setup_ambient_stars,
                setup_orbit_circles,
            ),
        )
        .add_systems(
            Update,
            (
                update_file_system,
                handle_camera_mode_buttons,
                update_camera,
                handle_manual_camera_input,
                billboard_labels,
                update_agent_nameplates,
                update_agent_action_bubble_transforms,
                update_agent_action_bubble_content,
                cleanup_agent_labels,
                update_agent_actions_display,
                update_file_stats_display,
                track_file_visits,
                update_file_hover_panel,
                animate_ambient_stars,
                animate_orbit_circles,
                hover_glow_system,
            ),
        )
        .add_systems(
            Update,
            (
                handle_prompt_focus,
                handle_prompt_unfocus,
                handle_prompt_input,
                handle_prompt_submit,
                apply_pending_agent_tasks,
                update_prompt_display,
                animate_cursor,
                handle_help_button,
                handle_close_overlay,
                update_tips_overlay,
            ),
        )
        .add_systems(
            Update,
            (
                process_ws_events,
                agent_state_machine,
                agent_transform_system,
                agent_despawn_system,
                file_highlight_system,
                process_spaceship_materials,
            )
                .chain(),
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
        ChromaticAberration {
            intensity: 0.008,
            max_samples: 6,
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

fn setup_ui(mut commands: Commands, _fs_state: Res<FileSystemState>) {
    // Root UI container in bottom left
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(20.0),
                bottom: Val::Px(20.0),
                width: Val::Px(320.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(20.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.92)),
            BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.3)),
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
                            BackgroundColor(Color::srgb(0.6, 0.45, 0.7)),
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

    // Prompt interface at the top center
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(20.0),
                left: Val::Percent(50.0),
                width: Val::Px(600.0),
                margin: UiRect::left(Val::Px(-300.0)), // Center by offsetting half width
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.95)),
            BorderColor::all(Color::srgba(0.6, 0.45, 0.9, 0.5)),
            PromptContainer,
        ))
        .with_children(|parent| {
            // Input field container
            parent.spawn((
                Button, // Make it clickable
                Node {
                    flex_grow: 1.0,
                    padding: UiRect::all(Val::Px(12.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.1, 0.05, 0.15, 0.8)),
                BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.4)),
                PromptInputField,
            ))
            .with_children(|field| {
                // Text will be added dynamically by update_prompt_display
                // Cursor will be added dynamically too
            });

            // Submit button
            parent.spawn((
                Button,
                Node {
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(12.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.6, 0.45, 0.9)),
                BorderColor::all(Color::srgba(0.8, 0.6, 1.0, 0.6)),
                PromptSubmitButton,
            ))
            .with_child((
                Text::new("Launch"),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });

    // Agent actions display at the top left
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(20.0),
                left: Val::Px(20.0),
                width: Val::Px(320.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(20.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.92)),
            BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.3)),
            AgentActionsContainer,
        ));

    // File stats display above camera mode (bottom left, above the camera controls)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(20.0),
                bottom: Val::Px(180.0), // Position above camera mode
                width: Val::Px(320.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(20.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.92)),
            BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.3)),
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
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.01, 0.08, 0.92)),
            BorderColor::all(Color::srgba(0.4, 0.3, 0.7, 0.3)),
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
                // Create a container for each legend item with mini planet + label
                parent.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                }).with_children(|item| {
                    // Mini planet (circular with border)
                    item.spawn((
                        Node {
                            width: Val::Px(14.0),
                            height: Val::Px(14.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(7.0)), // Make it circular
                            ..default()
                        },
                        BackgroundColor(color),
                        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.3)), // Subtle white border
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

    // Help button in bottom right corner (above color legend)
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            bottom: Val::Px(460.0), // Position above color legend
            width: Val::Px(50.0),
            height: Val::Px(50.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(25.0)), // Circular
            ..default()
        },
        BackgroundColor(Color::srgba(0.6, 0.45, 0.9, 0.9)),
        BorderColor::all(Color::srgba(0.8, 0.6, 1.0, 0.6)),
        HelpButton,
    ))
    .with_child((
        Text::new("?"),
        TextFont {
            font_size: 28.0,
            ..default()
        },
        TextColor(Color::WHITE),
    ));

    // Tips overlay (initially visible, will be managed by update_tips_overlay)
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)), // Semi-transparent backdrop
        TipsOverlay,
        GlobalZIndex(1000), // On top of everything
    ))
    .with_children(|parent| {
        // Tips panel
        parent.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(40.0)),
                border: UiRect::all(Val::Px(3.0)),
                border_radius: BorderRadius::all(Val::Px(20.0)),
                row_gap: Val::Px(20.0),
                max_width: Val::Px(600.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.02, 0.15)),
            BorderColor::all(Color::srgb(0.6, 0.45, 0.9)),
        ))
        .with_children(|panel| {
            // Title
            panel.spawn((
                Text::new("Welcome to Space Agents!"),
                TextFont {
                    font_size: 32.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 1.0)),
            ));

            // Tips
            let tips = [
                "Stars represent files & folders in your codebase",
                "Spaceships are AI agents working on your code",
                "Hover over a star to see recent activity",
                "Use the prompt bar at the top to give agents tasks",
                "Watch the Agent Activity panel to see what they're doing",
            ];

            for tip in tips {
                panel.spawn((
                    Text::new(tip),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }

            // Close button
            panel.spawn((
                Button,
                Node {
                    margin: UiRect::top(Val::Px(20.0)),
                    padding: UiRect::axes(Val::Px(30.0), Val::Px(15.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    align_self: AlignSelf::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.6, 0.45, 0.9)),
                BorderColor::all(Color::srgba(0.8, 0.6, 1.0, 0.6)),
                CloseOverlayButton,
            ))
            .with_child((
                Text::new("Got it!"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
    });
}

fn setup_vignette(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let vignette = create_vignette_image(256, 0.55, 0.6);
    let handle = images.add(vignette);

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        ImageNode::new(handle),
        GlobalZIndex(-1),
        Pickable::IGNORE,
    ));
}

fn setup_galaxy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut planet_materials: ResMut<Assets<PlanetMaterial>>,
    asset_server: Res<AssetServer>,
    mut fs_state: ResMut<FileSystemState>,
) {
    // Spawn initial galaxy stars from the already-built file system model
    for node_idx in 0..fs_state.model.total_nodes() {
        let entity = spawn_star(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut planet_materials,
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
    mut planet_materials: ResMut<Assets<PlanetMaterial>>,
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
                        &mut planet_materials,
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
                        &mut planet_materials,
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
                    *bg_color = BackgroundColor(Color::srgb(0.6, 0.45, 0.7));
                }
            }
            Interaction::Pressed | Interaction::None => {
                if controller.mode == button.mode {
                    *bg_color = BackgroundColor(Color::srgb(0.6, 0.45, 0.7));
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

                // Create a row container for each file entry
                parent.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    width: Val::Percent(100.0),
                    ..default()
                }).with_children(|row| {
                    // Filename (left-aligned, colored)
                    row.spawn((
                        Text::new(filename),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(color),
                    ));

                    // Edit count (right-aligned, white)
                    row.spawn((
                        Text::new(format!("{} edits", count)),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });
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

fn create_vignette_image(size: u32, inner_radius: f32, strength: f32) -> Image {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let denom = (size - 1).max(1) as f32;

    for y in 0..size {
        for x in 0..size {
            let nx = (x as f32 / denom) * 2.0 - 1.0;
            let ny = (y as f32 / denom) * 2.0 - 1.0;
            let dist = (nx * nx + ny * ny).sqrt();
            let edge = ((dist - inner_radius) / (1.0 - inner_radius)).clamp(0.0, 1.0);
            let alpha = (edge * edge * strength).clamp(0.0, 1.0);
            let a = (alpha * 255.0) as u8;
            data.extend_from_slice(&[0, 0, 0, a]);
        }
    }

    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
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

                // Use explanation if available, otherwise show tool name
                let label = if let Some(reason) = &event.reason {
                    format!("{} [{}]", reason, time_str)
                } else {
                    format!("{} [{}]", event.tool_name, time_str)
                };

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

fn handle_prompt_focus(
    mut prompt_state: ResMut<PromptInputState>,
    input_query: Query<&Interaction, (Changed<Interaction>, With<PromptInputField>)>,
) {
    for interaction in input_query.iter() {
        if *interaction == Interaction::Pressed {
            prompt_state.is_focused = true;
        }
    }
}

fn handle_prompt_unfocus(
    mut prompt_state: ResMut<PromptInputState>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    input_query: Query<&Interaction, With<PromptInputField>>,
) {
    // If user clicks and it's not on the input field, unfocus
    if mouse_button.just_pressed(MouseButton::Left) {
        if let Ok(interaction) = input_query.single() {
            // If interaction is None, the click was outside the input field
            if *interaction == Interaction::None && prompt_state.is_focused {
                prompt_state.is_focused = false;
            }
        }
    }
}

fn handle_prompt_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut prompt_state: ResMut<PromptInputState>,
) {
    // Only handle input when focused
    if !prompt_state.is_focused {
        return;
    }

    // Handle backspace
    if keyboard.just_pressed(KeyCode::Backspace) {
        prompt_state.text.pop();
    }

    // Handle character input - basic alphanumeric and common punctuation
    for key in keyboard.get_just_pressed() {
        let char_to_add = match key {
            KeyCode::Space => Some(' '),
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyB => Some('b'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyD => Some('d'),
            KeyCode::KeyE => Some('e'),
            KeyCode::KeyF => Some('f'),
            KeyCode::KeyG => Some('g'),
            KeyCode::KeyH => Some('h'),
            KeyCode::KeyI => Some('i'),
            KeyCode::KeyJ => Some('j'),
            KeyCode::KeyK => Some('k'),
            KeyCode::KeyL => Some('l'),
            KeyCode::KeyM => Some('m'),
            KeyCode::KeyN => Some('n'),
            KeyCode::KeyO => Some('o'),
            KeyCode::KeyP => Some('p'),
            KeyCode::KeyQ => Some('q'),
            KeyCode::KeyR => Some('r'),
            KeyCode::KeyS => Some('s'),
            KeyCode::KeyT => Some('t'),
            KeyCode::KeyU => Some('u'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyW => Some('w'),
            KeyCode::KeyX => Some('x'),
            KeyCode::KeyY => Some('y'),
            KeyCode::KeyZ => Some('z'),
            KeyCode::Digit0 => Some('0'),
            KeyCode::Digit1 => Some('1'),
            KeyCode::Digit2 => Some('2'),
            KeyCode::Digit3 => Some('3'),
            KeyCode::Digit4 => Some('4'),
            KeyCode::Digit5 => Some('5'),
            KeyCode::Digit6 => Some('6'),
            KeyCode::Digit7 => Some('7'),
            KeyCode::Digit8 => Some('8'),
            KeyCode::Digit9 => Some('9'),
            KeyCode::Period => Some('.'),
            KeyCode::Comma => Some(','),
            KeyCode::Minus => Some('-'),
            KeyCode::Slash => Some('/'),
            _ => None,
        };

        if let Some(c) = char_to_add {
            prompt_state.text.push(c);
        }
    }
}

fn handle_prompt_submit(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut prompt_state: ResMut<PromptInputState>,
    mut pending_task: ResMut<PendingAgentTask>,
    button_query: Query<&Interaction, (Changed<Interaction>, With<PromptSubmitButton>)>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<agent::AgentRegistry>,
    fs_state: Res<FileSystemState>,
) {
    let should_submit = (keyboard.just_pressed(KeyCode::Enter) && prompt_state.is_focused)
        || button_query.iter().any(|i| *i == Interaction::Pressed);

    if should_submit && !prompt_state.text.is_empty() {
        println!("🚀 Launching agent with task: {}", prompt_state.text);

        // Generate a unique session ID for the user-prompted agent
        let session_id = format!("user-agent-{}", registry.session_id_order.len());

        // Store task description to apply later
        pending_task.session_id = Some(session_id.clone());
        pending_task.task_description = prompt_state.text.clone();

        // Build initial action queue with some file visits
        let mut action_queue = std::collections::VecDeque::new();

        // Pick a few random files to visit
        if fs_state.model.total_nodes() > 0 {
            let num_files_to_visit = 5.min(fs_state.model.total_nodes());
            for i in 0..num_files_to_visit {
                let target_idx = (i * fs_state.model.total_nodes() / num_files_to_visit).min(fs_state.model.total_nodes() - 1);
                let position = galaxy::calculate_galaxy_position(&fs_state.model, target_idx);
                action_queue.push_back(agent::AgentAction::MoveTo {
                    position,
                    node_index: target_idx,
                });
            }
        }

        // Spawn the agent using the same system as WebSocket agents
        let greek_symbol = agent::GREEK_SYMBOLS[registry.session_id_order.len() % agent::GREEK_SYMBOLS.len()].to_string();

        let _entity = agent::spawn_agent_entity(
            &mut commands,
            &asset_server,
            &mut meshes,
            &mut materials,
            session_id.clone(),
            action_queue,
            greek_symbol,
        );

        registry.session_id_order.push(session_id.clone());
        registry.map.insert(session_id.clone(), _entity);

        // Clear the text and unfocus
        prompt_state.text.clear();
        prompt_state.is_focused = false;
    }
}

fn apply_pending_agent_tasks(
    mut pending_task: ResMut<PendingAgentTask>,
    registry: Res<agent::AgentRegistry>,
    mut agents: Query<&mut agent::Agent>,
) {
    if let Some(session_id) = &pending_task.session_id {
        if let Some(&entity) = registry.map.get(session_id) {
            if let Ok(mut agent) = agents.get_mut(entity) {
                agent.current_action = Some(format!("Working on: {}", pending_task.task_description));
                // Clear the pending task
                pending_task.session_id = None;
                pending_task.task_description.clear();
            }
        }
    }
}

fn update_prompt_display(
    mut commands: Commands,
    prompt_state: Res<PromptInputState>,
    input_query: Query<Entity, With<PromptInputField>>,
    children_query: Query<&Children>,
) {
    let Ok(input_entity) = input_query.single() else {
        return;
    };

    // Despawn existing children (text and cursor)
    if let Ok(children) = children_query.get(input_entity) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    // Update text display
    commands.entity(input_entity).with_children(|parent| {
        if !prompt_state.is_focused && prompt_state.text.is_empty() {
            // Show placeholder when not focused and empty
            parent.spawn((
                Text::new("Type a task for your AI agent..."),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgba(0.7, 0.7, 0.7, 0.6)),
            ));
        } else {
            // Show user input or empty with cursor when focused
            let display_text = if prompt_state.text.is_empty() {
                "".to_string()
            } else {
                prompt_state.text.clone()
            };

            parent.spawn((
                Text::new(&display_text),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // Add cursor when focused
            if prompt_state.is_focused {
                parent.spawn((
                    Text::new("|"),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    BlinkingCursor {
                        timer: 0.0,
                        visible: true,
                    },
                ));
            }
        }
    });
}

fn animate_cursor(
    time: Res<Time>,
    mut cursor_query: Query<(&mut BlinkingCursor, &mut TextColor)>,
) {
    for (mut cursor, mut color) in cursor_query.iter_mut() {
        cursor.timer += time.delta_secs();

        // Blink faster - every 0.4 seconds
        if cursor.timer >= 0.4 {
            cursor.timer = 0.0;
            cursor.visible = !cursor.visible;

            if cursor.visible {
                *color = TextColor(Color::WHITE);
            } else {
                *color = TextColor(Color::NONE);
            }
        }
    }
}

fn handle_help_button(
    mut tips_state: ResMut<TipsState>,
    button_query: Query<&Interaction, (Changed<Interaction>, With<HelpButton>)>,
) {
    for interaction in button_query.iter() {
        if *interaction == Interaction::Pressed {
            tips_state.visible = true;
        }
    }
}

fn handle_close_overlay(
    mut tips_state: ResMut<TipsState>,
    button_query: Query<&Interaction, (Changed<Interaction>, With<CloseOverlayButton>)>,
) {
    for interaction in button_query.iter() {
        if *interaction == Interaction::Pressed {
            tips_state.visible = false;
            tips_state.has_been_shown = true;
        }
    }
}

fn update_tips_overlay(
    tips_state: Res<TipsState>,
    mut overlay_query: Query<&mut Node, With<TipsOverlay>>,
) {
    if let Ok(mut node) = overlay_query.single_mut() {
        node.display = if tips_state.visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}
