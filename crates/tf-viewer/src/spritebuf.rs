//! Particle sprite vertices without a mesh asset change every frame.
//!
//! The sprite layers (pfx.rs, particles.rs) used to rebuild a `Mesh` asset per layer each frame.
//! Any modified mesh makes Bevy re-check every mesh entity of every material type for
//! respecialisation (`AssetChanged<Mesh3d>`: a hash lookup per entity, ~0.5 ms per material type
//! on Angel City) and re-uploads the mesh through the mesh allocator. Instead each layer owns a
//! storage buffer in its material, written in place once a frame from the render world, and
//! draws one of a few constant meshes of 2^k quads (`pull.wgsl` reads the vertices from the
//! buffer by the mesh's local vertex index; quads past the layer's count are clipped away). The
//! vertices, their order and the shading are the ones the mesh had.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Buffer, BufferDescriptor, BufferUsages};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};

/// Floats per vertex: position and u, normal and v, colour.
pub const STRIDE: usize = 12;
/// Floats before the vertices: the quad count (as f32) and padding.
const HEADER: usize = 4;
/// Prebuilt meshes: up to 2^15 quads (a layer past the 12000-particle cap with two renderers).
/// A larger one is made when needed (missing the frame it is made in).
const PREBUILT_BUCKETS: usize = 16;
/// A new layer's buffer holds this many quads; it doubles as needed.
const FIRST_CAP: usize = 64;

/// Bucket meshes: index k draws 2^k quads (dummy attributes, UV x = the local vertex index).
#[derive(Resource, Default)]
pub struct SpriteMeshes(Vec<Handle<Mesh>>);

impl SpriteMeshes {
    fn bucket_of(quads: usize) -> usize {
        quads.max(1).next_power_of_two().trailing_zeros() as usize
    }

    fn get(&mut self, meshes: &mut Assets<Mesh>, k: usize) -> Handle<Mesh> {
        while self.0.len() <= k {
            let n = 1usize << self.0.len();
            self.0.push(meshes.add(bucket_mesh(n)));
        }
        self.0[k].clone()
    }

    /// The one-quad mesh a new layer entity starts with (see `SpriteBuf::new`).
    pub fn first(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.get(meshes, 0)
    }

    /// Builds the common buckets up front (their first frame isn't a mesh upload).
    pub fn prebuild(&mut self, meshes: &mut Assets<Mesh>) {
        self.get(meshes, PREBUILT_BUCKETS - 1);
    }
}

fn bucket_mesh(quads: usize) -> Mesh {
    let n = quads * 4;
    // The attributes are those the sprite meshes had (the same vertex layout and shader defs);
    // only UV x, the vertex's index, is read.
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        (0..n).map(|i| [i as f32, 0.0]).collect::<Vec<_>>(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; n]);
    m.insert_indices(Indices::U32(
        (0..quads as u32)
            .flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            })
            .collect(),
    ));
    m
}

/// Buffer writes for the render world, in order (taken at extraction).
#[derive(Resource, Default)]
pub struct SpriteUploads(Vec<(Buffer, Vec<f32>)>);

/// One layer's vertex buffer and the bucket its entity draws.
///
/// Timing follows the mesh path exactly. `Assets::asset_events` runs early in PostUpdate, before
/// the particle systems, so a mesh modified in frame N reached the render world in frame N+1,
/// with the content the mesh asset had then (the particle system had usually written it again
/// by then). The buffer gets the same contents on the same frames: a frame's quads reach it only
/// if the old mesh would have been modified the frame before; otherwise the layer draws what it
/// last drew (on the first frame of quads after an idle stretch, the empty mesh, or for the
/// hand-built layers, which left an emptied mesh alone, its last quads).
pub struct SpriteBuf {
    pub buffer: Buffer,
    cap: usize,
    bucket: Option<usize>,
    /// The old mesh asset was modified last frame: its content reaches the GPU this frame.
    pending: bool,
    /// Made this frame, after the asset events (counts as a modification).
    made: bool,
    /// The old mesh asset held quads (not the empty mesh).
    nonempty: bool,
    /// The old mesh's content when it was modified without reaching the GPU (it does next frame).
    held: Option<(usize, Vec<f32>)>,
}

