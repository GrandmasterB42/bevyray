use bevy::{core_pipeline::prepass::DepthPrepass, prelude::*};
use raytracing::{RayTracePlugin, RayTracing};

mod raytracing;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, RayTracePlugin))
        .add_systems(Startup, setup)
        .run();
}

/// Set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // camera
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 5.0))
                .looking_at(Vec3::default(), Vec3::Y),
            camera: Camera {
                clear_color: Color::WHITE.into(),
                ..default()
            },
            ..default()
        },
        // Add the setting to the camera.
        // This component is also used to determine on which camera to run the post processing effect.
        RayTracing::Pure,
        DepthPrepass,
    ));

    // cube
    commands.spawn((PbrBundle {
        mesh: meshes.add(Cuboid::default()),
        material: materials.add(Color::srgb(0.8, 0.7, 0.6)),
        transform: Transform::from_xyz(0.0, 0.5, 0.0),
        ..default()
    },));
    /*
        // light
        commands.spawn(DirectionalLightBundle {
            directional_light: DirectionalLight {
                illuminance: 1_000.,
                ..default()
            },
            ..default()
        });
    */
}
