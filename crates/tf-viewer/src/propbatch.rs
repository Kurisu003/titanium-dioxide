//! Small static props drawn from a few thousand batch meshes instead of one entity per prop,
//! mesh part and LOD (~42k of Angel City's ~46k prop entities).
//!
//! Every entity costs Bevy's per-frame passes (visibility ranges, frustum tests for every
//! camera, bounds and specialization checks, extraction) whether or not it is drawn. A batch
//! is one entity per cell, material and LOD: its mesh lists, for each prop in it, that prop's
//! copy of the model part as (prop instance, model vertex) pairs, and `propbatch.wgsl` reads
//! the model's vertices and the prop's transform and draw range from two storage buffers. It
//! does per prop what Bevy does per entity: the range test (out of range, the prop's
//! triangles collapse to a point outside the view), the dithered crossfade, and the vertex
//! transforms, with the same functions; StandardMaterial's own fragment shaders shade the
//! result. A batch's own draw range and bounds enclose its props', so Bevy only ever culls a
//! batch whose props would all have been culled.
//!
//! Only props that don't cast real-time shadows go into batches (shadow passes measure the
//! draw range from the camera but dither from the light's view, which a shared vertex shader
//! can't tell apart), and only opaque or alpha-tested materials (translucent ones are sorted
//! per entity). `TF_PROP_BATCH=0` spawns every prop as its own entities again (comparisons).

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::math::{Affine3, Vec3A};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexAttributeValues, VertexFormat};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderStorageBuffer;
use bevy::shader::{ShaderDefVal, ShaderRef};
use std::collections::HashMap;

/// The batch meshes' one vertex attribute: (prop instance, model vertex).
pub const ATTRIBUTE_PROP_IDS: MeshVertexAttribute = MeshVertexAttribute::new("PropIds", 0x7466_7072_6f70_6964, VertexFormat::Uint32x2);

const SHADER: &str = "embedded://tf_viewer/propbatch.wgsl";

/// Batch cell size (map units). Bigger cells make fewer entities but draw more vertices that
/// the range test then discards (whole cells are culled only when none of their props is in
/// range).
const CELL: f32 = 2048.0;

/// The storage buffers every batch material shares.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct PropBatch {
    #[storage(100, read_only)]
    pub instances: Handle<ShaderStorageBuffer>,
    #[storage(101, read_only)]
    pub vertices: Handle<ShaderStorageBuffer>,
}

impl MaterialExtension for PropBatch {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Bevy sets these from the mesh's attributes, which a batch mesh doesn't have: the
        // shader defs the props' own meshes (positions, normals, UVs, tangents) gave them.
        let has = |defs: &[ShaderDefVal], name: &str| defs.iter().any(|d| matches!(d, ShaderDefVal::Bool(n, true) if n == name));
        let prepass = has(&descriptor.vertex.shader_defs, "PREPASS_PIPELINE");
        let mut defs = vec!["VERTEX_POSITIONS", "VERTEX_UVS", "VERTEX_UVS_A"];
        if !prepass || has(&descriptor.vertex.shader_defs, "NORMAL_PREPASS_OR_DEFERRED_PREPASS") {
            defs.extend(["VERTEX_NORMALS", "VERTEX_TANGENTS"]);
        }
        for list in std::iter::once(&mut descriptor.vertex.shader_defs).chain(descriptor.fragment.as_mut().map(|f| &mut f.shader_defs)) {
            for d in &defs {
                if !has(list, d) {
                    list.push((*d).into());
                }
            }
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[ATTRIBUTE_PROP_IDS.at_shader_location(0)])?];
        Ok(())
    }
}

pub type PropBatchMaterial = ExtendedMaterial<StandardMaterial, PropBatch>;

pub struct PropBatchPlugin;

impl Plugin for PropBatchPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "propbatch.wgsl");
        app.add_plugins(MaterialPlugin::<PropBatchMaterial>::default());
    }
}

/// Whether small props are batched (`TF_PROP_BATCH=0` turns it off).
pub fn enabled() -> bool {
    std::env::var("TF_PROP_BATCH").map_or(true, |v| v != "0")
}

/// Whether a prop part with this mesh and material can go into a batch: an opaque or
/// alpha-tested material that no other system swaps out, and a mesh with exactly the
/// attributes the batch shader stands in for.
pub fn batchable(mesh: &Mesh, material: &StandardMaterial, swapped: bool) -> bool {
    use VertexAttributeValues as V;
    let attr = |a: MeshVertexAttribute| mesh.attribute(a);
    !swapped
        && matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
        && mesh.primitive_topology() == PrimitiveTopology::TriangleList
        && mesh.indices().is_some()
        && mesh.attributes().count() == 4
        && matches!(attr(Mesh::ATTRIBUTE_POSITION), Some(V::Float32x3(_)))
        && matches!(attr(Mesh::ATTRIBUTE_NORMAL), Some(V::Float32x3(_)))
        && matches!(attr(Mesh::ATTRIBUTE_UV_0), Some(V::Float32x2(_)))
        && matches!(attr(Mesh::ATTRIBUTE_TANGENT), Some(V::Float32x4(_)))
}

