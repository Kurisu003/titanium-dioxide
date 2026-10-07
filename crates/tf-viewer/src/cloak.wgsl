// The Pilot's cloak on the first-person arms and gun: the game draws a cloaked player as a
// refracting outline (engine-side; the scripts only call EnableCloak). Each fragment shows the
// picture behind it (the view transmission texture: a copy of what was drawn before the
// transmissive phase, the world included, since the viewmodel camera draws over the world's
// picture), shifted by the surface's view-space normal, with a faint cool tint and a fresnel
// rim so the silhouette reads. (The scene is linear HDR, so the rim is small.)
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, view_transmission_texture, view_transmission_sampler},
}

struct Cloak {
    // x: refraction offset (fraction of the screen), y: rim strength, z: rim exponent,
    // w: how much of the cool tint applies.
    params: vec4<f32>,
    rim_color: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> cloak: Cloak;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var n = normalize(in.world_normal);
    if !is_front {
        n = -n;
    }
    let to_eye = normalize(view.world_position.xyz - in.world_position.xyz);
    let vn = (view.view_from_world * vec4(n, 0.0)).xyz;
    let size = vec2<f32>(textureDimensions(view_transmission_texture));
    // Screen y runs down, view y up.
    let offset = vec2(vn.x, -vn.y) * cloak.params.x;
    let at = clamp(in.position.xy / size + offset, vec2(0.001), vec2(0.999));
    var scene = textureSampleLevel(view_transmission_texture, view_transmission_sampler, at, 0.0).rgb;
    scene *= mix(vec3(1.0), vec3(0.82, 0.9, 1.0), cloak.params.w);
    let rim = pow(1.0 - clamp(abs(dot(n, to_eye)), 0.0, 1.0), cloak.params.z) * cloak.params.y;
    var out: FragmentOutput;
    out.color = vec4(scene + cloak.rim_color.rgb * rim, 1.0);
    return out;
}
