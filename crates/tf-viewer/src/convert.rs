//! Turning game assets into Bevy assets: textures, materials and meshes.

use crate::gamedata::GameData;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use tf_assets::texture::{self, TexFormat};

/// Caches so shared textures/materials are only uploaded once.
#[derive(Default, Resource)]
pub struct Cache {
    pub images: HashMap<u64, Option<Handle<Image>>>,
    pub materials: HashMap<String, Handle<StandardMaterial>>,
    pub missing_materials: Vec<String>,
    pub actors: HashMap<String, crate::actor::ActorTemplate>,
    /// Materials whose shader needs the effect path (uber.rs swaps them in on the meshes).
    pub uber: HashMap<AssetId<StandardMaterial>, crate::uber::UberInfo>,
}

pub fn wgpu_format(f: TexFormat) -> Option<TextureFormat> {
    use TexFormat::*;
    Some(match f {
        Bc1 => TextureFormat::Bc1RgbaUnorm,
        Bc1Srgb => TextureFormat::Bc1RgbaUnormSrgb,
        Bc2 => TextureFormat::Bc2RgbaUnorm,
        Bc2Srgb => TextureFormat::Bc2RgbaUnormSrgb,
        Bc3 => TextureFormat::Bc3RgbaUnorm,
        Bc3Srgb => TextureFormat::Bc3RgbaUnormSrgb,
        Bc4 => TextureFormat::Bc4RUnorm,
        Bc4Snorm => TextureFormat::Bc4RSnorm,
        Bc5 => TextureFormat::Bc5RgUnorm,
        Bc5Snorm => TextureFormat::Bc5RgSnorm,
        Bc6hUf16 => TextureFormat::Bc6hRgbUfloat,
        Bc6hSf16 => TextureFormat::Bc6hRgbFloat,
        Bc7 => TextureFormat::Bc7RgbaUnorm,
        Bc7Srgb => TextureFormat::Bc7RgbaUnormSrgb,
        Rgba16Float => TextureFormat::Rgba16Float,
        Rgba8 => TextureFormat::Rgba8Unorm,
        Rgba8Srgb => TextureFormat::Rgba8UnormSrgb,
        R8 => TextureFormat::R8Unorm,
        Other(_) => return None,
    })
}

pub fn sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    }
}

/// Load texture `guid` (searching all paks, preferring `pak_hint`) into a Bevy image.
/// Time spent loading textures and building spec/gloss maps (microseconds), for load profiling.
pub static TEX_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SPEC_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn image(gd: &GameData, cache: &mut Cache, images: &mut Assets<Image>, pak_hint: usize, guid: u64) -> Option<Handle<Image>> {
    if let Some(h) = cache.images.get(&guid) {
        return h.clone();
    }
    let t0 = std::time::Instant::now();
    let _timer = scopeguard(move || { TEX_US.fetch_add(t0.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed); });
    let result = (|| {
        let (pi, ai) = gd.find_texture(pak_hint, guid)?;
        let pak = &gd.paks[pi];
        let tex = texture::load(pak, &pak.assets[ai]).ok()?;
        let format = wgpu_format(tex.info.format)?;
        let (_, bw, bh) = tex.info.format.block()?;
        // Compressed textures need the top level to be whole blocks.
        if tex.width as usize % bw != 0 || tex.height as usize % bh != 0 {
            return None;
        }
        let mut img = Image::default();
        img.data = Some(tex.data);
        img.texture_descriptor.size = Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 };
        img.texture_descriptor.mip_level_count = tex.mips;
        img.texture_descriptor.format = format;
        img.texture_descriptor.dimension = TextureDimension::D2;
        // The app default sampler is `sampler()`; sharing it (rather than an identical
        // descriptor per image) keeps bindless material slabs small.
        img.sampler = ImageSampler::Default;
        img.asset_usage = RenderAssetUsages::RENDER_WORLD;
        Some(images.add(img))
    })();
    cache.images.insert(guid, result.clone());
    result
}

/// Cache key salt for the combined spec/gloss images.
const SPEC_GLOSS_KEY: u64 = 0x2000_6E55_0000_0002;
const COLOR_OPA_KEY: u64 = 0x2000_6E55_0000_0003;