pub fn new_buffer(device: &RenderDevice, quads: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("sprite vertices"),
        size: ((HEADER + quads * 4 * STRIDE) * 4) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl SpriteBuf {
    /// For an entity spawned with `SpriteMeshes::first`. `before_asset_events`: made before
    /// PostUpdate (the old mesh asset's creation reached the render world this frame), else
    /// made by the particle system itself (it reaches it next frame). The new buffer is zeroed:
    /// no quads.
    pub fn new(device: &RenderDevice, before_asset_events: bool) -> Self {
        SpriteBuf {
            buffer: new_buffer(device, FIRST_CAP),
            cap: FIRST_CAP,
            bucket: Some(0),
            pending: before_asset_events,
            made: !before_asset_events,
            nonempty: false,
            held: None,
        }
    }

    /// A vertex list to fill with `push`: the header, then `STRIDE` floats per vertex.
    pub fn begin(quads: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(HEADER + quads * 4 * STRIDE);
        v.extend([0.0; HEADER]);
        v
    }

    pub fn push(v: &mut Vec<f32>, pos: [f32; 3], normal: [f32; 3], uv: [f32; 2], col: [f32; 4]) {
        v.extend([
            pos[0], pos[1], pos[2], uv[0], normal[0], normal[1], normal[2], uv[1], col[0], col[1],
            col[2], col[3],
        ]);
    }

    /// A frame in which the layer's mesh was written (or emptied): this frame's `quads`, pushed
    /// in draw order into `v` (from `begin`). Exactly one of `frame` or `idle` runs per layer
    /// every frame. When the buffer has to grow, the new one is returned for the caller to put
    /// in the layer's material (see `swap_buffer`).
    pub fn frame(
        &mut self,
        quads: usize,
        v: Vec<f32>,
        entity: Entity,
        ctx: &mut SpriteCtx,
    ) -> Option<Buffer> {
        debug_assert_eq!(v.len(), HEADER + quads * 4 * STRIDE);
        // The old code wrote the mesh when it had quads and emptied it when it had none.
        let modified = self.made || quads > 0 || self.nonempty;
        self.made = false;
        self.nonempty = quads > 0;
        let mut grown = None;
        if self.pending {
            self.held = None;
            grown = self.upload(quads, v, entity, ctx);
        } else if modified {
            self.held = Some((quads, v));
        }
        self.pending = modified;
        grown
    }

    /// A frame in which the layer's mesh was left alone.
    pub fn idle(&mut self, entity: Entity, ctx: &mut SpriteCtx) -> Option<Buffer> {
        let mut grown = None;
        if let (true, Some((quads, v))) = (self.pending, self.held.take()) {
            grown = self.upload(quads, v, entity, ctx);
        }
        self.held = None;
        self.pending = std::mem::take(&mut self.made);
        grown
    }

    fn upload(
        &mut self,
        quads: usize,
        mut v: Vec<f32>,
        entity: Entity,
        ctx: &mut SpriteCtx,
    ) -> Option<Buffer> {
        v[0] = quads as f32;
        let mut grown = None;
        if quads > self.cap {
            self.cap = quads.next_power_of_two();
            self.buffer = new_buffer(ctx.device, self.cap);
            grown = Some(self.buffer.clone());
        }
        if quads > 0 {
            let k = SpriteMeshes::bucket_of(quads);
            if self.bucket != Some(k) {
                self.bucket = Some(k);
                ctx.commands
                    .entity(entity)
                    .insert(Mesh3d(ctx.buckets.get(ctx.meshes, k)));
            }
        }
        ctx.uploads.0.push((self.buffer.clone(), v));
        grown
    }
}

/// What `SpriteBuf::frame` and `idle` need besides the layer.
pub struct SpriteCtx<'a, 'w, 's> {
    pub device: &'a RenderDevice,
    pub uploads: &'a mut SpriteUploads,
    pub buckets: &'a mut SpriteMeshes,
    pub meshes: &'a mut Assets<Mesh>,
    pub commands: &'a mut Commands<'w, 's>,
}

/// Puts a grown buffer in a layer's material, reaching the render world this frame like the
/// buffer's contents (a tracked change would arrive a frame late, after the asset events): the
/// material is changed untracked and its modified event sent directly.
pub fn swap_buffer<M: Asset>(
    mats: &mut Assets<M>,
    events: &mut MessageWriter<AssetEvent<M>>,
    h: &Handle<M>,
    buffer: Buffer,
    slot: impl FnOnce(&mut M) -> &mut Buffer,
) {
    if let Some(m) = mats.get_mut_untracked(h) {
        *slot(m) = buffer;
        events.write(AssetEvent::Modified { id: h.id() });
    }
}

/// The render world's copy of the frame's writes.
#[derive(Resource, Default)]
struct PendingUploads(Vec<(Buffer, Vec<f32>)>);

fn extract_uploads(mut main: ResMut<MainWorld>, mut pending: ResMut<PendingUploads>) {
    if let Some(mut u) = main.get_resource_mut::<SpriteUploads>() {
        pending.0.append(&mut u.0);
    }
}

fn write_uploads(mut pending: ResMut<PendingUploads>, queue: Res<RenderQueue>) {
    for (buffer, v) in pending.0.drain(..) {
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&v));
    }
}

pub struct SpriteBufPlugin;

impl Plugin for SpriteBufPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "pull.wgsl");
        app.add_plugins(MaterialPlugin::<SpriteMaterial>::default());
        app.init_resource::<SpriteUploads>()
            .init_resource::<SpriteMeshes>()
            .add_systems(
                Startup,
                |mut b: ResMut<SpriteMeshes>, mut meshes: ResMut<Assets<Mesh>>| {
                    b.prebuild(&mut meshes)
                },
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<PendingUploads>()
                .add_systems(ExtractSchedule, extract_uploads)
                .add_systems(
                    Render,
                    write_uploads.in_set(RenderSystems::PrepareResources),
                );
        }
    }
}

/// The plain (`StandardMaterial`) sprite layers' material: its own fragment shader, the
/// vertices from the layer's buffer.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SpritePull {
    #[storage(103, read_only, buffer)]
    #[reflect(ignore, default = "no_buffer")]
    pub verts: Buffer,
}

/// Reflection never builds a sprite material (a buffer needs the render device).
pub fn no_buffer() -> Buffer {
    unreachable!("sprite vertex buffers are made with the render device")
}

impl bevy::pbr::MaterialExtension for SpritePull {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/pull.wgsl".into()
    }
}

pub type SpriteMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, SpritePull>;
