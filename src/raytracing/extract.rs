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
use rand::{thread_rng, Rng};

use super::{RaytracedCamera, RaytracedSphere};

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
            ExtractComponentPlugin::<RaytracedCamera>::default(),
            // Extracting the Geometry from the main world
            ExtractComponentPlugin::<RaytracedSphereExtract>::default(),
            // The settings will also be the data used in the shader.
            // This plugin will prepare the component for the GPU by creating a uniform buffer
            // and writing the data to that buffer every frame.
            UniformComponentPlugin::<RaytraceLevelExtract>::default(),
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
    random_seed: f32,
    sample_count: u32,
    bounce_count: u32,
    // 0 -> perspective; rest not supported
    projection: u32,
    near: f32,
    far: f32,
    fov: f32,
    // width / height
    aspect: f32,
    height: u32,
    position: Vec3,
    direction: Vec3,
    up: Vec3,
}

// This is the component that will get passed to the shader
#[derive(Component, Default, Clone, Copy, ShaderType)]
pub struct RaytraceLevelExtract {
    level: u32,
}

// Turning the marker into something the GPU can use
impl ExtractComponent for RaytracedCamera {
    type QueryData = (
        &'static RaytracedCamera,
        &'static GlobalTransform,
        &'static Projection,
    );

    type QueryFilter = ();

    type Out = (RaytraceLevelExtract, CameraExtract);

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        let camera = item.0;
        let camera_extract = match *item.2 {
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

                // TODO: This is probably a bad idea but other solutions needed mutable acces
                let mut rng = thread_rng();
                let random_seed: f32 = rng.gen_range(0.0..1.0);

                CameraExtract {
                    random_seed,
                    sample_count: camera.sample_count,
                    bounce_count: camera.bounces,
                    projection: 0,
                    near,
                    far,
                    aspect: aspect_ratio,
                    fov,
                    position,
                    direction,
                    up,
                    height: camera.height,
                }
            }
            // Currently unsupported
            Projection::Orthographic(OrthographicProjection { .. }) => return None,
        };

        let level = RaytraceLevelExtract {
            level: camera.level as u32,
        };

        Some((level, camera_extract))
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
    metallic: f32,
    roughness: f32,
    reflectance: f32,
    ior: f32,
    specular_transmission: f32,
}

impl RenderAsset for RaytraceMaterial {
    type SourceAsset = StandardMaterial;

    type Param = ();

    fn prepare_asset(
        source_asset: Self::SourceAsset,
        _param: &mut bevy::ecs::system::SystemParamItem<Self::Param>,
    ) -> Result<Self, bevy::render::render_asset::PrepareAssetError<Self::SourceAsset>> {
        Ok(RaytraceMaterial {
            base_color: source_asset.base_color.to_linear().to_vec3(),
            metallic: source_asset.metallic,
            roughness: source_asset.perceptual_roughness,
            reflectance: source_asset.reflectance,
            ior: source_asset.ior,
            specular_transmission: source_asset.specular_transmission,
        })
    }
}

#[derive(ShaderType)]
pub struct RaytraceSphere {
    position: Vec3,
    radius: f32,
    material_id: u32,
}

// This seems dumb | There is probably a better way to send data to the gpu (maybe also only the stuff that changed)
#[derive(Resource, Default, Deref)]
pub struct GeometryBuffer(std::sync::Mutex<StorageBuffer<Vec<RaytraceSphere>>>);

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

        all_spheres.push(RaytraceSphere {
            position: sphere.position,
            radius: sphere.radius,
            material_id: index as u32,
        });
    }

    geometry_buffer.set(all_spheres);
    material_buffer.set(all_materials);
}
