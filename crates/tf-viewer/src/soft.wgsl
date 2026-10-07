// Particle sprite material: Source's spritecard `$depthblend` (soft particles: a sprite fades
// where it is within `$depthblendscale` units of the scene behind it, so cards sitting on a
// surface or inside the cockpit don't show hard edges or wash the view out) and `$ignorez`
// (drawn over everything; the depth test is disabled in the pipeline specialisation).
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    view_transformations::depth_ndc_to_view_z,
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct Soft {
    // x: depth blend distance (metres), 0 = off.
    params: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> soft: Soft;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var fade = 1.0;
#ifdef DEPTH_PREPASS
    if soft.params.x > 0.0 {
        // View-space z is negative ahead of the eye: the scene behind this fragment is more
        // negative, and the fade is the gap over the blend distance.
        let scene_z = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
        let here_z = depth_ndc_to_view_z(in.position.z);
        fade = clamp((here_z - scene_z) / soft.params.x, 0.0, 1.0);
    }
#endif
    pbr_input.material.base_color.a *= fade;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
