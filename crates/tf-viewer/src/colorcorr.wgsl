// Source colour correction: a 32x32x32 lookup table (`materials/correction/*.raw`, red
// fastest) applied to the gamma-encoded picture, blended in by a weight. The picture after
// tonemapping is linear, so it is encoded to sRGB for the lookup and decoded after.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Settings {
    weight: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var lut: texture_3d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;
@group(0) @binding(4) var<uniform> settings: Settings;

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSample(source, source_sampler, in.uv);
    let g = clamp(to_srgb(max(src.rgb, vec3(0.0))), vec3(0.0), vec3(1.0));
    // Texel centres: 0 and 1 land on the first and last entries.
    let at = g * (31.0 / 32.0) + vec3(0.5 / 32.0);
    let corrected = to_linear(textureSampleLevel(lut, lut_sampler, at, 0.0).rgb);
    return vec4(mix(src.rgb, corrected, settings.weight), src.a);
}