/// Decode the top level of a gloss (BC4) or spec (BC1) map at most 256 texels across.
fn small_texture(gd: &GameData, pak_hint: usize, guid: u64) -> Option<(usize, usize, Vec<[u8; 3]>)> {
    let (pi, ai) = gd.find_texture(pak_hint, guid)?;
    let pak = &gd.paks[pi];
    let tex = texture::load_max(pak, &pak.assets[ai], 256).ok()?;
    let (w, h) = (tex.width as usize, tex.height as usize);
    let px = match tex.info.format {
        TexFormat::Bc4 => texture::decode_bc4(&tex.data, w, h).into_iter().map(|g| [g; 3]).collect(),
        TexFormat::Bc1 | TexFormat::Bc1Srgb => texture::decode_bc1(&tex.data, w, h),
        _ => return None,
    };
    Some((w, h, px))
}

/// A colour map with an opacity mask (`_opa`, Bc4) in its alpha, at most 256 texels across,
/// with box-filtered mips (sRGB).
fn color_opacity_image(gd: &GameData, cache: &mut Cache, images: &mut Assets<Image>, pak_hint: usize, col: u64, opa: u64) -> Option<Handle<Image>> {
    let key = col.rotate_left(29) ^ opa ^ COLOR_OPA_KEY;
    if let Some(h) = cache.images.get(&key) {
        return h.clone();
    }
    let result = (|| {
        let c = small_texture(gd, pak_hint, col)?;
        let o = small_texture(gd, pak_hint, opa)?;
        let (w, h) = (c.0, c.1);
        let at = |t: &(usize, usize, Vec<[u8; 3]>), x: usize, y: usize| t.2[(y * t.1 / h).min(t.1 - 1) * t.0 + (x * t.0 / w).min(t.0 - 1)];
        let mut level: Vec<[u8; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let rgb = at(&c, x, y);
                [rgb[0], rgb[1], rgb[2], at(&o, x, y)[0]]
            })
            .collect();
        let (mut lw, mut lh) = (w, h);
        let mut data = Vec::new();
        let mut mips = 0;
        loop {
            data.extend(level.iter().flatten());
            mips += 1;
            if lw == 1 && lh == 1 {
                break;
            }
            let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
            let px = |x: usize, y: usize| level[y.min(lh - 1) * lw + x.min(lw - 1)];
            level = (0..nh * nw)
                .map(|i| {
                    let (x, y) = (i % nw * 2, i / nw * 2);
                    let q = [px(x, y), px(x + 1, y), px(x, y + 1), px(x + 1, y + 1)];
                    std::array::from_fn(|ch| ((q.iter().map(|p| p[ch] as u32).sum::<u32>() + 2) / 4) as u8)
                })
                .collect();
            (lw, lh) = (nw, nh);
        }
        let mut img = Image::default();
        img.data = Some(data);
        img.texture_descriptor.size = Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 };
        img.texture_descriptor.mip_level_count = mips;
        img.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
        img.texture_descriptor.dimension = TextureDimension::D2;
        img.sampler = ImageSampler::Default;
        img.asset_usage = RenderAssetUsages::RENDER_WORLD;
        Some(images.add(img))
    })();
    cache.images.insert(key, result.clone());
    result
}

