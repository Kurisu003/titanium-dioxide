// The minimap: the map's overview image, rotated so the player's heading is up, in a circle
// over a dark disc. params: the player's uv in the image and the heading (cos, sin) in game
// xy; extent: the circle's radius as a fraction of the image, the image's alpha.
#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> params: vec4<f32>;
@group(1) @binding(1) var<uniform> extent: vec4<f32>;
@group(1) @binding(2) var map: texture_2d<f32>;
@group(1) @binding(3) var map_sampler: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let d = (in.uv - vec2(0.5)) * 2.0;
    let r = length(d);
    if r > 1.0 {
        discard;
    }
    // Screen up is the heading; screen right is the heading's right (game xy, Z up).
    let fwd = vec2(params.z, params.w);
    let right = vec2(fwd.y, -fwd.x);
    let world = -d.y * fwd + d.x * right;
    // The overview's rows run north to south (world y decreasing).
    let uv = params.xy + vec2(world.x, -world.y) * extent.x;
    var c = textureSample(map, map_sampler, uv);
    if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
        c = vec4(0.0);
    }
    // The view cone: the map is lit inside the player's forward wedge (about 40 degrees each
    // side, as the game's minimap draws it) and dimmed outside it.
    let ang = atan2(d.x, -d.y);
    let cone = 1.0 - smoothstep(0.62, 0.72, abs(ang));
    let lit = mix(0.45, 1.6, cone);
    let disc = vec4(0.0, 0.0, 0.0, 0.7);
    let col = mix(disc.rgb, c.rgb * lit, c.a * extent.y);
    let a = max(disc.a, c.a * extent.y);
    let edge = 1.0 - smoothstep(0.97, 1.0, r);
    return vec4(col, a * edge);
}
