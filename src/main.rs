use bevy::prelude::*;
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use bevy_mod_picking::DefaultPickingPlugins;
use bevy_transform_gizmo::TransformGizmoPlugin;
use raytracing::{RaytracePlugin, RaytracedSphere, Raytracing};

mod raytracing;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            RaytracePlugin,
            WorldInspectorPlugin::new(),
            DefaultPickingPlugins,
            TransformGizmoPlugin::default(),
            NoCameraPlayerPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, sync_picking_radius)
        .add_systems(Last, remove_transform_gizmo_clear)
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
        Name::new("Raytraced Camera"),
        Raytracing::FallbackRaytraced,
        bevy_transform_gizmo::GizmoPickSource::default(),
        FlyCam,
    ));

    // cube
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Cuboid::default()),
            material: materials.add(Color::srgb(0.8, 0.7, 0.6)),
            transform: Transform::from_xyz(0.0, 0.5, 0.0),
            ..default()
        },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));

    // Sphere
    commands.spawn((
        RaytracedSphere { radius: 1.1 },
        Name::from("Raytraced Sphere"),
        // Components for making it pickable
        PbrBundle {
            mesh: meshes.add(Sphere::new(1.0)),
            transform: Transform::from_xyz(0.0, 0.5, 0.0),
            material: materials.add(Color::srgb(0.9, 0.1, 0.1)),
            ..default()
        },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));

    // light
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 1_000.,
            ..default()
        },
        ..default()
    });
}

// The gizmo camera copies the main camera, but the clear color messes up the modified render pipeline
fn remove_transform_gizmo_clear(
    mut gizmo_cam: Query<
        &mut Camera,
        (
            With<bevy_transform_gizmo::InternalGizmoCamera>,
            Without<bevy_transform_gizmo::GizmoPickSource>,
        ),
    >,
) {
    let Ok(mut gizmo_cam) = gizmo_cam.get_single_mut() else {
        return;
    };

    gizmo_cam.clear_color = ClearColorConfig::None;
}

// Replace the sphere used for picking to have the same size | This should be a non-issue with meshes as their Globaltransform should be loaded into the shader
fn sync_picking_radius(
    mut sync_items: Query<(&RaytracedSphere, &mut Transform), Changed<RaytracedSphere>>,
) {
    for (sphere, mut transform) in sync_items.iter_mut() {
        transform.scale = Vec3::splat(sphere.radius - 0.1);
    }
}
