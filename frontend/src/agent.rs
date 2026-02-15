// hello world
use bevy::prelude::*;
use crossbeam_channel::Receiver;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use crate::galaxy::{calculate_galaxy_position, FileStar};
use crate::ws_client::AgentEvent;
use crate::FileSystemState;

// --- Components ---

#[derive(Debug, Clone)]
pub enum AgentAction {
    MoveTo { position: Vec3, node_index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Spawning { timer: f32 },
    Idle { timer: f32 },
    Moving { from: Vec3, to: Vec3, progress: f32, target_node: usize },
    Despawning { timer: f32 },
}

#[derive(Component)]
pub struct Agent {
    pub session_id: String,
    pub event_queue: VecDeque<AgentAction>,
    pub state: AgentState,
    pub current_target_file: Option<usize>,
    pub current_action: Option<String>, // Description of what the agent is doing
    pub color: Color, // Unique color for this agent (used for UI and spaceship)
}

// --- Resources ---

#[derive(Resource, Default)]
pub struct AgentRegistry {
    pub map: HashMap<String, Entity>,
}

#[derive(Resource)]
pub struct WsClientState {
    pub receiver: Receiver<AgentEvent>,
}

// --- File event history ---

#[derive(Debug, Clone)]
pub struct FileEvent {
    pub tool_name: String,
    pub session_id: String,
    pub timestamp: Option<String>,
}

#[derive(Resource, Default)]
pub struct FileEventHistory {
    pub map: HashMap<usize, Vec<FileEvent>>, // node_index -> events (max 10)
}

#[derive(Resource, Default)]
pub struct HoveredFile(pub Option<usize>);

// --- Messages ---

#[derive(Message)]
pub struct AgentArrivedEvent {
    pub node_index: usize,
}

// --- Highlight component ---

#[derive(Component)]
pub struct FileHighlight {
    pub intensity: f32,
}

// --- Marker for newly spawned spaceships that need material processing ---

#[derive(Component)]
pub struct UnprocessedSpaceship;

// --- Constants ---

const SPAWN_DURATION: f32 = 0.5;
const DESPAWN_DURATION: f32 = 0.5;
const IDLE_TIMEOUT: f32 = 5.0;
const MOVE_SPEED: f32 = 1.2; // seconds per move
const AGENT_SCALE: f32 = 100.0;

// Ease-in-out cubic
fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0_f32).powi(3) / 2.0
    }
}

