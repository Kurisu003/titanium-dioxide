// Batched static props (propbatch.rs): one mesh holds many small props, each vertex naming its
// prop instance and its model vertex. This vertex shader does per prop what Bevy's mesh.wgsl
// and prepass.wgsl (and the GPU preprocessing pass before them) do per entity, with the same
// functions in the same order, so the fragments come out the same; the fragment shaders are
// StandardMaterial's own.
#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#import bevy_render::maths::{affine3_to_square, inverse_affine3, mat2x4_f32_to_mat3x3_unpack}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::VertexOutput
#else
#import bevy_pbr::forward_io::VertexOutput
#endif

// Per prop and LOD, four vec4s: the world_from_local affine (transposed, as Bevy's mesh input
// uniform holds it) and the draw range (Bevy's visibility range table entry: start margin
// start and end, end margin start and end).
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage, read> prop_instances: array<vec4<f32>>;
// Per model vertex, three vec4s: position.xyz normal.x, normal.yz uv.xy, tangent.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage, read> prop_vertices: array<vec4<f32>>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    // (prop instance, model vertex)
    @location(0) ids: vec2<u32>,
}

// `mesh_functions::get_visibility_range_dither_level` with the prop's own range and position.
fn dither_level(lod_range: vec4<f32>, world_position: vec4<f32>) -> i32 {
    let camera_distance = length(view.world_position.xyz - world_position.xyz);
    let offset = select(-16, 0, camera_distance >= lod_range.z);
    let bounds = select(lod_range.xy, lod_range.zw, camera_distance >= lod_range.z);
    let level = i32(round((camera_distance - bounds.x) / (bounds.y - bounds.x) * 16.0));
    return offset + clamp(level, 0, 16);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let i = vertex.ids.x * 4u;
    let world_from_local_affine_transpose = mat3x4<f32>(prop_instances[i], prop_instances[i + 1u], prop_instances[i + 2u]);
    let lod_range = prop_instances[i + 3u];
    let world_from_local = affine3_to_square(world_from_local_affine_transpose);

    // Bevy's CPU range test (`VisibilityRange::is_visible_at_all`, from the prop's origin):
    // out of range, the prop isn't drawn at all. Every vertex of a triangle belongs to the
    // same prop, so the whole triangle lands on one point outside the clip volume.
    let camera_distance = length(view.world_position.xyz - world_from_local[3].xyz);
    if camera_distance < lod_range.x || camera_distance >= lod_range.w {
        out.position = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return out;
    }

    let v = vertex.ids.y * 3u;
    let a = prop_vertices[v];
    let b = prop_vertices[v + 1u];
    let position = a.xyz;
    let normal = vec3<f32>(a.w, b.xy);
    let uv = b.zw;
    let tangent = prop_vertices[v + 2u];

    // The inverse transpose as mesh_preprocess.wgsl packs it into the mesh uniform.
    let local_from_world_transpose = transpose(inverse_affine3(transpose(world_from_local_affine_transpose)));
    let local_from_world_transpose_a = mat2x4<f32>(
        vec4<f32>(local_from_world_transpose[0].xyz, local_from_world_transpose[1].x),
        vec4<f32>(local_from_world_transpose[1].yz, local_from_world_transpose[2].xy));
    let local_from_world_transpose_b = local_from_world_transpose[2].z;

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);

#ifdef VERTEX_UVS_A
    out.uv = uv;
#endif

#ifdef PREPASS_PIPELINE
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
#endif

#ifdef VERTEX_NORMALS
    // `mesh_functions::mesh_normal_local_to_world`
    if any(normal != vec3<f32>(0.0)) {
        out.world_normal = normalize(mat2x4_f32_to_mat3x3_unpack(local_from_world_transpose_a, local_from_world_transpose_b) * normal);
    } else {
        out.world_normal = normal;
    }
#endif

#ifdef VERTEX_TANGENTS
    // The sign of the determinant comes from the batch entity's flags: batches only hold props
    // whose transform has the same sign as the batch's (propbatch.rs).
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, tangent, vertex.instance_index);
#endif

#ifdef PREPASS_PIPELINE
#ifdef MOTION_VECTOR_PREPASS
    // Static: last frame's transform is this frame's.
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
#endif
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = dither_level(lod_range, world_from_local[3]);
#endif

    return out;
}
