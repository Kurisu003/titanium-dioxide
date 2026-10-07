// Refracting particle sprites: the game's `Refract` particle materials (heat haze, shockwave
// rings, the vortex's pinched ring). They draw in Bevy's transmissive phase, whose view
// transmission texture is a copy of the scene drawn so far (opaque world, sky), and look it up
// offset by the material's normal map, following Source's Refract shader: the screen-space
// offset is the normal's XY times `$refractamount`, the normal map's alpha and the vertex
// alpha; `$vertexcolormodulate` tints the result by the vertex colour; `$bluramount` blurs
// it. Respawn's `$opacitymap` is read as a mask on all of it: it scales the offset as well as
// blending the sprite, so the edges match the scene and the middle is a clean warp (blending
// alone would show a faded second copy of the scene). The vertex alpha fades the blur and the
// tint with the offset.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view_transmission_texture, view_transmission_sampler},
}

struct Refract {
    // x: $refractamount, y: $bluramount (pixels), z: 1 = tint by the vertex colour.
    params: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> refract: Refract;
// RG: the normal map's XY, B: the opacity map, A: the normal map's alpha.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var normal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var normal_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var uv = vec2(0.5);
#ifdef VERTEX_UVS_A
    uv = in.uv;
#endif
    var color = vec4(1.0);
#ifdef VERTEX_COLORS
    color = in.color;
#endif
    let s = textureSample(normal_texture, normal_sampler, uv);
    let n = s.xy * 2.0 - 1.0;
    let size = vec2<f32>(textureDimensions(view_transmission_texture));
    let offset = n * refract.params.x * s.a * color.a * s.b;
    let at = in.position.xy / size + offset;
    var scene = textureSampleLevel(view_transmission_texture, view_transmission_sampler, at, 0.0).rgb;
    if refract.params.y > 0.0 {
        let d = refract.params.y * color.a / size;
        scene = (scene * 2.0
            + textureSampleLevel(view_transmission_texture, view_transmission_sampler, at + vec2(d.x, d.y), 0.0).rgb
            + textureSampleLevel(view_transmission_texture, view_transmission_sampler, at + vec2(-d.x, d.y), 0.0).rgb
            + textureSampleLevel(view_transmission_texture, view_transmission_sampler, at + vec2(d.x, -d.y), 0.0).rgb
            + textureSampleLevel(view_transmission_texture, view_transmission_sampler, at + vec2(-d.x, -d.y), 0.0).rgb) / 6.0;
    }
    if refract.params.z > 0.0 {
        scene *= mix(vec3(1.0), min(color.rgb, vec3(1.0)), color.a);
    }
    var out: FragmentOutput;
    out.color = vec4(scene, s.b);
    return out;
}
