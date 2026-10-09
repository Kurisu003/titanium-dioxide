//! Shadow passes batched by pipeline instead of by material where the material can't matter.
//!
//! Bevy 0.18's `queue_shadows` keys every shadow batch set by the material's bind group, so each
//! material of the map became its own multi-draw (200-540 per cascade on Angel City, each with
//! its pipeline and bind group switches and wgpu's validation and tracking of them, 2-2.5 ms of
//! CPU per cascade). An opaque material's shadow pipeline has no fragment shader at all and the
//! default prepass vertex shader never reads the material bind group: the group is bound only
//! because the pipeline layout has a slot for it, and any group of that material type fits it.
//! For those pipelines this queues the meshes under one batch set per pipeline (and vertex/index
//! slab), so they are drawn by one multi-draw; the representative's material group is bound.
//! The depth written is the same: same meshes, same instances and pipelines, and a depth-only
//! pass with depth writes and no blending gives the same result in any draw order.
//! Pipelines that run a fragment shader (alpha masks, blended or emulated unclipped depth) or a
//! material's own prepass vertex shader keep one batch set per material, as before.
//! `TF_SHADOW_BY_MATERIAL=1` turns it off.

use bevy::pbr::{
    queue_shadows, ExtractedDirectionalLight, ExtractedPointLight, LightEntity, PreparedMaterial, PrepassPipeline, RenderCascadesVisibleEntities, RenderCubemapVisibleEntities, RenderMaterialInstances,
    RenderMeshInstanceFlags, RenderMeshInstances, RenderVisibleMeshEntities, Shadow, ShadowBatchSetKey, ShadowBinKey, ShadowsDrawFunction, SpecializedShadowMaterialPipelineCache, ViewLightEntities,
};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::render_phase::{BinnedRenderPhaseType, ViewBinnedRenderPhases};
use bevy::render::render_resource::{CachedRenderPipelineId, PipelineCache};
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderSystems};

pub struct ShadowBatchPlugin;

impl Plugin for ShadowBatchPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(Render, log_batch_sets.in_set(RenderSystems::Render).run_if(|| log::log_enabled!(log::Level::Debug)));
        if std::env::var_os("TF_SHADOW_BY_MATERIAL").is_some() {
            return;
        }
        if let Err(e) = render_app.remove_systems_in_set(Render, queue_shadows, bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly) {
            log::warn!("shadow batching by pipeline off: {e:?}");
            return;
        }
        render_app.init_resource::<MaterialFree>().add_systems(Render, queue_shadows_by_pipeline.in_set(RenderSystems::QueueMeshes));
    }
}

/// Debug log: the batch sets of every shadow phase, every 300 frames.
fn log_batch_sets(phases: Res<ViewBinnedRenderPhases<Shadow>>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame % 300 != 0 {
        return;
    }
    let mut sets: Vec<_> = phases.values().map(|p| p.multidrawable_meshes.len() + p.batchable_meshes.len() + p.unbatchable_meshes.len()).collect();
    sets.sort_unstable();
    log::debug!("shadow batch sets per phase: {sets:?}");
}

/// Per shadow pipeline: whether drawing with it reads nothing from the material bind group.
#[derive(Resource, Default)]
struct MaterialFree(HashMap<CachedRenderPipelineId, bool>);

impl MaterialFree {
    /// None while the pipeline is still in the cache's queue (it can't draw this frame anyway).
    fn get(&mut self, id: CachedRenderPipelineId, cache: &PipelineCache, prepass: &PrepassPipeline) -> Option<bool> {
        if let Some(&free) = self.0.get(&id) {
            return Some(free);
        }
        let bevy::render::render_resource::PipelineDescriptor::RenderPipelineDescriptor(d) = &cache.pipelines().nth(id.id())?.descriptor else {
            return Some(false);
        };
        let free = d.fragment.is_none() && d.vertex.shader == prepass.default_prepass_shader;
        self.0.insert(id, free);
        Some(free)
    }
}