/// Respawn's materials are specular/gloss. Both maps go into one texture, used as the
/// metallic-roughness map (green: roughness = 1 - gloss, blue: metallic 0) and as the specular
/// map (alpha: the spec map's luminance as stored, sRGB-encoded). With reflectance 5, Bevy's
/// F0 = 0.16 * (5 * a * 0.5)^2 = a^2, about the linear specular colour's luminance. One texture
/// instead of two keeps the bindless material slabs small. At most 256 texels across, with
/// box-filtered mips.
fn spec_gloss_image(gd: &GameData, cache: &mut Cache, images: &mut Assets<Image>, pak_hint: usize, gloss: Option<u64>, spec: Option<u64>) -> Option<Handle<Image>> {
    let key = gloss.unwrap_or(0).rotate_left(17) ^ spec.unwrap_or(0) ^ SPEC_GLOSS_KEY;
    if let Some(h) = cache.images.get(&key) {
        return h.clone();
    }
    let t0 = std::time::Instant::now();
    let _timer = scopeguard(move || { SPEC_US.fetch_add(t0.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed); });
    let result = (|| {
        let g = gloss.and_then(|g| small_texture(gd, pak_hint, g));
        let s = spec.and_then(|s| small_texture(gd, pak_hint, s));
        if g.is_none() && s.is_none() {
            return None;
        }
        let (w, h) = g.as_ref().or(s.as_ref()).map(|t| (t.0, t.1))?;
        // Sample either map at the chosen size (nearest).
        let at = |t: &Option<(usize, usize, Vec<[u8; 3]>)>, x: usize, y: usize| t.as_ref().map(|(tw, th, px)| px[(y * th / h).min(th - 1) * tw + (x * tw / w).min(tw - 1)]);
        let mut level: Vec<[u8; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let rough = at(&g, x, y).map(|v| 255 - v[0]).unwrap_or(153);
                // A dielectric's 0.04 F0 when there's no spec map.
                let a = at(&s, x, y).map(|v| ((v[0] as u32 * 54 + v[1] as u32 * 183 + v[2] as u32 * 19) >> 8) as u8).unwrap_or(51);
                [0, rough, 0, a]
            })
            .collect();
        let (mut lw, mut lh) = (w, h);
        let mut data = Vec::new();
        let mut mips = 0;
        loop {
            data.extend(level.iter().flatten());
            mips += 1;
            if lw == 1 && lh == 1 {
                break;
            }
            let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
            let px = |x: usize, y: usize| level[y.min(lh - 1) * lw + x.min(lw - 1)];
            level = (0..nh * nw)
                .map(|i| {
                    let (x, y) = (i % nw * 2, i / nw * 2);
                    let q = [px(x, y), px(x + 1, y), px(x, y + 1), px(x + 1, y + 1)];
                    std::array::from_fn(|c| ((q.iter().map(|p| p[c] as u32).sum::<u32>() + 2) / 4) as u8)
                })
                .collect();
            (lw, lh) = (nw, nh);
        }
        let mut img = Image::default();
        img.data = Some(data);
        img.texture_descriptor.size = Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 };
        img.texture_descriptor.mip_level_count = mips;
        img.texture_descriptor.format = TextureFormat::Rgba8Unorm;
        img.texture_descriptor.dimension = TextureDimension::D2;
        // The app default sampler is `sampler()`; sharing it (rather than an identical
        // descriptor per image) keeps bindless material slabs small.
        img.sampler = ImageSampler::Default;
        img.asset_usage = RenderAssetUsages::RENDER_WORLD;
        Some(images.add(img))
    })();
    cache.images.insert(key, result.clone());
    result
}

/// Build (or fetch) a StandardMaterial for a game material name.
/// Self-illumination (`_ilm`) at full strength, in multiples of a white surface's brightness.
const ILM_WHITE: f32 = 2.0;

