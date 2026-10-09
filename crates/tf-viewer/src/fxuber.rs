//! Effect models (shield domes and walls, the Thermal Shield's flame fan) drawn with the game's
//! own unlit uber pixel shader. The shader set name of an effect material spells out which
//! parts of the shader are compiled in (`uberUnlitVcoltVcolaEmitEmulOpamEntcolmeEntcolmdAefEef
//! AddUvd2Uv1dUv1atUv2dUv2atUV312210Samp22222`), and its CPU data holds the constants
//! (`tf_assets::material::ShaderParams`). fxuber.wgsl follows the compiled pixel shaders
//! (`shdr` assets in common.rpak, ps_5_0 DXBC; disassembled from the user's install, e.g. the
//! Vortex dome's 4bd6a6bd4aad037a and the Particle Wall's c8be5a00373d3dff), whose
//! reflection data names every constant:
//!
//! - three UV transforms (`c_uvNRotScaleX/Y`, `c_uvNTranslate`). `UvNat` animates the
//!   translate (times `c_gameTime`), `UvNs` takes the screen position (`SV_Position *
//!   c_rcpRenderTargetSize`) instead of the mesh UV as its input, `UvNd` adds the distortion
//!   offset scaled by the transform's diagonal. The digits after `UV<count>` give each texture's
//!   transform (0 the plain mesh UV) in the order albedo, emissive, emissive multiply, opacity
//!   multiply, detail, distortion 1, distortion 2 (checked on the Vortex dome, the flame fan,
//!   the hex shields and the wall backs; detail's place in the order is a guess, no shield
//!   material has both it and a multiply texture);
//! - `Uvd1`/`Uvd2`: two-channel offset maps (slots 18 and 19) read as `xy * 2 - 1` times
//!   `c_uvDistortionIntensity` / `c_uvDistortion2Intensity`;
//! - alpha: albedo texture alpha x vertex alpha x `c_opacity` x the instance alpha
//!   (`CBufModelInstance` row 6: the particle's alpha) x the opacity multiply texture (slot 13,
//!   red) x the alpha edge fade (`Aef`) x the alpha distance fade (`Adf`,
//!   `saturate(distance * scale + bias)`);
//! - edge fades: `max(lerp(inner, outer, pow(max(1 - |N.V|, 0), exponent)), 0)`: unlike the
//!   world path's fade (uber.rs) these grow toward the silhouette when `outer > inner` (the
//!   Particle Wall's emissive goes from 0.3 face-on to 20 edge-on);
//! - colour: albedo (texture x `c_albedoTint` x vertex colour (`Vcolt`; the vertex shader
//!   raises it to 2.2) x the instance colour (`Entcolmd`) x 2 x the detail texture (`Det`)),
//!   its brightest channel held to `c_maxLightingValue` (5: materialsystem_dx11.dll fills it
//!   from `mat_bloom_max_lighting_value`, 0x18001b947), plus emissive (texture x
//!   `c_emissiveTint` x the emissive multiply texture (slot 17) x the instance colour
//!   (`Entcolme`) x the emissive edge fade (`Eef`)). With `c_useAlphaModulateEmissive` at 0 or
//!   below the emissive is divided by the alpha first, so it is not faded by it;
//! - output premultiplied: `Add` blends ONE/ONE, `Trans` ONE/INV_SRC_ALPHA (the materials' DX
//!   blend state), depth tested but not written, back faces culled unless the material's
//!   rasterizer flags say two-sided.
//!
//! Not drawn: the height fog the shader applies (the lighting and atmosphere are left for
//! later), and its extra render targets (TSAA and depth of field masks).

use crate::gamedata::GameData;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};

/// `c_maxLightingValue`: `mat_bloom_max_lighting_value`'s default (materialsystem_dx11.dll
/// ConVar at 0x1819ac7b0, "5.0").
pub const MAX_LIGHTING_VALUE: f32 = 5.0;

pub const ALBEDO: u32 = 1;
pub const EMISSIVE: u32 = 2;
pub const EMUL: u32 = 4;
pub const OPAM: u32 = 8;
pub const DETAIL: u32 = 16;
pub const UVD1: u32 = 32;
pub const UVD2: u32 = 64;
pub const VCOLT: u32 = 128;
pub const VCOLA: u32 = 256;
pub const AEF: u32 = 512;
pub const EEF: u32 = 1024;
pub const ADF: u32 = 2048;
/// Additive (the shader still outputs premultiplied colour; alpha 0 makes Bevy's premultiplied
/// blend ONE/ONE).
pub const ADD: u32 = 4096;

