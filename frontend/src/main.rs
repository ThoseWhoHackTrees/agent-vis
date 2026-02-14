use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest())) // for crisp pixel art
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    // 2D camera for "Map" view
    commands.spawn(Camera2d);

    // test planet
    commands.spawn(Sprite {
        color: Color::srgb(0.2, 0.7, 0.9),
        custom_size: Some(Vec2::new(100.0, 100.0)),
        ..default()
    });
}
