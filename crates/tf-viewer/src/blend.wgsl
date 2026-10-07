// Two-layer world materials (`*_bm`): layer B (dirt, moss...) blended over layer A by the
// vertex alpha, shaped by the material's height mask so the edge follows the texture.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{prepass_io::{VertexOutput, FragmentOutput}, pbr_deferred_functions::deferred_output}
#else
#import bevy_pbr::{forward_io::{VertexOutput, FragmentOutput}, pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}}
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var layer_b: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var layer_b_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var blend_mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var blend_mask_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var amount = 0.0;
#ifdef VERTEX_COLORS
    amount = 1.0 - in.color.a;
#endif
#ifdef VERTEX_UVS_A
    let b = textureSample(layer_b, layer_b_sampler, in.uv);
    let m = textureSample(blend_mask, blend_mask_sampler, in.uv).r;
    // Height blend: where the mask is high, layer A holds on longer.
    let t = smoothstep(0.0, 1.0, clamp((amount - m) * 4.0 + 0.5, 0.0, 1.0));
    pbr_input.material.base_color = vec4<f32>(mix(pbr_input.material.base_color.rgb, b.rgb, t), 1.0);
#endif
    pbr_input.material.base_color.a = 1.0;
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