// Generate a consistent color for an agent based on their session_id
pub fn generate_agent_color(session_id: &str) -> Color {
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

// Helper function to spawn an agent with spaceship model
fn spawn_agent_entity(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    session_id: String,
    event_queue: VecDeque<AgentAction>,
) -> Entity {
    // Load the spaceship GLB scene
    let spaceship_scene = asset_server.load("spaceships.glb#Scene0");

    // Generate consistent color for this agent
    let agent_color = generate_agent_color(&session_id);

    // Create parent entity with Agent component
    let agent_entity = commands
        .spawn((
            Agent {
                session_id,
                event_queue,
                state: AgentState::Spawning { timer: 0.0 },
                current_target_file: None,
                current_action: None,
                color: agent_color,
            },
            Transform::from_translation(Vec3::new(0.0, 15.0, 0.0))
                .with_scale(Vec3::ZERO)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)), // Rotate to face forward
            UnprocessedSpaceship, // Mark for material processing
        ))
        .with_children(|parent| {
            // Spawn the GLB scene as a child
            parent.spawn(SceneRoot(spaceship_scene));

            // Add a bright point light to make the spaceship more visible
            parent.spawn((
                PointLight {
                    color: Color::srgb(0.9, 0.95, 1.0), // Cool white/blue light
                    intensity: 5000000.0,
                    range: 50.0,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
        })
        .id();

    agent_entity
}

// --- System 1: Process WebSocket events ---

pub fn process_ws_events(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    ws_state: Res<WsClientState>,
    fs_state: Res<FileSystemState>,
    mut registry: ResMut<AgentRegistry>,
    mut agents: Query<&mut Agent>,
    mut event_history: ResMut<FileEventHistory>,
) {
    while let Ok(event) = ws_state.receiver.try_recv() {
        match event {
            AgentEvent::SessionStart { session_id, .. } => {
                if registry.map.contains_key(&session_id) {
                    // Agent already exists, cancel despawn if needed
                    if let Some(&entity) = registry.map.get(&session_id) {
                        if let Ok(mut agent) = agents.get_mut(entity) {
                            if matches!(agent.state, AgentState::Despawning { .. }) {
                                agent.state = AgentState::Idle { timer: 0.0 };
                            }
                        }
                    }
                    continue;
                }

                println!("[agent] Spawning agent for session {}", session_id);

                let entity = spawn_agent_entity(
                    &mut commands,
                    &asset_server,
                    session_id.clone(),
                    VecDeque::new(),
                );

                registry.map.insert(session_id, entity);
            }
            AgentEvent::ToolUse {
                session_id,
                file_path,
                tool_name,
                timestamp,
            } => {
                // Resolve file path to galaxy position
                let canonical = PathBuf::from(&file_path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&file_path));

                // Extract filename for display
                let filename = canonical
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file_path);

                // Create action description
                let action_desc = format!("{} {}",
                    match tool_name.as_str() {
                        "Read" => "Reading",
                        "Write" => "Writing",
                        "Edit" => "Editing",
                        "Grep" => "Searching",
                        "Glob" => "Finding",
                        _ => "Working on",
                    },
                    filename
                );

                let resolved = fs_state
                    .model
                    .get_node_by_path(&canonical)
                    .map(|(idx, _)| (idx, calculate_galaxy_position(&fs_state.model, idx)));

                if let Some((node_idx, position)) = resolved {
                    // Record event in history
                    let events = event_history.map.entry(node_idx).or_default();
                    events.push(FileEvent {
                        tool_name: tool_name.clone(),
                        session_id: session_id.clone(),
                        timestamp: timestamp.clone(),
                    });
                    if events.len() > 10 {
                        events.remove(0);
                    }

                    // Get or create agent
                    let entity = if let Some(&entity) = registry.map.get(&session_id) {
                        // Cancel despawn if needed
                        if let Ok(mut agent) = agents.get_mut(entity) {
                            if matches!(agent.state, AgentState::Despawning { .. }) {
                                agent.state = AgentState::Idle { timer: 0.0 };
                            }
                            agent.event_queue.push_back(AgentAction::MoveTo {
                                position,
                                node_index: node_idx,
                            });
                            agent.current_action = Some(action_desc.clone());
                        }
                        Some(entity)
                    } else {
                        // Auto-spawn agent on first tool_use if no session_start was seen
                        println!(
                            "[agent] Auto-spawning agent for session {} (tool_use)",
                            session_id
                        );

                        let mut queue = VecDeque::new();
                        queue.push_back(AgentAction::MoveTo {
                            position,
                            node_index: node_idx,
                        });

                        let entity = spawn_agent_entity(
                            &mut commands,
                            &asset_server,
                            session_id.clone(),
                            queue,
                        );

                        registry.map.insert(session_id.clone(), entity);
                        Some(entity)
                    };

                    // Set current action for already-spawned agents
                    if let Some(entity) = entity {
                        if let Ok(mut agent) = agents.get_mut(entity) {
                            agent.current_action = Some(action_desc);
                        }
                    }
                } else {
                    println!(
                        "[agent] File not in galaxy, skipping: {}",
                        file_path
                    );
                }
            }
        }
    }
}

// --- System 2: Agent state machine ---