/// Per transform: bit 0 animated translate, bit 1 screen-space input, bit 2 distorted.
const UV_ANIM: u32 = 1;
const UV_SCREEN: u32 = 2;
const UV_DIST: u32 = 4;

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct FxUberParams {
    /// Each UV transform's rotation/scale: u' = dot(uv, xy), v' = dot(uv, zw).
    pub uv_rs: [Vec4; 3],
    /// Each UV transform's translate (or scroll speed when animated).
    pub uv_t: [Vec4; 3],
    /// Distortion 1 intensity (xy), distortion 2 intensity (zw).
    pub distort: Vec4,
    /// Albedo tint, opacity.
    pub albedo: Vec4,
    /// Alpha edge fade exponent, inner, outer; alpha distance fade scale in w.
    pub aef: Vec4,
    /// Emissive edge fade exponent, inner, outer; alpha distance fade bias in w.
    pub eef: Vec4,
    /// Emissive tint; w: game units per world unit.
    pub emissive: Vec4,
    /// `c_useAlphaModulateSpecular`, `c_useAlphaModulateEmissive`, `c_maxLightingValue`.
    pub misc: Vec4,
    /// The instance's colour and alpha (the particle's), `CBufModelInstance` row 6.
    pub modulation: Vec4,
    /// Texture transforms: albedo, emissive, emissive multiply, opacity multiply.
    pub tex_uv: UVec4,
    /// Texture transforms: detail, distortion 1, distortion 2; flags in w.
    pub tex_uv2: UVec4,
    /// Per transform UV_* bits.
    pub uv_flags: UVec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(FxUberKey)]
pub struct FxUberMaterial {
    #[uniform(0)]
    pub params: FxUberParams,
    #[texture(1)]
    #[sampler(2)]
    pub albedo: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub emissive: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    pub emissive_mul: Option<Handle<Image>>,
    #[texture(7)]
    #[sampler(8)]
    pub opacity_mul: Option<Handle<Image>>,
    #[texture(9)]
    #[sampler(10)]
    pub detail: Option<Handle<Image>>,
    #[texture(11)]
    #[sampler(12)]
    pub distort1: Option<Handle<Image>>,
    #[texture(13)]
    #[sampler(14)]
    pub distort2: Option<Handle<Image>>,
    pub two_sided: bool,
    pub additive: bool,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct FxUberKey {
    two_sided: bool,
}

impl From<&FxUberMaterial> for FxUberKey {
    fn from(m: &FxUberMaterial) -> Self {
        FxUberKey { two_sided: m.two_sided }
    }
}

impl Material for FxUberMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/fxuber.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        if self.additive { AlphaMode::Add } else { AlphaMode::Premultiplied }
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_pipeline: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _layout: &MeshVertexBufferLayoutRef, key: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.two_sided { None } else { Some(Face::Back) };
        Ok(())
    }
}

pub struct FxUberPlugin;

impl Plugin for FxUberPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "fxuber.wgsl");
        app.add_plugins(MaterialPlugin::<FxUberMaterial>::default());
    }
}

/// Texture slots in the order the shader set name's UV digits list them.
const SLOTS: [(usize, u32); 7] = [(0, ALBEDO), (4, EMISSIVE), (17, EMUL), (13, OPAM), (14, DETAIL), (18, UVD1), (19, UVD2)];

