#![allow(clippy::type_complexity)]

use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        prepass::DepthPrepass,
    },
    prelude::*,
    render::{
        extract_component::{ExtractComponentPlugin, UniformComponentPlugin},
        render_asset::RenderAssetPlugin,
        render_graph::{RenderGraphApp, RenderLabel, ViewNodeRunner},
        Render, RenderApp, RenderSet,
    },
};

use pipeline::{
    prepare_buffers, CameraExtract, GeometryBuffer, MaterialBuffer, RayTraceLevelExtract,
    RayTracingNode, RaytraceMaterial, RaytracedSphereExtract, RaytracingPipeline,
};

mod pipeline;

pub use pipeline::RaytracedSphere;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct RayTraceLabel;

pub struct RayTracePlugin;

impl Plugin for RayTracePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // The settings will be a component that lives in the main world but will
            // be extracted to the render world every frame.
            // This makes it possible to control the effect from the main world.
            // This plugin will take care of extracting it automatically.
            // It's important to derive [`ExtractComponent`] on [`PostProcessingSettings`]
            // for this plugin to work correctly.
            ExtractComponentPlugin::<RayTracing>::default(),
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
        ))
        // TODO: Investigate how to make this Msaa compatible
        .insert_resource(Msaa::Off)
        .register_type::<RayTracing>()
        .register_type::<RaytracedSphere>()
        .add_systems(Update, auto_add_depth_prepass);

        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            // Bevy's renderer uses a render graph which is a collection of nodes in a directed acyclic graph.
            // It currently runs on each view/camera and executes each node in the specified order.
            // It will make sure that any node that needs a dependency from another node
            // only runs when that dependency is done.
            //
            // Each node can execute arbitrary work, but it generally runs at least one render pass.
            // A node only has access to the render world, so if you need data from the main world
            // you need to extract it manually or with the plugin like above.
            // Add a [`Node`] to the [`RenderGraph`]
            // The Node needs to impl FromWorld
            //
            // The [`ViewNodeRunner`] is a special [`Node`] that will automatically run the node for each view
            // matching the [`ViewQuery`]
            // Buffers used to send data to the GPU
            .init_resource::<GeometryBuffer>()
            .init_resource::<MaterialBuffer>()
            .add_systems(Render, prepare_buffers.in_set(RenderSet::PrepareResources))
            .add_render_graph_node::<ViewNodeRunner<RayTracingNode>>(
                // Specify the label of the graph, in this case we want the graph for 3d
                Core3d, // It also needs the label of the node
                RayTraceLabel,
            )
            .add_render_graph_edges(
                Core3d,
                // Specify the node ordering.
                // This will automatically create all required node edges to enforce the given ordering.
                (
                    Node3d::Tonemapping,
                    RayTraceLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            // Initialize the pipeline
            .init_resource::<RaytracingPipeline>();
    }
}

// This is a marker component that specifies the raytracing level for a camera
#[repr(u32)]
#[derive(Component, Reflect, Clone, Copy)]
pub enum RayTracing {
    Skip,
    FallbackRaster,
    FallbackRaytraced,
    Pure,
}

fn auto_add_depth_prepass(
    added: Query<Entity, (With<Camera>, With<Projection>, Without<DepthPrepass>)>,
    mut cmd: Commands,
) {
    for camera in added.iter() {
        cmd.entity(camera).insert(DepthPrepass);
    }
}
