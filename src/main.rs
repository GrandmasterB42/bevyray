use bevy::prelude::*;
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use bevy_mod_picking::{
    backends::raycast::{bevy_mod_raycast::prelude::RaycastVisibility, RaycastBackendSettings},
    DefaultPickingPlugins,
};
use bevy_transform_gizmo::TransformGizmoPlugin;
use rand::random;
use raytracing::{RaytracePlugin, RaytracedCamera, RaytracedSphere, Raytracing};

mod raytracing;

// NOTE: Depth blending still doesnt work properly

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
        .add_systems(Startup, (setup, modify_raycast_backend))
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
        RaytracedCamera {
            level: Raytracing::FallbackRaytraced,
            sample_count: 4,
            bounces: 4,
        },
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

    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        metallic: 0.0,
        ..default()
    });
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(1000.0)),
            material: ground_material,
            transform: Transform::from_xyz(0.0, -1000.0, 0.0),
            visibility: Visibility::Hidden,
            ..default()
        },
        RaytracedSphere { radius: 1000.0 },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));

    for a in -11..=11 {
        for b in -11..11 {
            let choose_mat = random::<f32>();
            let center_v = Vec3::new(
                a as f32 + 0.9 * random::<f32>(),
                0.2,
                b as f32 + 0.9 * random::<f32>(),
            );
            let center = Transform::from_xyz(center_v.x, center_v.y, center_v.z);

            if (center_v - Vec3::new(4.0, 0.2, 0.0)).length() > 0.9 {
                if choose_mat < 0.8 {
                    // diffuse
                    let albedo = Vec3::new(random(), random(), random())
                        * Vec3::new(random(), random(), random());
                    let sphere_material = materials.add(StandardMaterial {
                        base_color: Color::srgb_from_array(albedo.to_array()),
                        metallic: 0.0,
                        ..default()
                    });
                    commands.spawn((
                        PbrBundle {
                            mesh: meshes.add(Sphere::new(0.2)),
                            material: sphere_material,
                            transform: center,
                            visibility: Visibility::Hidden,
                            ..default()
                        },
                        RaytracedSphere { radius: 0.2 },
                        bevy_mod_picking::PickableBundle::default(),
                        bevy_transform_gizmo::GizmoTransformable,
                    ));
                } else if choose_mat < 0.95 {
                    // metal
                    let albedo = Vec3::new(random(), random(), random());
                    let roughness = random();
                    let sphere_material = materials.add(StandardMaterial {
                        base_color: Color::srgb_from_array(albedo.to_array()),
                        metallic: 1.0,
                        perceptual_roughness: roughness,
                        ..default()
                    });
                    commands.spawn((
                        PbrBundle {
                            mesh: meshes.add(Sphere::new(0.2)),
                            material: sphere_material,
                            transform: center,
                            visibility: Visibility::Hidden,
                            ..default()
                        },
                        RaytracedSphere { radius: 0.2 },
                        bevy_mod_picking::PickableBundle::default(),
                        bevy_transform_gizmo::GizmoTransformable,
                    ));
                } else {
                    // glass
                    let sphere_material = materials.add(StandardMaterial {
                        metallic: 0.0,
                        ior: 1.5,
                        specular_transmission: 1.0,
                        ..default()
                    });
                    commands.spawn((
                        PbrBundle {
                            mesh: meshes.add(Sphere::new(0.2)),
                            material: sphere_material,
                            transform: center,
                            visibility: Visibility::Hidden,
                            ..default()
                        },
                        RaytracedSphere { radius: 0.2 },
                        bevy_mod_picking::PickableBundle::default(),
                        bevy_transform_gizmo::GizmoTransformable,
                    ));
                }
            }
        }
    }

    // big spheres
    let sphere_material = materials.add(StandardMaterial {
        metallic: 0.0,
        ior: 1.5,
        specular_transmission: 1.0,
        ..default()
    });
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(1.0)),
            material: sphere_material,
            transform: Transform::from_xyz(0.0, 1.0, 0.0),
            visibility: Visibility::Hidden,
            ..default()
        },
        RaytracedSphere { radius: 1.0 },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));

    let sphere_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.2, 0.1),
        metallic: 0.0,
        ..default()
    });
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(1.0)),
            material: sphere_material,
            transform: Transform::from_xyz(-4.0, 1.0, 0.0),
            visibility: Visibility::Hidden,
            ..default()
        },
        RaytracedSphere { radius: 1.0 },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));

    let sphere_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.6, 0.5),
        metallic: 1.0,
        perceptual_roughness: 0.0,
        ..default()
    });
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Sphere::new(1.0)),
            material: sphere_material,
            transform: Transform::from_xyz(4.0, 1.0, 0.0),
            visibility: Visibility::Hidden,
            ..default()
        },
        RaytracedSphere { radius: 1.0 },
        bevy_mod_picking::PickableBundle::default(),
        bevy_transform_gizmo::GizmoTransformable,
    ));
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

// Make raycast picking ignore standart visibility
fn modify_raycast_backend(mut settings: ResMut<RaycastBackendSettings>) {
    settings.raycast_visibility = RaycastVisibility::Ignore;
}

// Replace the sphere used for picking to have the same size | This should be a non-issue with meshes as their Globaltransform should be loaded into the shader
fn sync_picking_radius(
    mut sync_items: Query<(&RaytracedSphere, &mut Transform), Changed<RaytracedSphere>>,
) {
    for (sphere, mut transform) in sync_items.iter_mut() {
        transform.scale = Vec3::splat(sphere.radius);
    }
}