pub fn agent_state_machine(
    time: Res<Time>,
    mut agents: Query<(&mut Agent, &Transform)>,
    mut arrived_events: MessageWriter<AgentArrivedEvent>,
) {
    let dt = time.delta_secs();

    for (mut agent, transform) in agents.iter_mut() {
        match agent.state.clone() {
            AgentState::Spawning { timer } => {
                let new_timer = timer + dt;
                if new_timer >= SPAWN_DURATION {
                    // Done spawning, transition to idle
                    agent.state = AgentState::Idle { timer: 0.0 };
                } else {
                    agent.state = AgentState::Spawning { timer: new_timer };
                }
            }
            AgentState::Idle { timer } => {
                // Try to pop next action
                if let Some(action) = agent.event_queue.pop_front() {
                    match action {
                        AgentAction::MoveTo {
                            position,
                            node_index,
                        } => {
                            agent.current_target_file = Some(node_index);
                            agent.state = AgentState::Moving {
                                from: transform.translation,
                                to: position,
                                progress: 0.0,
                                target_node: node_index,
                            };
                        }
                    }
                } else {
                    // No actions, increment idle timer
                    let new_timer = timer + dt;
                    if new_timer >= IDLE_TIMEOUT {
                        agent.state = AgentState::Despawning { timer: 0.0 };
                        agent.current_action = None; // Clear action when starting to despawn
                    } else {
                        agent.state = AgentState::Idle { timer: new_timer };
                    }
                }
            }
            AgentState::Moving {
                from: _,
                to: _,
                progress,
                target_node,
            } => {
                let new_progress = progress + dt / MOVE_SPEED;
                if new_progress >= 1.0 {
                    // Arrived
                    agent.current_target_file = Some(target_node);
                    arrived_events.write(AgentArrivedEvent {
                        node_index: target_node,
                    });
                    agent.state = AgentState::Idle { timer: 0.0 };
                } else {
                    agent.state = AgentState::Moving {
                        from: agent.state.moving_from().unwrap(),
                        to: agent.state.moving_to().unwrap(),
                        progress: new_progress,
                        target_node,
                    };
                }
            }
            AgentState::Despawning { timer } => {
                let new_timer = timer + dt;
                if new_timer >= DESPAWN_DURATION {
                    // Will be cleaned up by despawn system
                    agent.state = AgentState::Despawning {
                        timer: DESPAWN_DURATION,
                    };
                } else {
                    agent.state = AgentState::Despawning { timer: new_timer };
                }
            }
        }
    }
}

impl AgentState {
    fn moving_from(&self) -> Option<Vec3> {
        if let AgentState::Moving { from, .. } = self {
            Some(*from)
        } else {
            None
        }
    }

    fn moving_to(&self) -> Option<Vec3> {
        if let AgentState::Moving { to, .. } = self {
            Some(*to)
        } else {
            None
        }
    }
}

// --- System 3: Agent transform (position + scale interpolation) ---

pub fn agent_transform_system(mut agents: Query<(&Agent, &mut Transform)>) {
    for (agent, mut transform) in agents.iter_mut() {
        match &agent.state {
            AgentState::Spawning { timer } => {
                let t = (*timer / SPAWN_DURATION).clamp(0.0, 1.0);
                let eased = ease_in_out_cubic(t);
                transform.scale = Vec3::splat(eased * AGENT_SCALE);
            }
            AgentState::Idle { .. } => {
                transform.scale = Vec3::splat(AGENT_SCALE);
            }
            AgentState::Moving {
                from,
                to,
                progress,
                ..
            } => {
                let t = ease_in_out_cubic(*progress);
                transform.translation = from.lerp(*to, t);
                transform.scale = Vec3::splat(AGENT_SCALE);

                // Make spaceship face movement direction
                let direction = (*to - *from).normalize();
                if direction.length_squared() > 0.001 {
                    // Calculate rotation to face direction (assuming spaceship faces +Z by default)
                    let target_rotation = Quat::from_rotation_arc(Vec3::Z, direction);
                    transform.rotation = target_rotation;
                }
            }
            AgentState::Despawning { timer } => {
                let t = (*timer / DESPAWN_DURATION).clamp(0.0, 1.0);
                let eased = ease_in_out_cubic(t);
                transform.scale = Vec3::splat((1.0 - eased) * AGENT_SCALE);
            }
        }
    }
}

// --- System 4: Agent despawn ---

pub fn agent_despawn_system(
    mut commands: Commands,
    agents: Query<(Entity, &Agent)>,
    mut registry: ResMut<AgentRegistry>,
) {
    for (entity, agent) in agents.iter() {
        if let AgentState::Despawning { timer } = &agent.state {
            if *timer >= DESPAWN_DURATION {
                println!("[agent] Despawning agent for session {}", agent.session_id);
                registry.map.remove(&agent.session_id);
                commands.entity(entity).despawn();
            }
        }
    }
}

