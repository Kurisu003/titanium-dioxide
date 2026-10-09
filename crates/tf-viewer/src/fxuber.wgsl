// The game's unlit uber pixel shader for effect models (see fxuber.rs for where each term
// comes from). Output is premultiplied: additive materials write alpha 0 (Bevy's premultiplied
// blend then adds), translucent ones their alpha.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
}

struct FxUber {
    uv_rs: array<vec4<f32>, 3>,
    uv_t: array<vec4<f32>, 3>,
    distort: vec4<f32>,
    albedo: vec4<f32>,
    aef: vec4<f32>,
    eef: vec4<f32>,
    emissive: vec4<f32>,
    misc: vec4<f32>,
    modulation: vec4<f32>,
    tex_uv: vec4<u32>,
    tex_uv2: vec4<u32>,
    uv_flags: vec4<u32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> fx: FxUber;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var albedo_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var albedo_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var emissive_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var emissive_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var emul_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var emul_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var opam_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var opam_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var detail_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var detail_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var dist1_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var dist1_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var dist2_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var dist2_samp: sampler;

const ALBEDO: u32 = 1u;
const EMISSIVE: u32 = 2u;
const EMUL: u32 = 4u;
const OPAM: u32 = 8u;
const DETAIL: u32 = 16u;
const UVD1: u32 = 32u;
const UVD2: u32 = 64u;
const VCOLT: u32 = 128u;
const VCOLA: u32 = 256u;
const AEF: u32 = 512u;
const EEF: u32 = 1024u;
const ADF: u32 = 2048u;
const ADD: u32 = 4096u;

fn has(f: u32) -> bool {
    return (fx.tex_uv2.w & f) != 0u;
}

// UV transform `t` (1-3; 0 is the plain mesh UV) of the mesh or screen UV, with the
// distortion offset `d` where the transform takes it.
fn uv_of(t: u32, mesh_uv: vec2<f32>, screen_uv: vec2<f32>, d: vec2<f32>) -> vec2<f32> {
    if t == 0u {
        return mesh_uv;
    }
    let i = t - 1u;
    let f = fx.uv_flags[i];
    var base = mesh_uv;
    if (f & 2u) != 0u {
        base = screen_uv;
    }
    let rs = fx.uv_rs[i];
    var tr = fx.uv_t[i].xy;
    if (f & 1u) != 0u {
        // c_gameTime times the scroll speed; the textures wrap, so keep the offset small.
        tr = fract(tr * globals.time);
    }
    var uv = vec2(dot(base, rs.xy), dot(base, rs.zw)) + tr;
    if (f & 4u) != 0u {
        uv = uv + d * vec2(rs.x, rs.w);
    }
    return uv;
}

// The edge fades: max(lerp(inner, outer, pow(max(1 - |N.V|, 0), exponent)), 0).
fn edge(ndv: f32, p: vec3<f32>) -> f32 {
    let x = max(1.0 - abs(ndv), 1e-6);
    return max(mix(p.y, p.z, pow(x, p.x)), 0.0);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let screen_uv = in.position.xy / view.viewport.zw;
    var mesh_uv = vec2(0.0);
#ifdef VERTEX_UVS_A
    mesh_uv = in.uv;
#endif
    var vcol = vec4(1.0);
#ifdef VERTEX_COLORS
    vcol = in.color;
#endif
    let to_px = in.world_position.xyz - view.world_position.xyz;
    let dist = length(to_px);
    let ndv = dot(normalize(in.world_normal), -to_px / max(dist, 1e-6));

    // Distortion offsets (their own UVs are never distorted).
    var d = vec2(0.0);
    if has(UVD1) {
        let uv = uv_of(fx.tex_uv2.y, mesh_uv, screen_uv, vec2(0.0));
        d = d + (textureSample(dist1_tex, dist1_samp, uv).xy * 2.0 - 1.0) * fx.distort.xy;
    }
    if has(UVD2) {
        let uv = uv_of(fx.tex_uv2.z, mesh_uv, screen_uv, vec2(0.0));
        d = d + (textureSample(dist2_tex, dist2_samp, uv).xy * 2.0 - 1.0) * fx.distort.zw;
    }

    // Albedo and alpha.
    let a_tex = textureSample(albedo_tex, albedo_samp, uv_of(fx.tex_uv.x, mesh_uv, screen_uv, d));
    var albedo = a_tex.rgb * fx.albedo.rgb * fx.modulation.rgb;
    var alpha = a_tex.a * fx.albedo.w * fx.modulation.a;
    if has(VCOLT) {
        albedo = albedo * vcol.rgb;
    }
    if has(VCOLA) {
        alpha = alpha * vcol.a;
    }
    if has(OPAM) {
        alpha = alpha * textureSample(opam_tex, opam_samp, uv_of(fx.tex_uv.w, mesh_uv, screen_uv, d)).r;
    }
    if has(DETAIL) {
        let det = textureSample(detail_tex, detail_samp, uv_of(fx.tex_uv2.x, mesh_uv, screen_uv, d)).rgb;
        albedo = albedo * 2.0 * clamp(det - vec3(0.00196081), vec3(0.0), vec3(1.0));
    }
    if has(AEF) {
        alpha = alpha * edge(ndv, fx.aef.xyz);
    }
    if has(ADF) {
        alpha = alpha * clamp(dist * fx.emissive.w * fx.aef.w + fx.eef.w, 0.0, 1.0);
    }

    // Emissive.
    var emissive = vec3(0.0);
    if has(EMISSIVE) {
        emissive = textureSample(emissive_tex, emissive_samp, uv_of(fx.tex_uv.y, mesh_uv, screen_uv, d)).rgb * fx.emissive.rgb;
        if has(EMUL) {
            emissive = emissive * textureSample(emul_tex, emul_samp, uv_of(fx.tex_uv.z, mesh_uv, screen_uv, d)).rgb;
        }
        emissive = emissive * fx.modulation.rgb;
        if has(EEF) {
            emissive = emissive * edge(ndv, fx.eef.xyz);
        }
    }

    // Alpha is held above 0 unless both alpha-modulate switches are on; the emissive is
    // divided by it when c_useAlphaModulateEmissive is 0 or less (so the final multiply leaves
    // it unfaded).
    var a = alpha;
    if fx.misc.x == 0.0 || fx.misc.y == 0.0 {
        a = max(alpha, 1e-5);
    }
    if fx.misc.y <= 0.0 {
        emissive = emissive / a;
    }
    // The albedo's brightest channel is held to c_maxLightingValue.
    let peak = max(max(albedo.r, albedo.g), albedo.b);
    let color = albedo * fx.misc.z / max(peak, fx.misc.z) + emissive;

    var out: FragmentOutput;
    if has(ADD) {
        out.color = vec4(color * a, 0.0);
    } else {
        out.color = vec4(color * a, a);
    }
    return out;
}
