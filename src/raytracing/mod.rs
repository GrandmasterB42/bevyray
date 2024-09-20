#![allow(clippy::type_complexity)]

use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        prepass::DepthPrepass,
    },
    prelude::*,
    render::{
        render_graph::{RenderGraphApp, RenderLabel, ViewNodeRunner},
        RenderApp,
    },
};

mod extract;
mod pipeline;

use extract::RaytraceExtractPlugin;
use pipeline::{RayTracingNode, RaytracingPipeline};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct RaytraceLabel;

pub struct RaytracePlugin;

impl Plugin for RaytracePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RaytraceExtractPlugin)
            // TODO: Investigate how to make this Msaa compatible
            .insert_resource(Msaa::Off)
            .register_type::<RaytracedCamera>()
            .register_type::<Raytracing>()
            .register_type::<RaytracedSphere>()
            .add_systems(Update, auto_add_camera_components);

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
            .add_render_graph_node::<ViewNodeRunner<RayTracingNode>>(
                // Specify the label of the graph, in this case we want the graph for 3d
                Core3d, // It also needs the label of the node
                RaytraceLabel,
            )
            .add_render_graph_edges(
                Core3d,
                // Specify the node ordering.
                // This will automatically create all required node edges to enforce the given ordering.
                // NOTE: Should this be done before tonemapping?
                (
                    Node3d::Tonemapping,
                    RaytraceLabel,
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

#[derive(Component, Reflect, Clone, Copy)]
pub struct RaytracedCamera {
    pub level: Raytracing,
    pub sample_count: u32,
    pub bounces: u32,
}

// This is a marker component that specifies the raytracing level for a camera
#[repr(u32)]
#[derive(Reflect, Clone, Copy)]
pub enum Raytracing {
    Skip,
    FallbackRaster,
    FallbackRaytraced,
    Pure,
}

#[derive(Component, Reflect)]
pub struct RaytracedSphere {
    pub radius: f32,
}

fn auto_add_camera_components(
    added: Query<Entity, (With<Camera>, With<Projection>, Without<DepthPrepass>)>,
    mut cmd: Commands,
) {
    for camera in added.iter() {
        cmd.entity(camera).insert(DepthPrepass);
    }
}