// --- System 5: File highlight ---

pub fn file_highlight_system(
    time: Res<Time>,
    mut arrived_events: MessageReader<AgentArrivedEvent>,
    mut commands: Commands,
    fs_state: Res<FileSystemState>,
    _stars: Query<(Entity, &FileStar, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut highlights: Query<(Entity, &mut FileHighlight, &MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();

    // Boost stars on arrival
    for event in arrived_events.read() {
        if let Some(&star_entity) = fs_state.entity_map.get(&event.node_index) {
            // Add or refresh highlight
            if let Ok((_entity, mut highlight, _mat)) = highlights.get_mut(star_entity) {
                highlight.intensity = 6.0;
            } else {
                commands.entity(star_entity).insert(FileHighlight { intensity: 6.0 });
            }
        }
    }

    // Decay highlights
    for (entity, mut highlight, mat_handle) in highlights.iter_mut() {
        highlight.intensity -= dt * 1.5;
        if highlight.intensity <= 0.0 {
            // Remove highlight and restore original material
            commands.entity(entity).remove::<FileHighlight>();
        } else {
            // Boost emissive on the material
            if let Some(material) = materials.get_mut(mat_handle) {
                let base = material.base_color;
                let base_linear = LinearRgba::from(base);
                material.emissive = base_linear * (2.0 + highlight.intensity);
            }
        }
    }
}

// --- System 6: Process spaceship materials ---

pub fn process_spaceship_materials(
    mut commands: Commands,
    unprocessed: Query<(Entity, &Children, &Agent), With<UnprocessedSpaceship>>,
    children_query: Query<&Children>,
    mut mesh_query: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, children, agent) in unprocessed.iter() {
        // Get the agent's unique color
        let agent_color = LinearRgba::from(agent.color);

        // Recursively traverse all descendants
        let mut stack: Vec<Entity> = children.to_vec();
        let mut processed_any = false;

        while let Some(child) = stack.pop() {
            // Check if this child has a material
            if let Ok(mut mat_handle) = mesh_query.get_mut(child) {
                if let Some(original_material) = materials.get(&mat_handle.0) {
                    // Clone the material to create a unique instance for this agent
                    let mut new_material = original_material.clone();

                    // Make the spaceship unlit so it's not affected by scene lighting
                    new_material.unlit = true;

                    // Set the base color to the agent's color
                    new_material.base_color = agent.color;

                    // Set emissive to make it glow, but reduce bloom on very bright parts (antennae)
                    // If the material already had high emissive (antennae), reduce it to 3x
                    // Otherwise use 5x for the body to make it bright
                    let current_emissive_intensity =
                        new_material.emissive.red.max(new_material.emissive.green).max(new_material.emissive.blue);

                    let emissive_multiplier = if current_emissive_intensity > 5.0 {
                        // This is likely an antenna or other glowing part - tone it down
                        2.0
                    } else {
                        // Regular body - make it bright
                        8.0
                    };

                    new_material.emissive = agent_color * emissive_multiplier;

                    // Add the new material to assets and update the entity to use it
                    let new_handle = materials.add(new_material);
                    mat_handle.0 = new_handle;

                    processed_any = true;
                }
            }

            // Add this child's children to the stack
            if let Ok(grandchildren) = children_query.get(child) {
                stack.extend(grandchildren.to_vec());
            }
        }

        // Remove the marker component once we've processed materials
        if processed_any {
            commands.entity(entity).remove::<UnprocessedSpaceship>();
        }
    }
}

// --- Picking observers for file star hover ---

pub fn on_file_star_over(
    event: On<Pointer<Over>>,
    stars: Query<&FileStar>,
    mut hovered: ResMut<HoveredFile>,
) {
    if let Ok(star) = stars.get(event.entity) {
        hovered.0 = Some(star.node_index);
    }
}

pub fn on_file_star_out(
    event: On<Pointer<Out>>,
    stars: Query<&FileStar>,
    mut hovered: ResMut<HoveredFile>,
) {
    if let Ok(star) = stars.get(event.entity) {
        // Only clear if we're still hovering this specific star
        if hovered.0 == Some(star.node_index) {
            hovered.0 = None;
        }
    }
}
