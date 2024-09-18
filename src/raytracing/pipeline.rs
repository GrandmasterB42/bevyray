use bevy::{
    core_pipeline::{
        fullscreen_vertex_shader::fullscreen_shader_vertex_state, prepass::ViewPrepassTextures,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent},
        render_asset::{RenderAsset, RenderAssets},
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            BindGroupEntries, BindGroupLayout, BindGroupLayoutEntries, BindingType,
            BufferBindingType, CachedRenderPipelineId, ColorTargetState, ColorWrites,
            FragmentState, MultisampleState, Operations, PipelineCache, PrimitiveState,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, StorageBuffer,
            TextureFormat, TextureSampleType,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::BevyDefault,
        view::ViewTarget,
    },
};

use super::RayTracing;

// Turning the marker into something the GPU can use
impl ExtractComponent for RayTracing {
    type QueryData = (
        &'static RayTracing,
        &'static GlobalTransform,
        &'static Projection,
    );

    type QueryFilter = With<RayTracing>;

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

// This is the component that will get passed to the shader
#[derive(Component, Default, Clone, Copy, ShaderType)]
pub struct RayTraceLevelExtract {
    level: u32,
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

#[derive(Component, Reflect)]
pub struct RaytracedSphere {
    pub radius: f32,
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

// The post process node used for the render graph
#[derive(Default)]
pub struct RayTracingNode;

// The ViewNode trait is required by the ViewNodeRunner
impl ViewNode for RayTracingNode {
    // The node needs a query to gather data from the ECS in order to do its rendering,
    // but it's not a normal system so we need to define it manually.
    //
    // This query will only run on the view entity
    type ViewQuery = (
        &'static ViewTarget,
        // The Prepass textures (depth used for blending between raster and raytraced)
        &'static ViewPrepassTextures,
        // This makes sure the node only runs on cameras with the PostProcessSettings component
        &'static RayTraceLevelExtract,
        // As there could be multiple post processing components sent to the GPU (one per camera),
        // we need to get the index of the one that is associated with the current view.
        &'static DynamicUniformIndex<RayTraceLevelExtract>,
        // The camera data
        &'static CameraExtract,
        &'static DynamicUniformIndex<CameraExtract>,
    );

    // Runs the node logic
    // This is where you encode draw commands.
    //
    // This will run on every view on which the graph is running.
    // If you don't want your effect to run on every camera,
    // you'll need to make sure you have a marker component as part of [`ViewQuery`]
    // to identify which camera(s) should run the effect.
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, prepass_textures, _raytrace_level, settings_index, _camera, camera_index): QueryItem<
            Self::ViewQuery,
        >,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // Get the pipeline resource that contains the global data we need
        // to create the render pipeline
        let raytrace_pipeline = world.resource::<RaytracingPipeline>();

        // The pipeline cache is a cache of all previously created pipelines.
        // It is required to avoid creating a new pipeline each frame,
        // which is expensive due to shader compilation.
        let pipeline_cache = world.resource::<PipelineCache>();

        // Get the pipeline from the cache
        let Some(pipeline) = pipeline_cache.get_render_pipeline(raytrace_pipeline.pipeline_id)
        else {
            return Ok(());
        };

        // Get the settings uniform binding
        let settings_uniforms = world.resource::<ComponentUniforms<RayTraceLevelExtract>>();
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };

        // Get the camera uniform binding
        let camera_uniforms = world.resource::<ComponentUniforms<CameraExtract>>();
        let Some(camera_binding) = camera_uniforms.uniforms().binding() else {
            return Ok(());
        };

        // This will start a new "post process write", obtaining two texture
        // views from the view target - a `source` and a `destination`.
        // `source` is the "current" main texture and you _must_ write into
        // `destination` because calling `post_process_write()` on the
        // [`ViewTarget`] will internally flip the [`ViewTarget`]'s main
        // texture to the `destination` texture. Failing to do so will cause
        // the current main texture information to be lost.
        let post_process = view_target.post_process_write();

        let Some(prepass) = prepass_textures.depth_view() else {
            return Ok(());
        };

        let geometry = world.resource::<GeometryBuffer>();
        let mut geometry_buffer = geometry
            .lock()
            .expect("Could not get geometry buffer out of mutex");

        let material = world.resource::<MaterialBuffer>();
        let mut material_buffer = material
            .lock()
            .expect("Could not get material buffer out of mutex");

