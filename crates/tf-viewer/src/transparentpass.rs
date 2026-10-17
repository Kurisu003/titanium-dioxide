//! The 3D transparent pass encoded on a worker thread, like the opaque pass.
//!
//! Bevy 0.18's `MainTransparentPass3dNode` records into the render graph's own command encoder,
//! so its draws (every sorted translucent item: the BSP's translucent surfaces, decals,
//! particles) were recorded, and with wgpu 27's deferred encoding validated and encoded by
//! `CommandEncoder::finish`, on the render thread while it walks the graph. This node is the
//! same pass in a command buffer generation task (as Bevy's opaque, prepass, shadow and bloom
//! nodes already are): the attachments are taken when the node runs, in graph order, and the
//! command buffers are submitted in the order they were queued, so the GPU gets the same
//! commands in the same order. `TF_SERIAL_TRANSPARENT=1` keeps Bevy's node.

use bevy::camera::{MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_graph::{
    NodeRunError, RenderGraph, RenderGraphContext, ViewNode, ViewNodeRunner,
};
use bevy::render::render_phase::{TrackedRenderPass, ViewSortedRenderPhases};
use bevy::render::render_resource::{CommandEncoderDescriptor, RenderPassDescriptor, StoreOp};
use bevy::render::renderer::RenderContext;
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::render::RenderApp;

pub struct ParallelTransparentPlugin;

impl Plugin for ParallelTransparentPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        if std::env::var_os("TF_SERIAL_TRANSPARENT").is_some() {
            return;
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let runner = ViewNodeRunner::new(ParallelTransparentPass3dNode, render_app.world_mut());
        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        let Some(core3d) = graph.get_sub_graph_mut(Core3d) else {
            return;
        };
        match core3d.get_node_state_mut(Node3d::MainTransparentPass) {
            Ok(state) => {
                state.node = Box::new(runner);
                state.type_name =
                    std::any::type_name::<ViewNodeRunner<ParallelTransparentPass3dNode>>();
            }
            Err(e) => log::warn!("transparent pass stays serial: {e:?}"),
        }
    }
}

/// Bevy's `MainTransparentPass3dNode` (without its WebGL2 viewport reset), recorded in a task.
#[derive(Default)]
pub struct ParallelTransparentPass3dNode;

impl ViewNode for ParallelTransparentPass3dNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, view, target, depth, resolution_override): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph.view_entity();
        let Some(transparent_phases) =
            world.get_resource::<ViewSortedRenderPhases<Transparent3d>>()
        else {
            return Ok(());
        };
        let Some(transparent_phase) = transparent_phases.get(&view.retained_view_entity) else {
            return Ok(());
        };
        if transparent_phase.items.is_empty() {
            return Ok(());
        }
        let diagnostics = render_context.diagnostic_recorder();
        // Taken now, not in the task: which main texture is current and whether an attachment
        // still has to be cleared depend on the nodes that ran before this one.
        let color_attachments = [Some(target.get_color_attachment())];
        // NOTE (Bevy): depth is stored, not just loaded, as a workaround for bevy#3776.
        let depth_stencil_attachment = Some(depth.get_attachment(StoreOp::Store));
        let viewport =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override);

        render_context.add_command_buffer_generation_task(move |render_device| {
            #[cfg(feature = "profile")]
            let _span = bevy::log::info_span!("main_transparent_pass_3d").entered();
            let mut command_encoder =
                render_device.create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("main_transparent_pass_3d_command_encoder"),
                });
            let render_pass = command_encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_transparent_pass_3d"),
                color_attachments: &color_attachments,
                depth_stencil_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut render_pass = TrackedRenderPass::new(&render_device, render_pass);
            let pass_span = diagnostics.pass_span(&mut render_pass, "main_transparent_pass_3d");
            if let Some(viewport) = viewport {
                render_pass.set_camera_viewport(&viewport);
            }
            if let Err(err) = transparent_phase.render(&mut render_pass, world, view_entity) {
                error!("Error encountered while rendering the transparent phase {err:?}");
            }
            pass_span.end(&mut render_pass);
            drop(render_pass);
            command_encoder.finish()
        });
        Ok(())
    }
}
