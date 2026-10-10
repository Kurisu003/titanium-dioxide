// Particle sprite vertex shader (spritebuf.rs): Bevy's mesh vertex shader (bevy_pbr mesh.wgsl,
// unskinned, no morph targets) with the vertex read from the layer's storage buffer instead of
// the mesh's attributes. The mesh is a constant run of quads whose UV x is the vertex's index;
// vertices past the layer's quad count are put outside the clip volume.
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

// [0].x: the quad count; then per vertex (position, u), (normal, v), colour.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<storage, read> sprite: array<vec4<f32>>;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;

    var vertex = vertex_no_morph;
    let index = u32(vertex_no_morph.uv.x);
    if index >= u32(sprite[0].x) * 4u {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let a = sprite[1u + index * 3u];
    let b = sprite[2u + index * 3u];
    vertex.position = a.xyz;
    vertex.normal = b.xyz;
    vertex.uv = vec2<f32>(a.w, b.w);
    vertex.color = sprite[3u + index * 3u];

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416 .
    var world_from_local = mesh_world_from_local;

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416
    out.instance_index = vertex_no_morph.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);
#endif

    return out;
}