        let render_device = render_context.render_device();
        {
            let render_queue = world.resource::<RenderQueue>();

            geometry_buffer.write_buffer(render_device, render_queue);
            material_buffer.write_buffer(render_device, render_queue);
        }

        let Some(geometry_buffer_binding) = geometry_buffer.binding() else {
            return Ok(());
        };

        let Some(material_buffer_binding) = material_buffer.binding() else {
            return Ok(());
        };

        // The bind_group gets created each frame.
        //
        // Normally, you would create a bind_group in the Queue set,
        // but this doesn't work with the post_process_write().
        // The reason it doesn't work is because each post_process_write will alternate the source/destination.
        // The only way to have the correct source/destination for the bind_group
        // is to make sure you get it during the node execution.
        let bind_group = render_device.create_bind_group(
            "raytrace_bind_group",
            &raytrace_pipeline.layout,
            // It's important for this to match the BindGroupLayout defined in the PostProcessPipeline
            &BindGroupEntries::sequential((
                // Make sure to use the source view
                post_process.source,
                // Use the sampler created for the pipeline
                &raytrace_pipeline.sampler,
                prepass,
                &raytrace_pipeline.depth_sampler,
                // Set the settings binding
                settings_binding.clone(),
                // Camera data
                camera_binding.clone(),
            )),
        );

        let buffer_bind_group = render_device.create_bind_group(
            "raytrace_geometry_bind_group",
            &raytrace_pipeline.buffer_layout,
            &BindGroupEntries::sequential((geometry_buffer_binding, material_buffer_binding)),
        );

        // Begin the render pass
        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("raytrace_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                // We need to specify the post process destination view here
                // to make sure we write to the appropriate texture.
                view: post_process.destination,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // This is mostly just wgpu boilerplate for drawing a fullscreen triangle,
        // using the pipeline/bind_group created above
        render_pass.set_render_pipeline(pipeline);
        // By passing in the index of the post process settings on this view, we ensure
        // that in the event that multiple settings were sent to the GPU (as would be the
        // case with multiple cameras), we use the correct one.
        render_pass.set_bind_group(
            0,
            &bind_group,
            &[settings_index.index(), camera_index.index()],
        );
        render_pass.set_bind_group(1, &buffer_bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

// This contains global data used by the render pipeline. This will be created once on startup.
#[derive(Resource)]
pub struct RaytracingPipeline {
    layout: BindGroupLayout,
    buffer_layout: BindGroupLayout,
    sampler: Sampler,
    depth_sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for RaytracingPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        // We need to define the bind group layout used for our pipeline
        let layout = render_device.create_bind_group_layout(
            "raytrace_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                // The layout entries will only be visible in the fragment stage
                ShaderStages::FRAGMENT,
                (
                    // The screen texture
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    // The sampler that will be used to sample the screen texture
                    sampler(SamplerBindingType::Filtering),
                    // The depth texture
                    texture_2d(TextureSampleType::Depth),
                    // The sampler that will be used to sample the depth texture
                    sampler(SamplerBindingType::NonFiltering),
                    // The Level uniform that will control the blending
                    uniform_buffer::<RayTraceLevelExtract>(true),
                    // The camera uniform
                    uniform_buffer::<CameraExtract>(true),
                ),
            ),
        );

        let buffer_layout = render_device.create_bind_group_layout(
            "raytrace_geometry_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                // The layout entries will only be visible in the fragment stage
                ShaderStages::FRAGMENT,
                (
                    // the geometry buffer
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    // The material buffer
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ),
        );

        // We can create the sampler here since it won't change at runtime and doesn't depend on the view
        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let depth_sampler = render_device.create_sampler(&SamplerDescriptor::default());

        // Get the shader handle
        let shader = world.load_asset("shaders/raytrace.wgsl");

        let pipeline_id = world
            .resource_mut::<PipelineCache>()
            // This will add the pipeline to the cache and queue it's creation
            .queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("raytrace_pipeline".into()),
                layout: vec![layout.clone(), buffer_layout.clone()],
                // This will setup a fullscreen triangle for the vertex state
                vertex: fullscreen_shader_vertex_state(),
                fragment: Some(FragmentState {
                    shader,
                    shader_defs: vec![],
                    // Make sure this matches the entry point of your shader.
                    // It can be anything as long as it matches here and in the shader.
                    entry_point: "fragment".into(),
                    targets: vec![Some(ColorTargetState {
                        format: TextureFormat::bevy_default(),
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
            });

        Self {
            layout,
            buffer_layout,
            sampler,
            depth_sampler,
            pipeline_id,
        }
    }
}
