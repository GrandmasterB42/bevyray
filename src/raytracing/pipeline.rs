use bevy::{
    core_pipeline::{
        fullscreen_vertex_shader::fullscreen_shader_vertex_state, prepass::ViewPrepassTextures,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent},
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            BindGroupEntries, BindGroupLayout, BindGroupLayoutEntries, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, FragmentState, MultisampleState, Operations,
            PipelineCache, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, TextureFormat, TextureSampleType,
        },
        renderer::{RenderContext, RenderDevice},
        texture::BevyDefault,
        view::ViewTarget,
    },
};

use super::RayTracing;

// Turning the marker into something the GPU can use
impl ExtractComponent for RayTracing {
    type QueryData = (&'static RayTracing, &'static Projection);

    type QueryFilter = ();

    type Out = (RayTraceLevelExtract, CameraExtract);

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        let camera = match *item.1 {
            Projection::Perspective(PerspectiveProjection { near, far, .. }) => CameraExtract {
                projection: 0,
                near,
                far,
            },
            Projection::Orthographic(OrthographicProjection { near, far, .. }) => CameraExtract {
                projection: 0,
                near,
                far,
            },
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
    // 0 -> perspective; 1 -> orthographic
    projection: u32,
    near: f32,
    far: f32,
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

        // The bind_group gets created each frame.
        //
        // Normally, you would create a bind_group in the Queue set,
        // but this doesn't work with the post_process_write().
        // The reason it doesn't work is because each post_process_write will alternate the source/destination.
        // The only way to have the correct source/destination for the bind_group
        // is to make sure you get it during the node execution.
        let bind_group = render_context.render_device().create_bind_group(
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
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

// This contains global data used by the render pipeline. This will be created once on startup.
#[derive(Resource)]
pub struct RaytracingPipeline {
    layout: BindGroupLayout,
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
                layout: vec![layout.clone()],
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
                // All of the following properties are not important for this effect so just use the default values.
                // This struct doesn't have the Default trait implemented because not all field can have a default value.
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
            });

        Self {
            layout,
            sampler,
            depth_sampler,
            pipeline_id,
        }
    }
}
