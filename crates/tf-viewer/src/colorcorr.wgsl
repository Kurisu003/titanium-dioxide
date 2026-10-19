// The game's auto exposure, tonemapping and colour correction (see colorcorr.rs), on the
// linear picture (pre-exposure, in the game's lighting units).
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Settings {
    weight: f32,
    exposure_min: f32,
    exposure_max: f32,
    compensation: f32,
    rate: f32,
    metering: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var lut: texture_3d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;
@group(0) @binding(4) var<uniform> settings: Settings;
// The tonemap pass reads this frame's adapted exposure, the exposure pass the last frame's.
@group(0) @binding(5) var exposure_tex: texture_2d<f32>;

const LUMA: vec3<f32> = vec3(0.299, 0.587, 0.114);
const GRID: vec2<u32> = vec2(64u, 36u);

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

// logluminance_ps40 + exposure_adaptation_ps40, into a 1x1 target.
@fragment
fn exposure(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let ratio = clamp(settings.metering, 0.1, 1.0);
    var sum = 0.0;
    for (var y = 0u; y < GRID.y; y++) {
        for (var x = 0u; x < GRID.x; x++) {
            let uv = ((vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(GRID) * 2.0 - 1.0) * ratio
                * 0.5 + 0.5;
            let c = textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
            sum += log2(max(dot(c, LUMA), 0.001));
        }
    }
    let l = exp2(sum / f32(GRID.x * GRID.y));
    let key = 1.03 - 2.0 / (log2(l + 1.0002) * 0.6931472 + 2.0);
    let target_exposure = key / l * exp2(settings.compensation);
    let prev = textureLoad(exposure_tex, vec2(0, 0), 0).r;
    var e = target_exposure;
    if prev > 0.0 {
        let d = target_exposure - prev;
        let step = min(abs(d) / max(target_exposure, prev) * settings.rate, abs(d));
        e = prev + sign(d) * step;
    }
    if e != e {
        e = 0.0;
    }
    return vec4(clamp(e, settings.exposure_min, settings.exposure_max), 0.0, 0.0, 1.0);
}

// engine_post_ps50's tonemap and colour correction.
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSample(source, source_sampler, in.uv);
    let x = max(src.rgb, vec3(0.0)) * textureLoad(exposure_tex, vec2(0, 0), 0).r;
    let c = x * (10.0 * x + 0.3) / (x * (10.0 * x + 0.5) + 1.5);
    // Texel centres: 0 and 1 land on the first and last entries.
    let at = to_srgb(c) * (31.0 / 32.0) + vec3(0.5 / 32.0);
    let corrected = textureSampleLevel(lut, lut_sampler, at, 0.0).rgb;
    return vec4(mix(c, corrected, settings.weight), src.a);
}