pub fn material(
    gd: &GameData,
    cache: &mut Cache,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    name: &str,
    blend: bool,
) -> Handle<StandardMaterial> {
    let key = format!("{}|{}", crate::gamedata::normalize_name(name), blend);
    if let Some(h) = cache.materials.get(&key) {
        return h.clone();
    }
    let mut mat = StandardMaterial {
        perceptual_roughness: 0.6,
        reflectance: 0.3,
        flip_normal_map_y: true,
        lightmap_exposure: crate::env::LIGHTMAP_EXPOSURE,
        ..default()
    };
    // TF_NO_SPEC: debug toggle, the flat roughness/reflectance used before gloss/spec maps.
    let no_spec = std::env::var_os("TF_NO_SPEC").is_some();
    match gd.material(name) {
        Some((pi, m)) => {
            let (mut gloss, mut spec, mut opa, mut col) = (None, None, None, None);
            for (slot, &guid) in m.textures.iter().enumerate() {
                if guid == 0 {
                    continue;
                }
                let tex_name = gd
                    .find_texture(pi, guid)
                    .and_then(|(p, a)| texture::info(&gd.paks[p], &gd.paks[p].assets[a]).ok())
                    .and_then(|i| i.name)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                // Prefer the name suffix; otherwise use the usual slot order of model/world shaders.
                let suffix = tex_name.rsplit('_').next().unwrap_or("");
                let kind = match suffix {
                    "col" | "nml" | "gls" | "exp" | "spc" | "ilm" | "ao" | "cav" | "opa" => suffix,
                    _ => ["col", "nml", "gls", "spc"].get(slot).copied().unwrap_or(""),
                };
                match kind {
                    "col" if mat.base_color_texture.is_none() => {
                        mat.base_color_texture = image(gd, cache, images, pi, guid);
                        col = Some(guid);
                    }
                    "opa" if opa.is_none() => opa = Some(guid),
                    "nml" if mat.normal_map_texture.is_none() => mat.normal_map_texture = image(gd, cache, images, pi, guid),
                    "ilm" if mat.emissive_texture.is_none() && std::env::var_os("TF_NO_ILM").is_none() => {
                        mat.emissive_texture = image(gd, cache, images, pi, guid);
                        if mat.emissive_texture.is_some() {
                            // Full self-illumination at twice a sunlit white surface, through the
                            // camera's exposure (Bevy leaves emissive unexposed by default, which
                            // made faint baked glow, like the cockpit's screen bounce, shine).
                            let k = crate::env::nits_for_white() * ILM_WHITE;
                            mat.emissive = LinearRgba::rgb(k, k, k);
                            mat.emissive_exposure_weight = 1.0;
                        }
                    }
                    "ao" if mat.occlusion_texture.is_none() => mat.occlusion_texture = image(gd, cache, images, pi, guid),
                    "gls" | "exp" if gloss.is_none() => gloss = Some(guid),
                    "spc" if spec.is_none() => spec = Some(guid),
                    _ => {}
                }
            }
            if !no_spec {
                if let Some(sg) = spec_gloss_image(gd, cache, images, pi, gloss, spec) {
                    mat.metallic_roughness_texture = Some(sg.clone());
                    mat.perceptual_roughness = 1.0;
                    mat.metallic = 0.0;
                    mat.specular_texture = Some(sg);
                    mat.reflectance = 5.0;
                }
            }
            let alpha_tested = gd.material_by_guid(m.depth_prepass).is_some_and(|d| d.textures.iter().any(|&t| t != 0));
            // An opacity mask (`Opam` shaders: glass panes, sight lenses) goes into the base
            // colour's alpha, which the blend reads.
            let opacity_masked = opa.is_some() && (blend || m.blended() || tf_assets::material::shader_set_name(&gd.paks[pi], m.shader_set).is_some_and(|n| n.contains("Opam")));
            if opacity_masked {
                if let (Some(c), Some(o)) = (col, opa) {
                    if let Some(img) = color_opacity_image(gd, cache, images, pi, c, o) {
                        mat.base_color_texture = Some(img);
                    }
                }
            }
            // Unlit translucent shaders (scope reticles, sight glows): blended, unlit, both sides.
            let shader = tf_assets::material::shader_set_name(&gd.paks[pi], m.shader_set).unwrap_or_default();
            let unlit_trans = shader.contains("Unlit") && shader.contains("Trans");
            if unlit_trans {
                mat.unlit = true;
                mat.double_sided = true;
                mat.cull_mode = None;
            }
            if blend || m.blended() || opacity_masked || unlit_trans {
                mat.alpha_mode = AlphaMode::Blend;
            } else if alpha_tested {
                mat.alpha_mode = AlphaMode::Mask(0.5);
            }
            // Decals and blended overlays lie flush on other surfaces; nudge them towards the
            // camera so they don't z-fight (in depth ULPs: tiny, but enough for coplanar faces).
            let lname = name.to_ascii_lowercase();
            if blend || m.blended() || lname.contains("decal") || lname.contains("overlay") {
                mat.depth_bias = 256.0;
                // Decal gloss/spec maps don't follow the world materials' reading (painted
                // letters turned into grey mirrors), so overlays keep a plain rough dielectric.
                mat.specular_texture = None;
                mat.metallic_roughness_texture = None;
                mat.perceptual_roughness = 0.6;
                mat.reflectance = 0.5;
            }
            if m.two_sided() || alpha_tested {
                mat.double_sided = true;
                mat.cull_mode = None;
            }
            if mat.base_color_texture.is_none() {
                let names: Vec<String> = m
                    .textures
                    .iter()
                    .filter(|&&g| g != 0)
                    .map(|&g| match gd.find_texture(pi, g) {
                        Some((p, a)) => texture::info(&gd.paks[p], &gd.paks[p].assets[a])
                            .map(|i| format!("{:?} {:?}", i.name, i.format))
                            .unwrap_or_default(),
                        None => format!("{g:016x} not loaded"),
                    })
                    .collect();
                log::debug!("material {name} has no color texture: {names:?}");
            }
        }
        None => {
            // Materials whose header sits in a patch's older pages: their textures may still be
            // readable by name (`<material>_col`, `_nml`).
            let by_name = |suffix: &str| gd.texture_by_name(&format!("{name}_{suffix}"));
            match by_name("col") {
                Some((p, g)) => {
                    mat.base_color_texture = image(gd, cache, images, p, g);
                    if let Some((p, g)) = by_name("nml") {
                        mat.normal_map_texture = image(gd, cache, images, p, g);
                    }
                    log::debug!("material {name}: textures by name from the patch paks");
                }
                None => {
                    cache.missing_materials.push(name.to_string());
                    mat.base_color = Color::srgb(0.5, 0.5, 0.5);
                }
            }
        }
    }
    // TF_MIRROR: debug view with every surface a mirror, for checking the cubemaps' orientation.
    if std::env::var_os("TF_MIRROR").is_some() {
        mat.base_color_texture = None;
        mat.base_color = Color::WHITE;
        mat.metallic = 1.0;
        mat.perceptual_roughness = 0.08;
        mat.metallic_roughness_texture = None;
    }
    let h = materials.add(mat);
    if let Some(info) = crate::uber::info(gd, name) {
        cache.uber.insert(h.id(), info);
    }
    cache.materials.insert(key, h.clone());
    h
}