/// A model part's vertices in the shared vertex buffer, its indices and its bounds.
struct PoolPart {
    base: u32,
    count: u32,
    indices: Vec<u32>,
    min: Vec3,
    max: Vec3,
}

/// One prop's part in a batch.
struct Member {
    instance: u32,
    part: usize,
    /// Bounds in the static root's space (map units).
    min: Vec3,
    max: Vec3,
    /// Where its draw range is measured from (world space) and the range.
    origin: Vec3A,
    start: f32,
    end: f32,
}

struct Group {
    material: Handle<StandardMaterial>,
    members: Vec<Member>,
}

/// Collects batched prop parts while the props spawn; `finish` builds the batches.
pub struct PropBatcher {
    /// The static root's transform, as Bevy will compute it (it has no parent).
    root: GlobalTransform,
    /// A batch entity's transform (identity under the root), as Bevy will compute it.
    batch: GlobalTransform,
    instances: Vec<Vec4>,
    vertices: Vec<Vec4>,
    parts: Vec<PoolPart>,
    part_index: HashMap<(usize, usize, usize), usize>,
    instance_index: HashMap<(usize, usize), u32>,
    groups: Vec<Group>,
    group_index: HashMap<(IVec3, AssetId<StandardMaterial>, usize), usize>,
}

impl PropBatcher {
    pub fn new(root: &Transform) -> Self {
        let root = GlobalTransform::from(*root);
        Self {
            root,
            batch: root.mul_transform(Transform::IDENTITY),
            instances: Vec::new(),
            vertices: Vec::new(),
            parts: Vec::new(),
            part_index: HashMap::new(),
            instance_index: HashMap::new(),
            groups: Vec::new(),
            group_index: HashMap::new(),
        }
    }

