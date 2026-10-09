//! Reuse Bevy's per-frame mesh bind groups while nothing they point at has changed.
//!
//! Bevy 0.18's `prepare_mesh_bind_groups` rebuilds every mesh bind group each frame. With
//! lightmaps that is one group per lightmap slab (four lightmapped entities each) per render
//! phase: Angel City's 419 world batches make ~105 slabs, so ~750 bind groups were created and
//! dropped every frame (4-8 ms of the render thread). The groups only reference the per-phase
//! instance buffers, the skin (and morph) buffers and the lightmap slabs, so this keeps the
//! last two sets (skin buffers alternate every frame) and puts one back instead of letting Bevy
//! rebuild it when those are the same. Any lightmap allocation or removal, or morph targets,
//! fall back to Bevy's rebuild. `TF_NO_BIND_CACHE=1` turns it off.

use bevy::pbr::{prepare_mesh_bind_groups, Lightmap, MeshBindGroups, MeshInputUniform, MeshUniform, MorphUniforms, SkinUniforms};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::render::batching::gpu_preprocessing::BatchedInstanceBuffers;
use bevy::render::render_resource::BufferId;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

pub struct MeshBindCachePlugin;

impl Plugin for MeshBindCachePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        if std::env::var_os("TF_NO_BIND_CACHE").is_some() {
            return;
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        // Bevy's system again, behind a run condition (a system's own set can't take one).
        if let Err(e) = render_app.remove_systems_in_set(Render, prepare_mesh_bind_groups, bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly) {
            log::warn!("mesh bind group cache off: {e:?}");
            return;
        }
        render_app
            .init_resource::<BindCache>()
            .add_systems(ExtractSchedule, watch_lightmaps)
            .add_systems(
                Render,
                (restore, prepare_mesh_bind_groups.run_if(|c: Res<BindCache>| !c.reused)).chain().in_set(RenderSystems::PrepareBindGroups),
            );
    }
}

/// What a set of mesh bind groups was built from.
#[derive(PartialEq, Eq, Clone)]
struct Key {
    skin: (BufferId, BufferId),
    /// Each phase's instance data buffer (sorted).
    phases: Vec<(std::any::TypeId, BufferId)>,
}

#[derive(Resource, Default)]
struct BindCache {
    /// The key of the `MeshBindGroups` resource now in the render world.
    current: Option<Key>,
    /// The set before it.
    spare: Option<(Key, MeshBindGroups)>,
    /// Set when this frame reuses a cached set, so Bevy's rebuild is skipped.
    reused: bool,
    /// Lightmapped entities that have been visible (Bevy gives each a lightmap slot then).
    lit: HashSet<Entity>,
    /// Frames to keep rebuilding after a lightmap slab changed.
    dirty: u32,
    /// Frames reused and rebuilt since the last debug log.
    stats: (u32, u32),
}

/// Mirror Bevy's `extract_lightmaps`: a lightmapped entity gets a slab slot the first time it is
/// visible, and loses it when the `Lightmap` goes. Either changes a slab's bind group.
fn watch_lightmaps(
    mut cache: ResMut<BindCache>,
    changed: Extract<Query<(Entity, &ViewVisibility), (With<Lightmap>, Or<(Changed<ViewVisibility>, Changed<Lightmap>)>)>>,
    mut removed: Extract<RemovedComponents<Lightmap>>,
) {
    let cache = cache.as_mut();
    let mut touched = false;
    for (e, vis) in &changed {
        if vis.get() && cache.lit.insert(e) {
            touched = true;
        }
    }
    for e in removed.read() {
        touched |= cache.lit.remove(&e);
    }
    if touched {
        // Bevy fills a slot once its image is on the GPU (normally the same frame).
        cache.dirty = 3;
    }
}

fn restore(world: &mut World) {
    let key = {
        let skins = world.resource::<SkinUniforms>();
        let morphs = world.resource::<MorphUniforms>();
        let instances = world.get_resource::<BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>();
        match instances {
            Some(instances) if morphs.current_buffer.buffer().is_none() => {
                let mut phases: Vec<_> = instances
                    .phase_instance_buffers
                    .iter()
                    .filter_map(|(t, b)| Some((*t, b.data_buffer.buffer()?.id())))
                    .collect();
                phases.sort_unstable_by_key(|(t, _)| *t);
                Some(Key { skin: (skins.current_buffer.id(), skins.prev_buffer.id()), phases })
            }
            _ => None,
        }
    };
    let mut cache = world.resource_mut::<BindCache>();
    if cache.reused {
        cache.stats.0 += 1;
    } else {
        cache.stats.1 += 1;
    }
    if cache.stats.0 + cache.stats.1 >= 600 {
        log::debug!("mesh bind groups: {} frames reused, {} rebuilt", cache.stats.0, cache.stats.1);
        cache.stats = (0, 0);
    }
    cache.reused = false;
    if cache.dirty > 0 || key.is_none() {
        cache.dirty = cache.dirty.saturating_sub(1);
        cache.current = None;
        cache.spare = None;
        return;
    }
    let key = key.unwrap();
    if cache.current.as_ref() == Some(&key) {
        cache.reused = true;
        return;
    }
    let previous = cache.current.take();
    let spare = cache.spare.take_if(|(k, _)| *k == key);
    let reused = spare.is_some();
    cache.reused = reused;
    // Bevy rebuilds this frame's set unless the spare fits; either way it is built for `key`.
    cache.current = Some(key);
    // The set now in place (built for `previous`) becomes the spare.
    let outgoing = world.remove_resource::<MeshBindGroups>();
    if let Some((_, groups)) = spare {
        world.insert_resource(groups);
    }
    world.resource_mut::<BindCache>().spare = previous.zip(outgoing);
}