pub fn build_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    skin: Option<(Vec<[u16; 4]>, Vec<[f32; 4]>)>,
) -> Mesh {
    // The game uses clockwise front faces (Direct3D); wgpu culls those by default.
    let mut indices = indices;
    for tri in indices.chunks_exact_mut(3) {
        tri.swap(1, 2);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices));
    if let Some((joints, weights)) = skin {
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    }
    if let Err(e) = mesh.generate_tangents() {
        log::debug!("tangent generation failed: {e}");
    }
    mesh
}

/// Second layer of a two-layer (`_bm`) world material.
#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct BlendLayer {
    #[texture(100)]
    #[sampler(101)]
    pub layer_b: Option<Handle<Image>>,
    #[texture(102)]
    #[sampler(103)]
    pub mask: Option<Handle<Image>>,
}

impl bevy::pbr::MaterialExtension for BlendLayer {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/blend.wgsl".into()
    }
    fn deferred_fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/blend.wgsl".into()
    }
}

pub type BlendMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, BlendLayer>;

/// For a `_bm` material: the base material plus layer B's colour (slot 23) and the blend
/// mask (slot 22). None if it isn't a two-layer material.
pub fn blend_material(
    gd: &GameData,
    cache: &mut Cache,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    blends: &mut Assets<BlendMaterial>,
    name: &str,
) -> Option<Handle<BlendMaterial>> {
    let (pi, m) = gd.material(name)?;
    let mask = *m.textures.get(22).filter(|&&g| g != 0)?;
    let col_b = *m.textures.get(23).filter(|&&g| g != 0)?;
    let base = material(gd, cache, images, materials, name, false);
    let base = materials.get(&base)?.clone();
    Some(blends.add(BlendMaterial { base, extension: BlendLayer { layer_b: image(gd, cache, images, pi, col_b), mask: image(gd, cache, images, pi, mask) } }))
}

pub struct BlendMaterialPlugin;

impl Plugin for BlendMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "blend.wgsl");
        app.add_plugins(MaterialPlugin::<BlendMaterial>::default());
    }
}

struct Guard<F: FnMut()>(F);
impl<F: FnMut()> Drop for Guard<F> {
    fn drop(&mut self) {
        (self.0)()
    }
}
fn scopeguard<F: FnMut()>(f: F) -> Guard<F> {
    Guard(f)
}
