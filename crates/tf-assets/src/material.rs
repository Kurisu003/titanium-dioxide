//! `matl` (version 12) material assets.

use crate::rpak::{Asset, Rpak};
use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Material {
    pub guid: u64,
    pub name: String,
    pub surface_prop: String,
    pub shader_set: u64,
    /// Texture GUIDs by shader slot (0 = none).
    pub textures: Vec<u64>,
    pub glue_flags: u32,
    pub glue_flags2: u32,
    /// GUID of the material used for the depth prepass (alpha-tested materials have their
    /// own, which samples the colour texture).
    pub depth_prepass: u64,
    /// D3D state of the main pass: blend state mask, depth-stencil flags, rasterizer flags.
    pub blend_mask: u32,
    pub depth_flags: u16,
    pub raster_flags: u16,
    /// The shader constants from the material's CPU data, if present.
    pub params: Option<ShaderParams>,
}

/// The start of a v12 material's shader constant buffer (its CPU data), as read from the game's
/// materials: three UV transforms, then distortion, tints and fades. Checked on
/// `world\atmosphere\godray_500_fade_scroll`, whose distance fade (scale 0.0021, bias -0.0638)
/// reaches full opacity at 500 units, as its name says, and whose UV1 translate (0.01, -0.2)
/// is its scroll speed (the shader set's `Uv1at` flag animates it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaderParams {
    /// UV transforms: [m00, m01, m10, m11, translate u, translate v].
    pub uv: [[f32; 6]; 3],
    pub uv_distortion: [f32; 2],
    pub albedo_tint: [f32; 3],
    pub opacity: f32,
    /// Alpha edge fade: exponent, inner, outer (by |N·V|).
    pub edge_fade: [f32; 3],
    /// Alpha distance fade: alpha *= saturate(distance * scale + bias).
    pub distance_fade: [f32; 2],
    pub emissive_tint: [f32; 3],
}

impl ShaderParams {
    fn parse(f: impl Fn(usize) -> f32) -> Self {
        Self {
            uv: std::array::from_fn(|t| std::array::from_fn(|k| f(t * 6 + k))),
            uv_distortion: [f(18), f(19)],
            albedo_tint: [f(24), f(25), f(26)],
            opacity: f(27),
            edge_fade: [f(29), f(30), f(31)],
            distance_fade: [f(36), f(37)],
            emissive_tint: [f(40), f(41), f(42)],
        }
    }
}

/// A shader set's name (`shds` header +0x8), which spells out the shader's features, e.g.
/// `uberUnlitVcoltVcolaAdfAefAddNoCocNoTsaaUv1atUV21Samp2_wld`.
pub fn shader_set_name(pak: &Rpak, guid: u64) -> Option<String> {
    let a = pak.asset(guid)?;
    if &a.kind != b"shds" || a.head? + 0x10 > pak.data.len() {
        return None;
    }
    pak.ptr(a.head? + 0x8).filter(|&p| p < pak.data.len()).map(|p| pak.cstr(p))
}

/// The UV transform each texture slot samples with, from the `UV<n><slot digits>` part of a
/// shader set name (`...Uv1atUV21Samp2_wld`: 2 UV sets, texture 0 uses transform 1;
/// `UV2000001`: textures 0-4 untransformed, texture 5 through transform 1). 0 means the plain
/// mesh UVs.
pub fn uv_slots(shader: &str) -> Vec<u8> {
    let Some(i) = shader.find("UV").filter(|&i| shader[i + 2..].starts_with(|c: char| c.is_ascii_digit())) else { return Vec::new() };
    let digits: Vec<u8> = shader[i + 2..].chars().take_while(|c| c.is_ascii_digit()).map(|c| c as u8 - b'0').collect();
    digits.get(1..).map(|d| d.to_vec()).unwrap_or_default()
}

/// The feature flags in a shader set name, split at capitals (`Unlit`, `Add`, `Trans`, `Vcolt`,
/// `Vcola`, `Adf`, `Aef`, `Uv1at`, ...).
pub fn shader_flags(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl Material {
    /// Rasterizer flags without back-face culling mean the material is two-sided.
    pub fn two_sided(&self) -> bool {
        self.raster_flags & 0x4 == 0
    }
    pub fn blended(&self) -> bool {
        self.blend_mask & 0x1 != 0
    }
}

pub fn load(pak: &Rpak, a: &Asset) -> Result<Material> {
    let h = a.head.context("material has no header")?;
    let tex = pak.ptr(h + 0x98);
    let stream = pak.ptr(h + 0xA0);
    let mut textures = Vec::new();
    if let (Some(t), Some(s)) = (tex, stream) {
        // The streaming handle table directly follows the texture GUID table.
        let n = s.saturating_sub(t) / 8;
        for i in 0..n.min(64) {
            textures.push(pak.u64(t + i * 8));
        }
    }
    Ok(Material {
        guid: pak.u64(h + 0x10),
        name: pak.ptr(h + 0x18).map(|p| pak.cstr(p)).unwrap_or_default(),
        surface_prop: pak.ptr(h + 0x20).map(|p| pak.cstr(p)).unwrap_or_default(),
        shader_set: pak.u64(h + 0x90),
        textures,
        glue_flags: pak.u32(h + 0xC0),
        glue_flags2: pak.u32(h + 0xC4),
        depth_prepass: pak.u64(h + 0x38),
        blend_mask: pak.u32(h + 0x50 + 16),
        depth_flags: pak.u16(h + 0x50 + 20),
        raster_flags: pak.u16(h + 0x50 + 22),
        params: a.cpu.and_then(|c| {
            // MaterialCPUHeader: data pointer, then its size.
            if c + 12 > pak.data.len() {
                return None;
            }
            let p = pak.ptr(c)?;
            (pak.u32(c + 8) >= 0xB0 && p + 0xB0 <= pak.data.len()).then(|| ShaderParams::parse(|i| f32::from_bits(pak.u32(p + i * 4))))
        }),
    })
}
