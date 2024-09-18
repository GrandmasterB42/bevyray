use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        render_asset::{RenderAsset, RenderAssetPlugin, RenderAssets},
        render_resource::{ShaderType, StorageBuffer},
        Render, RenderApp, RenderSet,
    },
};

use super::{RaytracedSphere, Raytracing};

pub struct RaytraceExtractPlugin;

impl Plugin for RaytraceExtractPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // The settings will be a component that lives in the main world but will
            // be extracted to the render world every frame.
            // This makes it possible to control the effect from the main world.
            // This plugin will take care of extracting it automatically.
            // It's important to derive [`ExtractComponent`] on [`PostProcessingSettings`]
            // for this plugin to work correctly.
            ExtractComponentPlugin::<Raytracing>::default(),
            // Extracting the Geometry from the main world
            ExtractComponentPlugin::<RaytracedSphereExtract>::default(),
            // The settings will also be the data used in the shader.
            // This plugin will prepare the component for the GPU by creating a uniform buffer
            // and writing the data to that buffer every frame.
            UniformComponentPlugin::<RayTraceLevelExtract>::default(),
            UniformComponentPlugin::<CameraExtract>::default(),
            // Transforming Assets
            RenderAssetPlugin::<RaytraceMaterial>::default(),
            // Taking the handles along to populate the buffers
            ExtractComponentPlugin::<Handle<StandardMaterial>>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<GeometryBuffer>()
            .init_resource::<MaterialBuffer>()
            .add_systems(Render, prepare_buffers.in_set(RenderSet::PrepareResources));
    }
}

#[derive(Component, Default, Clone, Copy, ShaderType)]
pub struct CameraExtract {
    // 0 -> perspective; rest not supported
    projection: u32,
    near: f32,
    far: f32,
    fov: f32,
    // width / height
    aspect: f32,
    position: Vec3,
    direction: Vec3,
    up: Vec3,
}

// This is the component that will get passed to the shader
#[derive(Component, Default, Clone, Copy, ShaderType)]
pub struct RayTraceLevelExtract {
    level: u32,
}

// Turning the marker into something the GPU can use
impl ExtractComponent for Raytracing {
    type QueryData = (
        &'static Raytracing,
        &'static GlobalTransform,
        &'static Projection,
    );

    type QueryFilter = With<Raytracing>;

    type Out = (RayTraceLevelExtract, CameraExtract);

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        let camera = match *item.2 {
            Projection::Perspective(PerspectiveProjection {
                fov,
                aspect_ratio,
                near,
                far,
            }) => {
                let transform = item.1;

                // Maybe just pass the Matrix at some point
                let position = transform.translation();
                let direction = transform.forward().as_vec3();
                let up = transform.up().as_vec3();

                CameraExtract {
                    projection: 0,
                    near,
                    far,
                    aspect: aspect_ratio,
                    fov,
                    position,
                    direction,
                    up,
                }
            }
            // Currently unsupported
            Projection::Orthographic(OrthographicProjection { .. }) => return None,
        };

        let level = RayTraceLevelExtract {
            level: *item.0 as u32,
        };
        Some((level, camera))
    }
}

#[derive(Clone, Component)]
pub struct RaytracedSphereExtract {
    position: Vec3,
    radius: f32,
}

impl ExtractComponent for RaytracedSphereExtract {
    type QueryData = (&'static RaytracedSphere, &'static GlobalTransform);

    type QueryFilter = ();

    type Out = Self;

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        Some(RaytracedSphereExtract {
            position: item.1.translation(),
            radius: item.0.radius,
        })
    }
}

#[derive(Clone, Component, ShaderType)]
pub struct RaytraceMaterial {
    base_color: Vec3,
}

impl RenderAsset for RaytraceMaterial {
    type SourceAsset = StandardMaterial;

    type Param = ();

    fn prepare_asset(
        source_asset: Self::SourceAsset,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Result<Self, bevy::render::render_asset::PrepareAssetError<Self::SourceAsset>> {
        let base_color = source_asset.base_color.to_linear().to_vec3();
        Ok(RaytraceMaterial { base_color })
    }
}

#[derive(ShaderType)]
pub struct RayTraceSphere {
    position: Vec3,
    radius: f32,
    material_id: u32,
}

// This seems dumb | There is probably a better way to send data to the gpu (maybe also only the stuff that changed)
#[derive(Resource, Default, Deref)]
pub struct GeometryBuffer(std::sync::Mutex<StorageBuffer<Vec<RayTraceSphere>>>);

#[derive(Resource, Default, Deref)]
pub struct MaterialBuffer(std::sync::Mutex<StorageBuffer<Vec<RaytraceMaterial>>>);

pub fn prepare_buffers(
    geometry_buffer: ResMut<GeometryBuffer>,
    material_buffer: ResMut<MaterialBuffer>,
    data: Query<(&RaytracedSphereExtract, &Handle<StandardMaterial>)>,
    materials: Res<RenderAssets<RaytraceMaterial>>,
) {
    let Ok(mut geometry_buffer) = geometry_buffer.lock() else {
        return;
    };

    let Ok(mut material_buffer) = material_buffer.lock() else {
        return;
    };

    let mut all_spheres = Vec::new();
    let mut all_materials = Vec::new();
    for (index, (sphere, material_handle)) in data.iter().enumerate() {
        let material = materials.get(material_handle).expect("This should exist");
        // TODO: Intergrate this with change detection so these buffers don't get replaced every frame
        all_materials.push(material.clone());

        all_spheres.push(RayTraceSphere {
            position: sphere.position,
            radius: sphere.radius,
            material_id: index as u32,
        });
    }

    geometry_buffer.set(all_spheres);
    material_buffer.set(all_materials);
}