    /// Add part `part` of LOD `lod` of prop `prop` (a `model` placed by `tf` under the static
    /// root, drawn in `range`). False if it can't be batched (the caller spawns it as before).
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        prop: usize,
        model: usize,
        lod: usize,
        part: usize,
        mesh: &Mesh,
        material: &Handle<StandardMaterial>,
        tf: &Transform,
        range: &VisibilityRange,
    ) -> bool {
        // What Bevy's propagation gives the prop's entity, and its mesh input uniform.
        let global = self.root.mul_transform(*tf);
        let affine = global.affine();
        // The tangent sign comes from the batch entity (see the shader).
        if affine.matrix3.determinant().is_sign_positive() != self.batch.affine().matrix3.determinant().is_sign_positive() {
            return false;
        }
        if !range.end_margin.end.is_finite() || range.use_aabb {
            return false;
        }
        let pi = match self.part_index.get(&(model, lod, part)) {
            Some(&i) => i,
            None => {
                let Some(p) = self.pool(mesh) else { return false };
                self.parts.push(p);
                self.part_index.insert((model, lod, part), self.parts.len() - 1);
                self.parts.len() - 1
            }
        };
        let (pmin, pmax) = (self.parts[pi].min, self.parts[pi].max);
        // The part's bounds in the root's space (a little bigger: these are float sums), and
        // how far from its origin it reaches in the world.
        let m = tf.compute_affine();
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut reach = 0.0f32;
        for c in 0..8 {
            let corner = Vec3::new(if c & 1 == 0 { pmin.x } else { pmax.x }, if c & 2 == 0 { pmin.y } else { pmax.y }, if c & 4 == 0 { pmin.z } else { pmax.z });
            let p = m.transform_point3(corner);
            min = min.min(p);
            max = max.max(p);
            reach = reach.max(affine.transform_point3(corner).distance(affine.translation.into()));
        }
        // Batches skip Bevy's per-prop frustum test, which only changes the picture through
        // the far plane (the projection itself has none): batched props must stay nearer.
        if range.end_margin.end + reach * 1.01 + 1.0 >= bevy::camera::PerspectiveProjection::default().far {
            return false;
        }
        let pad = Vec3::splat(1.0) + (max - min).abs() * 1e-4;
        let (min, max) = (min - pad, max + pad);
        let instance = *self.instance_index.entry((prop, lod)).or_insert_with(|| {
            let t = Affine3::from(&affine).to_transpose();
            self.instances.extend(t);
            self.instances.push(Vec4::new(range.start_margin.start, range.start_margin.end, range.end_margin.start, range.end_margin.end));
            (self.instances.len() / 4 - 1) as u32
        });
        let cell = (tf.translation / CELL).floor().as_ivec3();
        let gi = *self.group_index.entry((cell, material.id(), lod)).or_insert_with(|| {
            self.groups.push(Group { material: material.clone(), members: Vec::new() });
            self.groups.len() - 1
        });
        self.groups[gi].members.push(Member { instance, part: pi, min, max, origin: global.translation_vec3a(), start: range.start_margin.start, end: range.end_margin.end });
        true
    }

    /// Put a part's vertices into the shared vertex buffer.
    fn pool(&mut self, mesh: &Mesh) -> Option<PoolPart> {
        use VertexAttributeValues as V;
        let (Some(V::Float32x3(pos)), Some(V::Float32x3(nrm)), Some(V::Float32x2(uv)), Some(V::Float32x4(tan))) = (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            mesh.attribute(Mesh::ATTRIBUTE_UV_0),
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT),
        ) else {
            return None;
        };
        let indices: Vec<u32> = mesh.indices()?.iter().map(|i| i as u32).collect();
        if pos.is_empty() || indices.iter().any(|&i| i as usize >= pos.len()) {
            return None;
        }
        let base = (self.vertices.len() / 3) as u32;
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for i in 0..pos.len() {
            let (p, n, t, w) = (pos[i], nrm[i], uv[i], tan[i]);
            self.vertices.push(Vec4::new(p[0], p[1], p[2], n[0]));
            self.vertices.push(Vec4::new(n[1], n[2], t[0], t[1]));
            self.vertices.push(Vec4::from_array(w));
            min = min.min(Vec3::from_array(p));
            max = max.max(Vec3::from_array(p));
        }
        Some(PoolPart { base, count: pos.len() as u32, indices, min, max })
    }

    /// Build the batch meshes and spawn them under `root`. Returns the number of batches.
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        self,
        commands: &mut Commands,
        root: Entity,
        meshes: &mut Assets<Mesh>,
        materials: &Assets<StandardMaterial>,
        batch_materials: &mut Assets<PropBatchMaterial>,
        buffers: &mut Assets<ShaderStorageBuffer>,
    ) -> usize {
        if self.groups.is_empty() {
            return 0;
        }
        let bytes = |v: &[Vec4]| v.iter().flat_map(|x| x.to_array()).flat_map(f32::to_le_bytes).collect::<Vec<u8>>();
        let instances = buffers.add(ShaderStorageBuffer::new(&bytes(&self.instances), RenderAssetUsages::RENDER_WORLD));
        let vertices = buffers.add(ShaderStorageBuffer::new(&bytes(&self.vertices), RenderAssetUsages::RENDER_WORLD));
        // Split groups into meshes of at most 65535 vertices (16-bit indices).
        let mut chunks: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            let (mut start, mut n) = (0, 0u32);
            for (mi, m) in g.members.iter().enumerate() {
                let c = self.parts[m.part].count;
                if n > 0 && n + c > u16::MAX as u32 {
                    chunks.push((gi, start..mi));
                    (start, n) = (mi, 0);
                }
                n += c;
            }
            chunks.push((gi, start..g.members.len()));
        }
        let built = crate::world::parallel_map(&chunks, |(gi, r)| {
            let members = &self.groups[*gi].members[r.clone()];
            let (mut ids, mut idx) = (Vec::new(), Vec::new());
            for m in members {
                let p = &self.parts[m.part];
                let first = ids.len() as u32;
                idx.extend(p.indices.iter().map(|&i| first + i));
                ids.extend((0..p.count).map(|k| [m.instance, p.base + k]));
            }
            let indices = if ids.len() <= u16::MAX as usize + 1 { Indices::U16(idx.into_iter().map(|i| i as u16).collect()) } else { Indices::U32(idx) };
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(ATTRIBUTE_PROP_IDS, VertexAttributeValues::Uint32x2(ids))
                .with_inserted_indices(indices)
        });
        let mut mats: HashMap<AssetId<StandardMaterial>, Handle<PropBatchMaterial>> = HashMap::new();
        let batch = self.batch.affine();
        for ((gi, r), mesh) in chunks.iter().zip(built) {
            let Some(mesh) = mesh else { continue };
            let g = &self.groups[*gi];
            let members = &g.members[r.clone()];
            let Some(base) = materials.get(&g.material) else { continue };
            let mat = mats
                .entry(g.material.id())
                .or_insert_with(|| batch_materials.add(PropBatchMaterial { base: base.clone(), extension: PropBatch { instances: instances.clone(), vertices: vertices.clone() } }))
                .clone();
            let (min, max) = members.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), m| (a.min(m.min), b.max(m.max)));
            let aabb = Aabb::from_min_max(min, max);
            // The batch's range, from where Bevy measures it (its box's centre, `use_aabb`):
            // wide enough for every prop's range, with a margin for rounding.
            let centre = batch.transform_point3a(aabb.center);
            let (mut lo, mut hi) = (f32::INFINITY, 0.0f32);
            for m in members {
                let d = m.origin.distance(centre);
                lo = lo.min(m.start - d);
                hi = hi.max(m.end + d);
            }
            let lo = (lo * 0.999 - 0.1).max(0.0);
            let hi = hi * 1.001 + 0.1;
            // A crossfade margin, so the batch is specialized with the dither (Bevy only
            // dithers entities whose range has one); the shader dithers each prop by its own.
            let range = VisibilityRange { start_margin: lo..lo, end_margin: hi * 0.999..hi, use_aabb: true };
            let e = commands
                .spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::IDENTITY, aabb, NoAutoAabb, range, bevy::light::NotShadowCaster))
                .id();
            commands.entity(root).add_child(e);
        }
        chunks.len()
    }
}
