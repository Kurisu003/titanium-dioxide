// The Pilot's cloak on the first-person arms and gun, as the engine's post-process draws a
// cloaked surface (materialsystem_dx11's engine_post_ps50 shader; constants from the
// cloak_pilot* ConVars, see cloak.rs): the picture is read at three points around the pixel,
// 120 degrees apart (+x, then up-left and down-left), weighted by the three tints, brightened
// (times BrightenMul plus BrightenAdd) and partly desaturated (Desaturate toward the luma
// 0.299/0.587/0.114), and that replaces the surface. The picture is the view transmission
// texture: a copy of what was drawn before the transmissive phase, the world included, since
// the viewmodel camera draws over the world's picture.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{globals, view_transmission_texture, view_transmission_sampler},
}

struct Cloak {
    // x: tap distance (fraction of the screen width), y: per-frame jitter of it
    // (cloak_pilotNoiseFactor), z: BrightenMul, w: BrightenAdd.
    params: vec4<f32>,
    // The three tap weights (cloak_pilotTint1-3 over their sum), w: cloak_pilotDesaturate.
    tints: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> cloak: Cloak;

fn hash(n: u32) -> f32 {
    var x = n * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x) / 4294967295.0;
}

fn tap(px: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let at = clamp(px / size, vec2(0.001), vec2(0.999));
    return textureSampleLevel(view_transmission_texture, view_transmission_sampler, at, 0.0).rgb;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let size = vec2<f32>(textureDimensions(view_transmission_texture));
    // The engine re-rolls the distance every frame, x and y apart: scale * (1 + noise * r)
    // with r in -1..1 (materialsystem_dx11.dll 0x18003af97).
    let f = globals.frame_count;
    let r = vec2(hash(f * 2u), hash(f * 2u + 1u)) * 2.0 - 1.0;
    let d = cloak.params.x * size.x * (1.0 + cloak.params.y * r);
    let px = in.position.xy;
    let a = tap(px + d * vec2(1.0, 0.0), size);
    let b = tap(px + d * vec2(-0.5, -0.86603), size);
    let c = tap(px + d * vec2(-0.5, 0.86603), size);
    var col = (a * cloak.tints.x + b * cloak.tints.y + c * cloak.tints.z) * cloak.params.z + cloak.params.w;
    let luma = dot(col, vec3(0.299, 0.587, 0.114));
    col = mix(col, vec3(luma), cloak.tints.w);
    var out: FragmentOutput;
    out.color = vec4(col, 1.0);
    return out;
}