/// The game's unlit uber material `name` as an effect material, or None when it isn't one
/// (lit, or no constants). `two_sided` comes from the material's rasterizer flags.
pub fn material(gd: &GameData, cache: &mut crate::convert::Cache, images: &mut Assets<Image>, name: &str) -> Option<FxUberMaterial> {
    let (pi, m) = gd.material(name)?;
    let p = m.params?;
    let shader = gd.shader_set_name(m.shader_set)?;
    if !shader.starts_with("uber") {
        return None;
    }
    let tokens = tf_assets::material::shader_flags(&shader);
    let has = |t: &str| tokens.iter().any(|x| x == t);
    if !has("Unlit") || !(has("Add") || has("Trans")) {
        return None;
    }
    let mut flags = 0;
    for (t, f) in [("Vcolt", VCOLT), ("Vcola", VCOLA), ("Aef", AEF), ("Eef", EEF), ("Adf", ADF), ("Add", ADD)] {
        if has(t) {
            flags |= f;
        }
    }
    // Which textures the shader samples: the ones its name compiles in.
    let present = |f: u32| match f {
        ALBEDO => true,
        EMISSIVE => has("Emit"),
        EMUL => has("Emul"),
        OPAM => has("Opam"),
        DETAIL => has("Det"),
        UVD1 => has("Uvd1"),
        UVD2 => has("Uvd2"),
        _ => false,
    };
    let digits = tf_assets::material::uv_slots(&shader);
    let mut tex_uv = [0u32; 7];
    let mut next = 0;
    let mut textures: [Option<Handle<Image>>; 7] = Default::default();
    for (k, &(slot, f)) in SLOTS.iter().enumerate() {
        if !present(f) {
            continue;
        }
        tex_uv[k] = digits.get(next).copied().unwrap_or(0) as u32;
        next += 1;
        textures[k] = m.textures.get(slot).filter(|&&g| g != 0).and_then(|&g| crate::convert::image(gd, cache, images, pi, g));
        if textures[k].is_some() {
            flags |= f;
        }
    }
    let mut uv_flags = [0u32; 4];
    for (t, uf) in uv_flags.iter_mut().take(3).enumerate() {
        let n = t + 1;
        for (suffix, bit) in [("at", UV_ANIM), ("s", UV_SCREEN), ("d", UV_DIST)] {
            if has(&format!("Uv{n}{suffix}")) {
                *uf |= bit;
            }
        }
    }
    let uv = |t: usize| Vec4::new(p.uv[t][0], p.uv[t][1], p.uv[t][2], p.uv[t][3]);
    let uvt = |t: usize| Vec4::new(p.uv[t][4], p.uv[t][5], 0.0, 0.0);
    let params = FxUberParams {
        uv_rs: [uv(0), uv(1), uv(2)],
        uv_t: [uvt(0), uvt(1), uvt(2)],
        distort: Vec4::new(p.uv_distortion[0], p.uv_distortion[1], p.uv_distortion2[0], p.uv_distortion2[1]),
        albedo: Vec4::new(p.albedo_tint[0], p.albedo_tint[1], p.albedo_tint[2], p.opacity),
        aef: Vec4::new(p.edge_fade[0], p.edge_fade[1], p.edge_fade[2], p.distance_fade[0]),
        eef: Vec4::new(p.emissive_edge_fade[0], p.emissive_edge_fade[1], p.emissive_edge_fade[2], p.distance_fade[1]),
        emissive: Vec4::new(p.emissive_tint[0], p.emissive_tint[1], p.emissive_tint[2], 1.0 / crate::player::UNIT),
        misc: Vec4::new(p.alpha_modulate[0], p.alpha_modulate[1], MAX_LIGHTING_VALUE, 0.0),
        modulation: Vec4::ONE,
        tex_uv: UVec4::new(tex_uv[0], tex_uv[1], tex_uv[2], tex_uv[3]),
        tex_uv2: UVec4::new(tex_uv[4], tex_uv[5], tex_uv[6], flags),
        uv_flags: UVec4::from_array(uv_flags),
    };
    log::debug!("effect model material {name}: {shader} flags {flags:#x} uv {tex_uv:?} uv flags {uv_flags:?}");
    let [albedo, emissive, emissive_mul, opacity_mul, detail, distort1, distort2] = textures;
    Some(FxUberMaterial { params, albedo, emissive, emissive_mul, opacity_mul, detail, distort1, distort2, two_sided: m.two_sided(), additive: flags & ADD != 0 })
}

/// The mesh's vertex colours as the game's vertex shader passes them on: RGB raised to 2.2,
/// alpha as is.
pub fn vertex_colors(colors: &[[u8; 4]]) -> Vec<[f32; 4]> {
    colors.iter().map(|c| [(c[0] as f32 / 255.0).powf(2.2), (c[1] as f32 / 255.0).powf(2.2), (c[2] as f32 / 255.0).powf(2.2), c[3] as f32 / 255.0]).collect()
}

/// The shield colour scripts pass in control point 1 (0-255 per channel): `_vortex.nut`'s
/// `GetTriLerpColor`, blue (115, 247, 255) at 0, orange (200, 128, 80) at 0.75 and red
/// (200, 80, 80) from 0.95. `frac` is how spent the shield is: the Vortex's charge fraction,
/// `1 - health fraction` for walls and the Gun Shield.
pub fn shield_color(frac: f32) -> Vec3 {
    const FULL: Vec3 = Vec3::new(115.0, 247.0, 255.0);
    const MED: Vec3 = Vec3::new(200.0, 128.0, 80.0);
    const EMPTY: Vec3 = Vec3::new(200.0, 80.0, 80.0);
    // VORTEX_SPHERE_COLOR_CROSSOVERFRAC_FULL2MED / _MED2EMPTY.
    const C1: f32 = 0.75;
    const C2: f32 = 0.95;
    if frac < C1 {
        FULL.lerp(MED, frac.max(0.0) / C1)
    } else if frac < C2 {
        MED.lerp(EMPTY, (frac - C1) / (C2 - C1))
    } else {
        EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shield_color_follows_the_script() {
        assert_eq!(shield_color(0.0), Vec3::new(115.0, 247.0, 255.0));
        assert_eq!(shield_color(0.75), Vec3::new(200.0, 128.0, 80.0));
        assert_eq!(shield_color(1.0), Vec3::new(200.0, 80.0, 80.0));
        let mid = shield_color(0.85);
        assert!((mid - Vec3::new(200.0, 104.0, 80.0)).length() < 1e-3);
    }

    #[test]
    fn vertex_colors_are_gamma_decoded() {
        let c = vertex_colors(&[[255, 128, 0, 128]]);
        assert!((c[0][0] - 1.0).abs() < 1e-6 && (c[0][1] - 0.2195).abs() < 1e-3 && c[0][2] == 0.0 && (c[0][3] - 0.502).abs() < 1e-3);
    }
}