/// Bevy's `queue_shadows`, with the material bind group left out of the batch set key for
/// material-free pipelines (see the module docs).
#[allow(clippy::too_many_arguments)]
fn queue_shadows_by_pipeline(
    render_mesh_instances: Res<RenderMeshInstances>,
    render_materials: Res<ErasedRenderAssets<PreparedMaterial>>,
    render_material_instances: Res<RenderMaterialInstances>,
    mut shadow_render_phases: ResMut<ViewBinnedRenderPhases<Shadow>>,
    gpu_preprocessing_support: Res<GpuPreprocessingSupport>,
    mesh_allocator: Res<MeshAllocator>,
    view_lights: Query<(Entity, &ViewLightEntities, Option<&bevy::camera::visibility::RenderLayers>), With<ExtractedView>>,
    view_light_entities: Query<(&LightEntity, &ExtractedView)>,
    point_light_entities: Query<&RenderCubemapVisibleEntities, With<ExtractedPointLight>>,
    directional_light_entities: Query<&RenderCascadesVisibleEntities, With<ExtractedDirectionalLight>>,
    spot_light_entities: Query<&RenderVisibleMeshEntities, With<ExtractedPointLight>>,
    specialized_material_pipeline_cache: Res<SpecializedShadowMaterialPipelineCache>,
    pipeline_cache: Res<PipelineCache>,
    prepass_pipeline: Res<PrepassPipeline>,
    mut material_free: ResMut<MaterialFree>,
) {
    for (entity, view_lights, camera_layers) in &view_lights {
        for view_light_entity in view_lights.lights.iter().copied() {
            let Ok((light_entity, extracted_view_light)) = view_light_entities.get(view_light_entity) else {
                continue;
            };
            let Some(shadow_phase) = shadow_render_phases.get_mut(&extracted_view_light.retained_view_entity) else {
                continue;
            };
            let Some(view_specialized_material_pipeline_cache) = specialized_material_pipeline_cache.get(&extracted_view_light.retained_view_entity) else {
                continue;
            };
            let visible_entities = match light_entity {
                LightEntity::Directional { light_entity, cascade_index } => directional_light_entities
                    .get(*light_entity)
                    .expect("Failed to get directional light visible entities")
                    .entities
                    .get(&entity)
                    .expect("Failed to get directional light visible entities for view")
                    .get(*cascade_index)
                    .expect("Failed to get directional light visible entities for cascade"),
                LightEntity::Point { light_entity, face_index } => point_light_entities.get(*light_entity).expect("Failed to get point light visible entities").get(*face_index),
                LightEntity::Spot { light_entity } => spot_light_entities.get(*light_entity).expect("Failed to get spot light visible entities"),
            };

            for (entity, main_entity) in visible_entities.iter().copied() {
                let Some((current_change_tick, pipeline_id)) = view_specialized_material_pipeline_cache.get(&main_entity) else {
                    continue;
                };
                let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(main_entity) else {
                    continue;
                };
                if !mesh_instance.flags.contains(RenderMeshInstanceFlags::SHADOW_CASTER) {
                    continue;
                }
                let mesh_layers = mesh_instance.shared.render_layers.as_ref().unwrap_or_default();
                let camera_layers = camera_layers.unwrap_or_default();
                if !camera_layers.intersects(mesh_layers) {
                    continue;
                }
                // Skip the entity if it's cached in a bin and up to date.
                if shadow_phase.validate_cached_entity(main_entity, *current_change_tick) {
                    continue;
                }
                let Some(material_instance) = render_material_instances.instances.get(&main_entity) else {
                    continue;
                };
                let Some(material) = render_materials.get(material_instance.asset_id) else {
                    continue;
                };
                let Some(draw_function) = material.properties.get_draw_function(ShadowsDrawFunction) else {
                    continue;
                };
                let (vertex_slab, index_slab) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id);
                // A pipeline queued this frame isn't ready to draw until a later frame; the entity
                // is queued then.
                let Some(free) = material_free.get(*pipeline_id, &pipeline_cache, &prepass_pipeline) else {
                    continue;
                };
                let material_bind_group_index = if free { None } else { Some(material.binding.group.0) };
                let batch_set_key = ShadowBatchSetKey { pipeline: *pipeline_id, draw_function, material_bind_group_index, vertex_slab: vertex_slab.unwrap_or_default(), index_slab };
                shadow_phase.add(
                    batch_set_key,
                    ShadowBinKey { asset_id: mesh_instance.mesh_asset_id.into() },
                    (entity, main_entity),
                    mesh_instance.current_uniform_index,
                    BinnedRenderPhaseType::mesh(mesh_instance.should_batch(), &gpu_preprocessing_support),
                    *current_change_tick,
                );
            }
        }
    }
}
