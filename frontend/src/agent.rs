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

// --- Constants ---

const SPAWN_DURATION: f32 = 0.5;
const DESPAWN_DURATION: f32 = 0.5;
const IDLE_TIMEOUT: f32 = 5.0;
const MOVE_SPEED: f32 = 1.2; // seconds per move
const AGENT_SCALE: f32 = 0.6;

// Ease-in-out cubic
fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0_f32).powi(3) / 2.0
    }
}

// --- System 1: Process WebSocket events ---

pub fn process_ws_events(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ws_state: Res<WsClientState>,
    fs_state: Res<FileSystemState>,
    mut registry: ResMut<AgentRegistry>,
    mut agents: Query<&mut Agent>,
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

                let mesh = meshes.add(Sphere::new(AGENT_SCALE));
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.1, 0.1),
                    emissive: LinearRgba::new(8.0, 0.5, 0.5, 1.0),
                    ..default()
                });

                let entity = commands
                    .spawn((
                        Agent {
                            session_id: session_id.clone(),
                            event_queue: VecDeque::new(),
                            state: AgentState::Spawning { timer: 0.0 },
                            current_target_file: None,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(Vec3::ZERO)
                            .with_scale(Vec3::ZERO),
                    ))
                    .id();

                registry.map.insert(session_id, entity);
            }
            AgentEvent::ToolUse {
                session_id,
                file_path,
                ..
            } => {
                // Resolve file path to galaxy position
                let canonical = PathBuf::from(&file_path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&file_path));

                let resolved = fs_state
                    .model
                    .get_node_by_path(&canonical)
                    .map(|(idx, _)| (idx, calculate_galaxy_position(&fs_state.model, idx)));

                if let Some((node_idx, position)) = resolved {
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
                        }
                        Some(entity)
                    } else {
                        // Auto-spawn agent on first tool_use if no session_start was seen
                        println!(
                            "[agent] Auto-spawning agent for session {} (tool_use)",
                            session_id
                        );
                        let mesh = meshes.add(Sphere::new(AGENT_SCALE));
                        let material = materials.add(StandardMaterial {
                            base_color: Color::srgb(1.0, 0.1, 0.1),
                            emissive: LinearRgba::new(8.0, 0.5, 0.5, 1.0),
                            ..default()
                        });

                        let mut queue = VecDeque::new();
                        queue.push_back(AgentAction::MoveTo {
                            position,
                            node_index: node_idx,
                        });

                        let entity = commands
                            .spawn((
                                Agent {
                                    session_id: session_id.clone(),
                                    event_queue: queue,
                                    state: AgentState::Spawning { timer: 0.0 },
                                    current_target_file: None,
                                },
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                Transform::from_translation(Vec3::ZERO)
                                    .with_scale(Vec3::ZERO),
                            ))
                            .id();

                        registry.map.insert(session_id, entity);
                        Some(entity)
                    };

                    let _ = entity; // suppress unused warning
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
