use std::time::Duration;

use bevy::{
    ecs::query::QueryItem,
    math::Vec3A,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        render_asset::{RenderAsset, RenderAssetPlugin, RenderAssets},
        render_resource::{ShaderType, StorageBuffer},
        Render, RenderApp, RenderSet,
    },
};
use obvhs::{bvh2::builder::build_bvh2, Boundable, BvhBuildParams};
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
            ExtractComponentPlugin::<CameraExtract>::default(),
            ExtractComponentPlugin::<WindowExtract>::default(),
            // Extracting the Geometry from the main world
            ExtractComponentPlugin::<RaytracedSphereExtract>::default(),
            // Taking the handles along to populate the buffers
            ExtractComponentPlugin::<Handle<StandardMaterial>>::default(),
            // The settings will also be the data used in the shader.
            // This plugin will prepare the component for the GPU by creating a uniform buffer
            // and writing the data to that buffer every frame.
            UniformComponentPlugin::<RaytraceLevelExtract>::default(),
            UniformComponentPlugin::<CameraExtract>::default(),
            UniformComponentPlugin::<WindowExtract>::default(),
            // Transforming Assets
            RenderAssetPlugin::<RaytraceMaterial>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<ModelBuffer>()
            .init_resource::<MaterialBuffer>()
            .init_resource::<BVHBuffer>()
            .add_systems(Render, prepare_buffers.in_set(RenderSet::PrepareResources));
    }
}

// This solution is fine for now, but cameras can also render to other places that aren't bound by this height
// At that point the uniform position needs to be dynamic again and the extraction has to look different
#[derive(Component, Default, Clone, ShaderType)]
pub struct WindowExtract {
    random_seed: f32,
    height: u32,
}

impl ExtractComponent for WindowExtract {
    type QueryData = &'static Window;

    type QueryFilter = ();

    type Out = Self;

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        // TODO: This is probably a bad idea but other solutions needed mutable acces
        let mut rng = thread_rng();
        let random_seed: f32 = rng.gen_range(0.0..1.0);

        Some(WindowExtract {
            random_seed,
            height: item.physical_height(),
        })
    }
}

#[derive(Component, Default, Clone, ShaderType)]
pub struct CameraExtract {
    sample_count: u32,
    bounce_count: u32,
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
pub struct RaytraceLevelExtract {
    level: u32,
}

// Turning the marker into something the GPU can use
impl ExtractComponent for CameraExtract {
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

                CameraExtract {
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

// TODO: This becomes transform matrix, triangle_start, triangle_count and material
// triangle actually points at the index buffer
#[derive(ShaderType, Clone)]
pub struct Model {
    position: Vec3,
    radius: f32,
    material_id: u32,
}

impl Boundable for Model {
    fn aabb(&self) -> obvhs::aabb::Aabb {
        obvhs::aabb::Aabb::new(
            Vec3A::from(self.position) - Vec3A::splat(self.radius),
            Vec3A::from(self.position) + Vec3A::splat(self.radius),
        )
    }
}

#[derive(ShaderType, Debug)]
pub struct BVHNode {
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    // is the model_index if it is a leaf node (model_count > 0)
    // otherwise the first child index (second child directly after that)
    pub index: u32,
    pub model_count: u32,
}

/*
#[derive(ShaderType)]
pub struct ModelBVHNode {
    bounds_min: Vec3,
    bounds_max: Vec3,
    // is the triangle_index if it is a leaf node (triangle_count > 0)
    // otherwise the first child index (second child directly after that)
    index: u32,
    triangle_count: u32,
}
*/

// There is probably a better way to send all these buffers to the gpu
#[derive(Resource, Default, Deref)]
pub struct ModelBuffer(std::sync::Mutex<StorageBuffer<Vec<Model>>>);

#[derive(Resource, Default, Deref)]
pub struct MaterialBuffer(std::sync::Mutex<StorageBuffer<Vec<RaytraceMaterial>>>);

// The BVH and ModelBVH are different buffers because the idea behind them is,
// that the ModelBVHBuffer is in model local space and pretty much constant in its data
// while the BVH is for the world and rebuilt every time stuff moves
#[derive(Resource, Default, Deref)]
pub struct BVHBuffer(std::sync::Mutex<StorageBuffer<Vec<BVHNode>>>);

// Note: Bevy Builds Aabb's automatically | This probably needs to be inserted seperatly for my special meshes?
// Todo: look into stuff like this for dynamic bvh:
// https://gpuopen.com/download/publications/HPLOC.pdf
// https://dl.acm.org/doi/pdf/10.1145/3543867

/*
#[derive(Resource, Default, Deref)]
pub struct ModelBVHBuffer(std::sync::Mutex<StorageBuffer<Vec<ModelBVHNode>>>);

#[derive(Resource, Default, Deref)]
pub struct VertexBuffer(std::sync::Mutex<StorageBuffer<Vec<Vertex>>>);

#[derive(Resource, Default, Deref)]
pub struct IndexBuffer(std::sync::Mutex<StorageBuffer<Vec<u32>>>);
*/

pub fn prepare_buffers(
    model_buffer: Res<ModelBuffer>,
    material_buffer: Res<MaterialBuffer>,
    bvh_buffer: Res<BVHBuffer>,
    data: Query<(&RaytracedSphereExtract, &Handle<StandardMaterial>)>,
    materials: Res<RenderAssets<RaytraceMaterial>>,
) {
    let Ok(mut model_buffer) = model_buffer.lock() else {
        return;
    };

    let Ok(mut material_buffer) = material_buffer.lock() else {
        return;
    };

    let Ok(mut bvh_buffer) = bvh_buffer.lock() else {
        return;
    };

    let mut all_spheres = Vec::new();
    let mut all_materials = Vec::new();
    for (index, (sphere, material_handle)) in data.iter().enumerate() {
        let material = materials.get(material_handle).expect("This should exist");
        // TODO: Intergrate this with change detection so these buffers don't get replaced every frame
        all_materials.push(material.clone());

        all_spheres.push(Model {
            position: sphere.position,
            radius: sphere.radius,
            material_id: index as u32,
        });
    }

    let mut _bvh_build_time = Duration::new(0, 0);
    let config = BvhBuildParams::fast_build();
    let bvh = build_bvh2(&all_spheres, config, &mut _bvh_build_time);

    // reorder spheres according to indcies
    for (original_index, new_index) in bvh.primitive_indices.iter().enumerate() {
        all_spheres.swap(original_index, *new_index as usize);
    }

    let bvh_nodes = bvh
        .nodes
        .into_iter()
        .map(|node| BVHNode {
            bounds_max: node.aabb.min.into(),
            bounds_min: node.aabb.max.into(),
            index: node.first_index,
            model_count: node.prim_count,
        })
        .collect::<Vec<_>>();

    model_buffer.set(all_spheres);
    material_buffer.set(all_materials);
    bvh_buffer.set(bvh_nodes);
}
