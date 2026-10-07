// Effect surfaces with the game's shader features (see uber.rs): UV1 transform and scroll,
// alpha distance and edge fades, vertex colour tint/alpha, albedo tint and opacity. Blending
// (additive or alpha) and unlit come from the base StandardMaterial.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::{view, globals},
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{prepass_io::{VertexOutput, FragmentOutput}, pbr_deferred_functions::deferred_output}
#else
#import bevy_pbr::{forward_io::{VertexOutput, FragmentOutput}, pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}}
#endif

struct UberParams {
    uv: vec4<f32>,
    uv_t: vec4<f32>,
    fade: vec4<f32>,
    fade2: vec4<f32>,
    tint: vec4<f32>,
    uv2: vec4<f32>,
    uv2_t: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> uber: UberParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var opacity_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var opacity_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var distort_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var distort_samp: sampler;

fn has(flag: u32) -> bool {
    return (u32(uber.fade2.y) & flag) != 0u;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var vin = in;
#ifdef VERTEX_UVS_A
    var t = uber.uv_t.xy;
    if (uber.uv_t.z > 0.5) {
        // Scrolling textures wrap; keep the offset small so precision holds over long sessions.
        t = fract(t * globals.time);
    }
    let uv = in.uv;
    vin.uv = vec2<f32>(uber.uv.x * uv.x + uber.uv.y * uv.y, uber.uv.z * uv.x + uber.uv.w * uv.y) + t;
    if (has(128u)) {
        // A scrolling offset map (two channels around 0.5) warps the colour UVs; the
        // amplitude is in uv2_t.w.
        var t2 = uber.uv2_t.xy;
        if (uber.uv2_t.z > 0.5) {
            t2 = fract(t2 * globals.time);
        }
        let uv2 = vec2<f32>(uber.uv2.x * uv.x + uber.uv2.y * uv.y, uber.uv2.z * uv.x + uber.uv2.w * uv.y) + t2;
        let d = textureSample(distort_tex, distort_samp, uv2).rg * 2.0 - 1.0;
        vin.uv = vin.uv + d * uber.uv2_t.w;
    }
#endif
    var pbr_input = pbr_input_from_standard_material(vin, is_front);
    var color = pbr_input.material.base_color;
    color = vec4<f32>(color.rgb * uber.tint.rgb, color.a * uber.fade2.z);
#ifdef VERTEX_UVS_A
    if (has(64u)) {
        color.a = color.a * textureSample(opacity_tex, opacity_samp, vin.uv).r;
    }
#endif
#ifdef VERTEX_COLORS
    if (has(4u)) {
        color = vec4<f32>(color.rgb * in.color.rgb, color.a);
    }
    if (has(8u)) {
        color.a = color.a * in.color.a;
    }
#endif
#ifndef PREPASS_PIPELINE
    let to_eye = view.world_position.xyz - in.world_position.xyz;
    if (has(16u)) {
        // Distance in game units (the world is scaled by UNIT).
        let d = length(to_eye) * uber.uv_t.w;
        color.a = color.a * clamp(d * uber.fade.x + uber.fade.y, 0.0, 1.0);
    }
    if (has(32u)) {
        let ndv = abs(dot(normalize(in.world_normal), normalize(to_eye)));
        let inner = uber.fade.w;
        let outer = uber.fade2.x;
        let k = clamp((ndv - outer) / max(inner - outer, 1e-4), 0.0, 1.0);
        color.a = color.a * pow(k, max(uber.fade.z, 1e-3));
    }
#endif
    pbr_input.material.base_color = color;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}
