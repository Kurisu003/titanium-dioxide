//! The game's own particle systems (`particles/*.pcf`, parsed by `tf_assets::pcf`), simulated
//! and drawn here: emitters, initializers, operators, forces, constraints and renderers are
//! compiled from each system's operator list (with Source's defaults for missing parameters),
//! children start with their delays, and every live particle of a material goes into one
//! camera-facing mesh per frame.
//!
//! Systems run in game space (inches, Z up) with control points (CP0 = where the effect plays,
//! CP1 = e.g. a tracer's end). A control point's frame is Source's: X forward, Y left, Z up;
//! impacts use the surface normal as Z, muzzle flashes the shot direction as X.
//!
//! Supported:
//! - emitters: emit_instantaneously, emit_continuously, emit noise (its mean rate),
//!   emit over distance, emit_instantaneously by distance;
//! - initializers: lifetime/radius/alpha/colour/rotation/rotation speed/sequence/trail length
//!   random, scalar and vector random, remap noise to scalar, position within sphere/box,
//!   along ring, along path, offset and warp, velocity random/noise/inherit, move particles
//!   between two control points, lifetime from sequence, yaw flip;
//! - operators: movement basic, lifespan decay, radius scale, alpha fade and decay (and the
//!   tracer variant), alpha fade in/out (random and simple), colour fade, rotation spin roll,
//!   graph scalar, lerp initial scalar, lock to control point, max velocity, set control point
//!   positions;
//! - forces: pull towards control point, random, turbulent, twist around axis;
//! - constraints: collision via traces (world BVH), constrain distance to control point;
//! - renderers: animated sprites (orientations 0-3), sprite trails, ropes, light sources
//!   (a small pool of point lights).
//!
//! Not drawn: refraction materials and screen-space systems. Decals (`Emit Decal`) are quads
//! laid on the surface at the impact.

use crate::player::{to_bevy, MainCamera, UNIT};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use tf_assets::pcf::{Op, SystemDef};
use tf_assets::vpk::Vpk;
use tf_assets::vtf::{self, Sequence};
use tf_sim::glam::Vec3 as SVec3;

/// Hard cap on live particles across all systems.
const MAX_PARTICLES: usize = 12000;
/// Unattached effects with endless emitters stop emitting after this long (the game stops
/// looping effects from script; here every effect is fire-and-forget).
const DEFAULT_STOP: f32 = 2.5;
/// And are removed outright after this long.
const HARD_LIMIT: f32 = 20.0;
/// Brightness of the game's 0-255 colours on screen (unlit, after exposure).
const BLEND_GAIN: f32 = 1.0;
const ADD_GAIN: f32 = 2.0;
/// Sprite material extension: Source's spritecard `$depthblend`/`$depthblendscale` (soft
/// particles against the depth prepass) and `$ignorez` (no depth test). See soft.wgsl.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(SoftKey)]
pub struct SoftParticle {
    /// x: depth blend distance in metres (0 = off).
    #[uniform(100)]
    pub params: Vec4,
    pub ignorez: bool,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct SoftKey {
    ignorez: bool,
}

impl From<&SoftParticle> for SoftKey {
    fn from(m: &SoftParticle) -> Self {
        SoftKey { ignorez: m.ignorez }
    }
}

impl bevy::pbr::MaterialExtension for SoftParticle {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/soft.wgsl".into()
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if key.bind_group_data.ignorez {
            if let Some(ds) = descriptor.depth_stencil.as_mut() {
                ds.depth_compare = bevy::render::render_resource::CompareFunction::Always;
            }
        }
        Ok(())
    }
}

pub type SoftMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, SoftParticle>;

pub struct SoftMaterialPlugin;

impl Plugin for SoftMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "soft.wgsl");
        app.add_plugins(MaterialPlugin::<SoftMaterial>::default());
    }
}

/// An effect model part's material: the plain one, or the effect path (uber.rs) for
/// surfaces whose look is their scrolling UVs and edge fade (the Thermal Shield's flame fan).
#[derive(Clone)]
pub enum FxMat {
    Std(Handle<StandardMaterial>),
    Uber(Handle<crate::uber::UberMaterial>),
}

/// Effect models (shields, walls) use the game's unlit `Add`/`Trans` shader sets, which the
/// plain model path draws opaque: swap in unlit materials with the right blend, tinted by the
/// system's colour (`Entcol*`: the entity colour modulates them).
#[allow(clippy::too_many_arguments)]
fn fx_model_materials(gd: &crate::gamedata::GameData, cache: &mut crate::convert::Cache, images: &mut Assets<Image>, path: &str, parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>, mats: &mut Assets<StandardMaterial>, uber: &mut Assets<crate::uber::UberMaterial>, tint: Vec3, hdr: f32) -> Vec<(Handle<Mesh>, FxMat)> {
    let plain = |parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>| parts.into_iter().map(|(m, h)| (m, FxMat::Std(h))).collect();
    let Ok(model) = gd.read_file(path).and_then(|b| tf_assets::mdl::Model::parse(b).map_err(Into::into)) else { return plain(parts) };
    let Ok(meshes) = model.meshes(&[]) else { return plain(parts) };
    let mut out = Vec::with_capacity(parts.len());
    for (md, (mesh, mat)) in meshes.iter().zip(parts) {
        let tex = model.skin_families.first().and_then(|f| f.get(md.material)).copied().unwrap_or(0) as usize;
        let name = model.textures.get(tex).cloned().unwrap_or_default();
        let found = gd.material(&name);
        let shader = found.as_ref().and_then(|(pi, m)| tf_assets::material::shader_set_name(&gd.paks[*pi], m.shader_set)).unwrap_or_default();
        let Some(src) = mats.get(&mat) else {
            out.push((mesh, FxMat::Std(mat)));
            continue;
        };
        let mut m = src.clone();
        if shader.contains("Unlit") {
            m.unlit = true;
        }
        // Hex shield walls (the A-Wall's `pilot_shield_wall*`, `pred_shield_hex`): a white base
        // whose look is a refraction through a hex normal map. Without refraction, the normal
        // map's slope (bright on the cell edges, over a faint fill) drawn additively stands in.
        let hex = found.as_ref().and_then(|(pi, gm)| {
            gm.textures.iter().filter(|&&g| g != 0).find_map(|&g| {
                let (tp, ta) = gd.find_texture(*pi, g)?;
                let n = tf_assets::texture::info(&gd.paks[tp], &gd.paks[tp].assets[ta]).ok()?.name?;
                n.contains("refract_hex_nml").then(|| hex_edges(gd, cache, images, *pi, g)).flatten()
            })
        });
        if let Some(img) = hex.filter(|_| shader.contains("Add") || shader.contains("Trans")) {
            m.alpha_mode = AlphaMode::Add;
            m.unlit = true;
            m.base_color = Color::srgb(tint.x, tint.y, tint.z);
            m.base_color_texture = Some(img);
            m.double_sided = true;
            m.cull_mode = None;
            out.push((mesh, FxMat::Std(mats.add(m))));
            continue;
        }
        if shader.contains("Add") && name.contains("xo_shield_wall") {
            // The Particle Wall's shells build their pattern in the shader (scaled, scrolled UV
            // layers), which the plain path doesn't: their base texture alone reads as a flat
            // sheet. A hex lattice tiled over their UVs stands in for it.
            m.alpha_mode = AlphaMode::Add;
            m.base_color = Color::srgb(tint.x, tint.y, tint.z);
            m.base_color_texture = Some(hex_tile(images));
        } else if shader.contains("Add") {
            m.alpha_mode = AlphaMode::Add;
            m.base_color = Color::srgb(tint.x, tint.y, tint.z);
            // Scrolling or edge-faded additive surfaces (the Thermal Shield's
            // `fx\flame_shield_edge`: a caustic pattern scrolled over the fan, fading toward
            // its edge-on parts) take the effect path; their base texture alone is a flat sheet.
            if let Some(info) = crate::uber::info_always(gd, &name).filter(|i| i.scroll || i.flags & crate::uber::AEF != 0) {
                m.double_sided = true;
                m.cull_mode = None;
                // With the opacity layer keeping the gaps dark, the system's HDR gain (the
                // flame lines' brightness) can come back.
                // The colour is a gamma value: linearise it before the gain, or the lines wash
                // out to a pale peach.
                let l = Color::srgb(tint.x.min(1.0), tint.y.min(1.0), tint.z.min(1.0)).to_linear();
                m.base_color = Color::linear_rgb(l.red * hdr, l.green * hdr, l.blue * hdr);
                log::debug!("particle model {path}: {name} via the effect path ({:?})", info.params);
                let h = crate::uber::material(&m, &info, uber);
                // Its other layers: the high-contrast caustic opacity (slot 13, through UV1)
                // and a cloud offset map (slot 18) scrolled through UV2 warping the pattern.
                // Which slot rides which transform is read from the shader name loosely, and
                // the warp amplitude (0.05 of the texture) is by eye.
                if let (Some((pi, gm)), Some(u)) = (found.as_ref(), uber.get_mut(&h)) {
                    let tex = |slot: usize, cache: &mut crate::convert::Cache, images: &mut Assets<Image>| gm.textures.get(slot).filter(|&&g| g != 0).and_then(|&g| crate::convert::image(gd, cache, images, *pi, g));
                    if let Some(img) = tex(13, cache, images) {
                        u.extension.opacity = Some(img);
                        u.extension.params.fade2.y = (info.flags | crate::uber::OPA) as f32;
                    }
                    if let Some(img) = tex(18, cache, images).filter(|_| shader.contains("Uvd")) {
                        u.extension.distort = Some(img);
                        let uv2 = info.params.uv[1];
                        u.extension.params.uv2 = Vec4::new(uv2[0], uv2[1], uv2[2], uv2[3]);
                        u.extension.params.uv2_t = Vec4::new(uv2[4], uv2[5], if shader.contains("Uv2at") { 1.0 } else { 0.0 }, 0.05);
                        let f = u.extension.params.fade2.y as u32;
                        u.extension.params.fade2.y = (f | crate::uber::DIST) as f32;
                    }
                }
                out.push((mesh, FxMat::Uber(h)));
                continue;
            }
        } else if shader.contains("Trans") {
            // The hex shields are refraction materials (a white base, a caustic pattern in
            // slot 4 and a hex normal map): without refraction, the caustic pattern tinted and
            // made translucent is the closest look.
            m.alpha_mode = AlphaMode::Blend;
            m.base_color = Color::srgba(tint.x, tint.y, tint.z, 0.35);
            if let Some((pi, gm)) = found.as_ref() {
                if let Some(&g) = gm.textures.get(4).filter(|&&g| g != 0) {
                    if let Some(img) = crate::convert::image(gd, cache, images, *pi, g) {
                        m.base_color_texture = Some(img);
                    }
                }
            }
        } else if shader.is_empty() {
            // No material found (the walls' `models/fx/ar_impact_pilot` impact layer): drawn
            // with the plain path it is an opaque slab, so leave it out.
            m.base_color = Color::NONE;
            m.alpha_mode = AlphaMode::Blend;
        } else {
            m.base_color = Color::srgb(tint.x, tint.y, tint.z);
        }
        m.double_sided = true;
        m.cull_mode = None;
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &md.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        log::debug!("particle model {path}: {name} shader {shader} bounds {lo:?}..{hi:?} tex {:?} slots {:?}", m.base_color_texture.is_some(), found.as_ref().map(|(_, gm)| gm.textures.clone()));
        out.push((mesh, FxMat::Std(mats.add(m))));
    }
    out
}

/// The cell edges of a hex normal map: its slope (|xy| of the decoded normal), as a
/// brightness texture with mips.
fn hex_edges(gd: &crate::gamedata::GameData, cache: &mut crate::convert::Cache, images: &mut Assets<Image>, pak_hint: usize, guid: u64) -> Option<Handle<Image>> {
    let key = guid ^ 0x4E58_0000_0000_0001;
    if let Some(h) = cache.images.get(&key) {
        return h.clone();
    }
    let result = (|| {
        let (pi, ai) = gd.find_texture(pak_hint, guid)?;
        let pak = &gd.paks[pi];
        let tex = tf_assets::texture::load_max(pak, &pak.assets[ai], 512).ok()?;
        if !matches!(tex.info.format, tf_assets::texture::TexFormat::Bc5) {
            return None;
        }
        let (w, h) = (tex.width as usize, tex.height as usize);
        let px = tf_assets::texture::decode_bc5(&tex.data, w, h);
        let mut level: Vec<u8> = px
            .iter()
            .map(|&[r, g]| {
                let (x, y) = (r as f32 / 127.5 - 1.0, g as f32 / 127.5 - 1.0);
                ((0.12 + 0.88 * (((x * x + y * y).sqrt() - 0.1) * 2.5).clamp(0.0, 1.0).powf(0.8)) * 255.0) as u8
            })
            .collect();
        let (mut lw, mut lh) = (w, h);
        let mut data = Vec::new();
        let mut mips = 0;
        loop {
            for &v in &level {
                data.extend_from_slice(&[v, v, v, 255]);
            }
            mips += 1;
            if lw == 1 && lh == 1 {
                break;
            }
            let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
            let at = |x: usize, y: usize| level[y.min(lh - 1) * lw + x.min(lw - 1)] as u32;
            level = (0..nw * nh).map(|i| {
                let (x, y) = (i % nw * 2, i / nw * 2);
                ((at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1) + 2) / 4) as u8
            }).collect();
            (lw, lh) = (nw, nh);
        }
        let mut img = Image::default();
        img.data = Some(data);
        img.texture_descriptor.size = bevy::render::render_resource::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 };
        img.texture_descriptor.mip_level_count = mips;
        img.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
        img.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
        Some(images.add(img))
    })();
    cache.images.insert(key, result.clone());
    result
}

/// A tileable hex lattice (one cell column wide, two rows tall: the lattice's rectangular
/// period), bright lines on a faint fill, for shield walls.
fn hex_tile(images: &mut Assets<Image>) -> Handle<Image> {
    static TILE: std::sync::OnceLock<Handle<Image>> = std::sync::OnceLock::new();
    TILE.get_or_init(|| {
        let (w, h) = (64u32, 111u32); // 1 : sqrt(3)
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        // Hexagon centres of the lattice in this period (x in 0..1, y in 0..sqrt 3).
        let s3 = 3.0f32.sqrt();
        let centres = [(0.0, 0.0), (1.0, 0.0), (0.5, s3 * 0.5), (0.0, s3), (1.0, s3)];
        for y in 0..h {
            for x in 0..w {
                let p = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32 * s3);
                // Distance to the nearest and second-nearest centre: equal on a cell edge.
                let mut d: Vec<f32> = centres.iter().map(|&(cx, cy)| p.distance(Vec2::new(cx, cy))).collect();
                d.sort_by(f32::total_cmp);
                let edge = d[1] - d[0];
                let line = (1.0 - edge / 0.06).clamp(0.0, 1.0);
                let v = (0.12 + 0.9 * line * line).min(1.0);
                let c = (v * 255.0) as u8;
                px.extend_from_slice(&[c, c, c, 255]);
            }
        }
        let mut img = Image::new(
            bevy::render::render_resource::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            px,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
            address_mode_u: bevy::image::ImageAddressMode::Repeat,
            address_mode_v: bevy::image::ImageAddressMode::Repeat,
            ..bevy::image::ImageSamplerDescriptor::linear()
        });
        images.add(img)
    })
    .clone()
}

/// Source's `$depthblendscale` default (game units).
const DEPTH_BLEND_DEFAULT: f32 = 50.0;

/// Depth (game units) over which a sprite fades out as it nears the eye.
const NEAR_FADE_START: f32 = 12.0;
/// Bullet-hole decals: width in game units (the decal materials don't carry a size; a guess
/// matching the game's rifle holes) and how many stay before the oldest goes.
const DECAL_SIZE: f32 = 4.0;
const MAX_DECALS: usize = 256;
/// First-person particle models closer than this to the eye are hidden (game units).
const FP_MODEL_CLEARANCE: f32 = 30.0;
const NEAR_FADE_END: f32 = 60.0;
const LIGHTS: usize = 6;
/// Lumens of a light-source particle at colour 1 (the game's lights are in its own units;
/// set by eye so a muzzle flash lights the cockpit without washing it out).
const LIGHT_LUMENS: f32 = 8_000.0;

/// Bevy space (metres, Y up) to game space (inches, Z up).
pub fn to_game(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y) / UNIT
}

fn dir_to_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

fn dir_to_game(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// A control point: position and frame (X forward, Y left, Z up), game space.
#[derive(Clone, Copy, Debug)]
pub struct Cp {
    pub pos: Vec3,
    pub fwd: Vec3,
    pub left: Vec3,
    pub up: Vec3,
}

impl Default for Cp {
    fn default() -> Self {
        Self { pos: Vec3::ZERO, fwd: Vec3::X, left: Vec3::Y, up: Vec3::Z }
    }
}

impl Cp {
    /// A frame looking along `fwd` (game space), Z as up where possible.
    pub fn facing(pos: Vec3, fwd: Vec3) -> Self {
        let fwd = fwd.normalize_or(Vec3::X);
        let hint = if fwd.z.abs() > 0.95 { Vec3::X } else { Vec3::Z };
        let left = hint.cross(fwd).normalize_or(Vec3::Y);
        let up = fwd.cross(left);
        Self { pos, fwd, left, up }
    }
    /// A frame whose up (Z) is `normal` (impacts).
    pub fn on_surface(pos: Vec3, normal: Vec3) -> Self {
        let up = normal.normalize_or(Vec3::Z);
        let hint = if up.z.abs() > 0.95 { Vec3::X } else { Vec3::Z };
        let left = up.cross(hint).normalize_or(Vec3::Y);
        let fwd = left.cross(up);
        Self { pos, fwd, left, up }
    }
    fn local(&self, v: Vec3) -> Vec3 {
        self.fwd * v.x + self.left * v.y + self.up * v.z
    }
}

// ---------------------------------------------------------------------------------------------
// Requests

/// Play a named system (Bevy-space points), for call sites that know the game's effect name.
#[derive(Component)]
pub struct PfxRequest {
    pub name: String,
    pub cps: Vec<Cp>,
    pub stop_after: Option<f32>,
    /// A first-person (`_FP`) system: it fades by size rather than by nearness to the eye.
    /// (Drawn in the world, depth-tested, so its smoke goes behind walls; at hip fire the
    /// viewmodel camera has the world's FOV, so it lines up with the gun.)
    pub vm: bool,
}

/// TF_PFX_TEST=<system>: the script action `pfxtest` plays it on the ground 300 units in front
/// of the camera, facing up (for checking an effect in isolation).
pub fn pfx_test(mut commands: Commands, mut input: ResMut<crate::player::PlayerInput>, cams: Query<&GlobalTransform, With<crate::player::MainCamera>>) {
    if !std::mem::take(&mut input.pfx_test) {
        return;
    }
    let Some(name) = std::env::var("TF_PFX_TEST").ok() else { return };
    let Ok(cam) = cams.single() else { return };
    let mut f = cam.forward().as_vec3();
    f.y = 0.0;
    let at = cam.translation() + f.normalize_or(Vec3::NEG_Z) * 300.0 * crate::player::UNIT - Vec3::Y * 60.0 * crate::player::UNIT;
    log::info!("pfx test {name}");
    emit_named(&mut commands, &name, at, Vec3::Y);
}

/// Play `name` at `at` looking along `dir` (Bevy space).
pub fn emit_named(commands: &mut Commands, name: &str, at: Vec3, dir: Vec3) {
    commands.spawn(PfxRequest { name: name.to_string(), cps: vec![Cp::facing(to_game(at), dir_to_game(dir))], stop_after: None, vm: false });
}

/// Play a first-person (`_FP`) system at a viewmodel attachment: `rot` is its Bevy-space
/// rotation of the game frame (X forward, Y left, Z up), as joint globals give it.
pub fn emit_named_vm_frame(commands: &mut Commands, name: &str, at: Vec3, rot: Quat) {
    let (fwd, left, up) = (dir_to_game(rot * Vec3::X).normalize_or(Vec3::X), dir_to_game(rot * Vec3::Y).normalize_or(Vec3::Y), dir_to_game(rot * Vec3::Z).normalize_or(Vec3::Z));
    commands.spawn(PfxRequest { name: name.to_string(), cps: vec![Cp { pos: to_game(at), fwd, left, up }], stop_after: None, vm: true });
}

/// Marks a [`PfxRequest`] to be drawn by the viewmodel camera, over the first-person models
/// (EffectSetIsWithCockpit: sparks on BT's cockpit panels).
#[derive(Component)]
pub struct PfxOverlay;

/// Play a first-person system on the cockpit (as `emit_named_vm_frame`, drawn by the viewmodel
/// camera so the cockpit model doesn't hide it).
pub fn emit_named_cockpit(commands: &mut Commands, name: &str, at: Vec3, rot: Quat) {
    let (fwd, left, up) = (dir_to_game(rot * Vec3::X).normalize_or(Vec3::X), dir_to_game(rot * Vec3::Y).normalize_or(Vec3::Y), dir_to_game(rot * Vec3::Z).normalize_or(Vec3::Z));
    commands.spawn((PfxRequest { name: name.to_string(), cps: vec![Cp { pos: to_game(at), fwd, left, up }], stop_after: None, vm: true }, PfxOverlay));
}

/// Play a system at an attachment frame (as `emit_named_vm_frame`), in the world.
pub fn emit_named_frame(commands: &mut Commands, name: &str, at: Vec3, rot: Quat) {
    let (fwd, left, up) = (dir_to_game(rot * Vec3::X).normalize_or(Vec3::X), dir_to_game(rot * Vec3::Y).normalize_or(Vec3::Y), dir_to_game(rot * Vec3::Z).normalize_or(Vec3::Z));
    commands.spawn(PfxRequest { name: name.to_string(), cps: vec![Cp { pos: to_game(at), fwd, left, up }], stop_after: None, vm: false });
}

/// Play a first-person (`_FP`) system at a viewmodel point.
pub fn emit_named_vm(commands: &mut Commands, name: &str, at: Vec3, dir: Vec3) {
    commands.spawn(PfxRequest { name: name.to_string(), cps: vec![Cp::facing(to_game(at), dir_to_game(dir))], stop_after: None, vm: true });
}

/// Play a beam/tracer system from `from` to `to` (CP0 and CP1, Bevy space).
pub fn emit_beam(commands: &mut Commands, name: &str, from: Vec3, to: Vec3) {
    let (a, b) = (to_game(from), to_game(to));
    let f = Cp::facing(a, b - a);
    commands.spawn(PfxRequest { name: name.to_string(), cps: vec![f, Cp { pos: b, ..f }], stop_after: None, vm: false });
}

/// Play an impact effect table's effects for a surface (default concrete) at `at` with the
/// surface `normal` (Bevy space). Tables are `scripts/impacts/<table>.txt`.
pub fn emit_impact(commands: &mut Commands, table: &str, at: Vec3, normal: Vec3) {
    commands.spawn(ImpactRequest { table: table.to_string(), cp: Cp::on_surface(to_game(at), dir_to_game(normal)) });
}

#[derive(Component)]
pub struct ImpactRequest {
    table: String,
    cp: Cp,
}

/// Put on an entity to run a system that follows it (projectile trails): CP0 is the entity's
/// position, facing its motion. Emission stops when the entity goes away.
#[derive(Component)]
pub struct PfxTrail {
    pub name: String,
    effect: Option<u64>,
    last: Option<Vec3>,
    /// Control point 0 takes the entity's rotation (forward = Bevy -Z) instead of its motion.
    pub oriented: bool,
}

impl PfxTrail {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), effect: None, last: None, oriented: false }
    }
    /// An effect fixed to an entity's position and rotation (shields, walls).
    pub fn oriented(name: &str) -> Self {
        Self { name: name.to_string(), effect: None, last: None, oriented: true }
    }
}

// ---------------------------------------------------------------------------------------------
// Compiled operators

#[derive(Clone, Copy, Debug)]
enum Field {
    Radius,
    Alpha,
    Trail,
    Roll,
    RollSpeed,
    Lifetime,
    Alpha2,
    Sequence,
    /// Creation time (as an input: the particle's age).
    Age,
    Id,
    Other,
}

fn field(i: i32) -> Field {
    match i {
        1 => Field::Lifetime,
        3 => Field::Radius,
        4 => Field::Roll,
        5 => Field::RollSpeed,
        7 => Field::Alpha,
        9 => Field::Sequence,
        10 => Field::Trail,
        16 => Field::Alpha2,
        8 => Field::Age,
        11 => Field::Id,
        _ => Field::Other,
    }
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from(a)
}

/// A system's 0-255 colour as linear RGB: the game tints its sRGB textures in gamma space,
/// which is the texture's linear colour times the tint's linear value (taken as linear, a
/// 73-grey smoke drew as a light haze).
fn col(c: [u8; 4]) -> Vec3 {
    if std::env::var_os("TF_PFX_GAMMA_COLORS").is_some() {
        return Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) / 255.0;
    }
    let l = Color::srgb_u8(c[0], c[1], c[2]).to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

/// Back to sRGB (for the model path, whose materials take sRGB tints).
fn to_srgb(v: Vec3) -> Vec3 {
    let s = Color::linear_rgb(v.x, v.y, v.z).to_srgba();
    Vec3::new(s.red, s.green, s.blue)
}

#[derive(Clone, Debug)]
enum Emit {
    Instant { count: u32, min: u32, start: f32 },
    Continuous { rate: f32, duration: f32, start: f32 },
    Distance { spacing: f32 },
}

#[derive(Clone, Debug)]
enum Init {
    Lifetime { min: f32, max: f32, exp: f32 },
    LifetimeFromSequence { fps: f32 },
    Radius { min: f32, max: f32, exp: f32 },
    Alpha { min: f32, max: f32, exp: f32 },
    Color { c1: Vec3, c2: Vec3 },
    Sphere { cp: usize, dmin: f32, dmax: f32, bias: Vec3, abs: Vec3, local_bias: bool, smin: f32, smax: f32, sexp: f32, lmin: Vec3, lmax: Vec3 },
    Box { cp: usize, min: Vec3, max: Vec3 },
    Ring { cp: usize, radius: f32, thickness: f32, smin: f32, smax: f32, xy: bool },
    Offset { cp: usize, min: Vec3, max: Vec3, local: bool, prop: bool },
    Warp { cp: usize, min: Vec3, max: Vec3 },
    Velocity { cp: usize, lmin: Vec3, lmax: Vec3, rmin: f32, rmax: f32 },
    VelocityNoise { cp: usize, min: Vec3, max: Vec3, local: bool },
    InheritCpVelocity { cp: usize, scale: f32 },
    Rotation { initial: f32, min: f32, max: f32, flip: bool },
    RotationSpeed { constant: f32, min: f32, max: f32, flip: bool },
    YawFlip { pct: f32 },
    Sequence { min: i32, max: i32 },
    Trail { min: f32, max: f32, exp: f32 },
    Scalar { field: Field, min: f32, max: f32, exp: f32 },
    ColorVector { min: Vec3, max: Vec3 },
    Between { end: usize, smin: f32, smax: f32, spread: f32 },
    Path { start: usize, end: usize, sequential: bool, count: f32 },
    /// Position From Parent Particles: born on a random particle of the parent system, with
    /// its velocity scaled (no parent particles: not born).
    FromParent { scale: f32 },
    /// Remap Initial Scalar: one field's starting value from another's.
    Remap { inp: Field, out: Field, imin: f32, imax: f32, omin: f32, omax: f32, scale: bool, active: bool },
}

#[derive(Clone, Debug)]
enum Oper {
    Move { gravity: Vec3, drag: f32 },
    Decay,
    RadiusScale { st: f32, et: f32, ss: f32, es: f32, bias: f32 },
    FadeAndDecay { sa: f32, ea: f32, sfi: f32, efi: f32, sfo: f32, efo: f32 },
    FadeOutRandom { min: f32, max: f32, prop: bool },
    FadeInRandom { min: f32, max: f32, prop: bool },
    FadeOutSimple { t: f32 },
    FadeInSimple { t: f32 },
    ColorFade { to: Vec3, st: f32, et: f32 },
    /// Rotation Spin Roll: `spin_rate_degrees`, easing to `spin_rate_min` over `spin_stop_time`
    /// seconds of the particle's life (0: no easing).
    SpinRoll { rate: f32, min: f32, stop: f32 },
    /// Oscillate Vector on the colour (the default field, 6): like Oscillate Scalar per channel.
    OscillateColor { rate: (Vec3, Vec3), freq: (Vec3, Vec3), mult: f32, phase: f32 },
    /// Restart Effect after Duration: re-runs the node's emitters every min..max seconds.
    Restart { min: f32, max: f32, kill: bool },
    /// Roll each particle to point away from a control point, plus an offset (Source's
    /// C_OP_Orient2DRelToCP; Source measures in world XY, this in the point's own XY plane,
    /// which is where the cockpit screen effects ring their particles).
    OrientToCp { cp: usize, offset: f32, strength: f32 },
    /// Remap Scalar: a field from another (creation time reads as the particle's age).
    Remap { inp: Field, out: Field, imin: f32, imax: f32, omin: f32, omax: f32, scale: bool, abs: bool },
    /// Oscillate Scalar: a field swings by a per-particle rate and frequency. Source integrates
    /// `rate * cos` each frame; fields reset to their base here, so the closed form is used.
    Oscillate { field: Field, rate: (f32, f32), freq: (f32, f32), st: (f32, f32), et: (f32, f32), prop: bool, mult: f32, phase: f32 },
    /// Remap Distance to Control Point to Scalar.
    DistToCp { cp: usize, out: Field, dmin: f32, dmax: f32, omin: f32, omax: f32, active: bool, scale_init: bool, scale_cur: bool },
    /// Remap Distance Between Two Control Points to Scalar (the same value for every particle).
    DistBetweenCps { a: usize, b: usize, out: Field, dmin: f32, dmax: f32, omin: f32, omax: f32, scale_init: bool, scale_cur: bool },
    /// Ramp Scalar Linear Simple: a field grows at a rate between two times (closed form).
    Ramp { field: Field, rate: f32, st: f32, et: f32 },
    Graph { field: Field, pts: Vec<[f32; 2]>, omin: f32, omax: f32, mul: bool, add: bool, lifespans: bool, time: f32, looped: bool },
    LerpInitial { field: Field, to: f32, st: f32, et: f32 },
    LockToCp { cp: usize },
    MaxVelocity { max: f32 },
    SetCps { base: usize, sets: Vec<(usize, Vec3)>, world: bool },
    /// Set Control Point To Player: the control point follows the camera (view origin).
    CpToPlayer { cp: usize },
}

#[derive(Clone, Debug)]
enum Force {
    Pull { cp: usize, amount: f32, falloff: f32 },
    Random { min: Vec3, max: Vec3 },
    Turbulent { amount: f32 },
    Twist { cp: usize, amount: f32, axis: Vec3, local: bool },
}

#[derive(Clone, Debug)]
enum Constraint {
    Collide { bounce: f32, slide: f32, kill_speed: f32 },
    Distance { cp: usize, min: f32, max: f32 },
}

#[derive(Clone, Debug)]
enum Render {
    Sprites { rate: f32, fit: bool, orient: i32 },
    Trail { rate: f32, min: f32, max: f32, fade_in: f32, tail: [f32; 4] },
    Rope,
    Light { scale: f32, radius: f32, by_alpha: bool },
    /// Render models: each particle draws a model (game shields, bullets, debris), scaled by
    /// its radius, oriented by the control point (or its Z to the normal) plus model-space
    /// pitch/yaw/roll.
    Models { model: String, pitch: f32, yaw: f32, roll: f32, orient_normal: bool },
    /// Emit Decal: a bullet hole (one of the materials, chosen at random) on the surface at
    /// control point 0, whose up is the surface normal for impacts.
    Decal { materials: Vec<String> },
}

#[derive(Clone, Debug)]
struct Compiled {
    material: String,
    max: usize,
    initial: usize,
    radius: f32,
    color: Vec3,
    /// "color HDR scale", applied to every colour the system sets.
    hdr: f32,
    alpha: f32,
    emit: Vec<Emit>,
    init: Vec<Init>,
    /// Per-initializer "operator end cap state": -1 always, 0 not during the end cap, 1 only
    /// during it (after the effect is stopped).
    init_cap: Vec<i8>,
    oper: Vec<Oper>,
    oper_cap: Vec<i8>,
    force: Vec<Force>,
    cons: Vec<Constraint>,
    render: Vec<Render>,
    children: Vec<(usize, f32, bool)>,
    /// Whether any operator retires particles at the end of their life.
    decays: bool,
}

/// Control points per effect (the game's systems address up to CP 10).
const NCP: usize = 16;

fn cpn(o: &Op, k: &str) -> usize {
    o.i(k, 0).clamp(0, NCP as i32 - 1) as usize
}

fn compile(def: &SystemDef, by_name: &HashMap<String, usize>) -> Compiled {
    let mut emit = Vec::new();
    for o in &def.emitters {
        match o.function.as_str() {
            "emit_instantaneously" => emit.push(Emit::Instant { count: o.i("num_to_emit", 100).max(0) as u32, min: o.i("num_to_emit_minimum", -1).max(-1) as u32, start: o.f("emission_start_time", 0.0) }),
            "emit_continuously" => emit.push(Emit::Continuous { rate: o.f("emission_rate", 100.0), duration: o.f("emission_duration", 0.0), start: o.f("emission_start_time", 0.0) }),
            "emit noise" => emit.push(Emit::Continuous { rate: (o.f("emission minimum", 0.0) + o.f("emission maximum", 100.0)) * 0.5, duration: o.f("emission_duration", 0.0), start: o.f("emission_start_time", 0.0) }),
            "emit over distance" => emit.push(Emit::Distance { spacing: o.f("distance between emissions", 100.0).max(1.0) }),
            "emit_instantaneously by distance" => emit.push(Emit::Instant { count: o.i("minimum count", 1).max(1) as u32, min: u32::MAX, start: o.f("emission_start_time", 0.0) }),
            _ => {}
        }
    }
    let mut init = Vec::new();
    let mut init_cap = Vec::new();
    for o in &def.initializers {
        let i = match o.function.as_str() {
            "Lifetime Random" => Init::Lifetime { min: o.f("lifetime_min", 0.0), max: o.f("lifetime_max", 0.0), exp: o.f("lifetime_random_exponent", 1.0) },
            "Lifetime From Sequence" | "lifetime from sequence" => Init::LifetimeFromSequence { fps: o.f("Frames Per Second", 30.0) },
            "Radius Random" => Init::Radius { min: o.f("radius_min", 1.0), max: o.f("radius_max", 1.0), exp: o.f("radius_random_exponent", 1.0) },
            "Alpha Random" => Init::Alpha { min: o.f("alpha_min", 255.0) / 255.0, max: o.f("alpha_max", 255.0) / 255.0, exp: o.f("alpha_random_exponent", 1.0) },
            "Color Random" | "Color Lit Per Particle" => Init::Color { c1: col(o.color("color1", [255; 4])), c2: col(o.color("color2", [255; 4])) },
            "Position Within Sphere Random" => Init::Sphere {
                cp: cpn(o, "control_point_number"),
                dmin: o.f("distance_min", 0.0),
                dmax: o.f("distance_max", 0.0),
                bias: v3(o.v3("distance_bias", [1.0; 3])),
                abs: v3(o.v3("distance_bias_absolute_value", [0.0; 3])),
                local_bias: o.b("bias in local system", false),
                smin: o.f("speed_min", 0.0),
                smax: o.f("speed_max", 0.0),
                sexp: o.f("speed_random_exponent", 1.0),
                lmin: v3(o.v3("speed_in_local_coordinate_system_min", [0.0; 3])),
                lmax: v3(o.v3("speed_in_local_coordinate_system_max", [0.0; 3])),
            },
            "Position Within Box Random" => Init::Box { cp: cpn(o, "control point number"), min: v3(o.v3("min", [0.0; 3])), max: v3(o.v3("max", [0.0; 3])) },
            "Position Along Ring" => Init::Ring {
                cp: cpn(o, "control point number"),
                radius: o.f("initial radius", 0.0),
                thickness: o.f("thickness", 0.0),
                smin: o.f("min initial speed", 0.0),
                smax: o.f("max initial speed", 0.0),
                xy: o.b("XY velocity only", true),
            },
            "Position Modify Offset Random" => Init::Offset {
                cp: cpn(o, "control_point_number"),
                min: v3(o.v3("offset min", [0.0; 3])),
                max: v3(o.v3("offset max", [0.0; 3])),
                local: o.b("offset in local space 0/1", false),
                prop: o.b("offset proportional to radius 0/1", false),
            },
            "Position Modify Warp Random" => Init::Warp { cp: cpn(o, "control point number"), min: v3(o.v3("warp min", [1.0; 3])), max: v3(o.v3("warp max", [1.0; 3])) },
            "Velocity Random" => Init::Velocity {
                cp: cpn(o, "control_point_number"),
                lmin: v3(o.v3("speed_in_local_coordinate_system_min", [0.0; 3])),
                lmax: v3(o.v3("speed_in_local_coordinate_system_max", [0.0; 3])),
                rmin: o.f("random_speed_min", 0.0),
                rmax: o.f("random_speed_max", 0.0),
            },
            "Velocity Noise" => Init::VelocityNoise {
                cp: cpn(o, "Control Point Number"),
                min: v3(o.v3("output minimum", [0.0; 3])),
                max: v3(o.v3("output maximum", [1.0; 3])),
                local: o.b("Apply Velocity in Local Space (0/1)", false),
            },
            "Velocity Inherit from Control Point" => Init::InheritCpVelocity { cp: cpn(o, "control point number"), scale: o.f("velocity scale", 1.0) },
            "Position From Parent Particles" => Init::FromParent { scale: o.f("Inherited Velocity Scale", 0.0) },
            "Remap Initial Scalar" => Init::Remap {
                inp: field(o.i("input field", 8)),
                out: field(o.i("output field", 3)),
                imin: o.f("input minimum", 0.0),
                imax: o.f("input maximum", 1.0),
                omin: o.f("output minimum", 0.0),
                omax: o.f("output maximum", 1.0),
                scale: o.b("output is scalar of initial random range", false),
                active: o.b("only active within specified input range", false),
            },
            "Rotation Random" => Init::Rotation {
                initial: o.f("rotation_initial", 0.0),
                min: o.f("rotation_offset_min", 0.0),
                max: o.f("rotation_offset_max", 360.0),
                flip: o.b("randomly_flip_direction", false),
            },
            "Rotation Speed Random" => Init::RotationSpeed {
                constant: o.f("rotation_speed_constant", 0.0),
                min: o.f("rotation_speed_random_min", 0.0),
                max: o.f("rotation_speed_random_max", 360.0),
                flip: o.b("randomly_flip_direction", true),
            },
            "Rotation Yaw Flip Random" => Init::YawFlip { pct: o.f("Flip Percentage", 0.5) },
            "Sequence Random" => Init::Sequence { min: o.i("sequence_min", 0), max: o.i("sequence_max", 0) },
            "Trail Length Random" => Init::Trail { min: o.f("length_min", 0.1), max: o.f("length_max", 0.1), exp: o.f("length_random_exponent", 1.0) },
            "Scalar Random" => Init::Scalar { field: field(o.i("output field", 3)), min: o.f("min", 0.0), max: o.f("max", 1.0), exp: o.f("exponent", 1.0) },
            "Remap Noise to Scalar" => Init::Scalar { field: field(o.i("output field", 3)), min: o.f("output minimum", 0.0), max: o.f("output maximum", 1.0), exp: 1.0 },
            "Vector Random" if o.i("output field", 6) == 6 => Init::ColorVector { min: v3(o.v3("min", [0.0; 3])), max: v3(o.v3("max", [1.0; 3])) },
            "move particles between 2 control points" | "Move Particles Between 2 Control Points" => Init::Between {
                end: cpn(o, "end control point").max(1),
                smin: o.f("minimum speed", 1.0),
                smax: o.f("maximum speed", 1.0),
                spread: o.f("end spread", 0.0),
            },
            "Position Along Path Sequential" | "Position Along Path Random" => Init::Path {
                start: cpn(o, "start control point number"),
                end: cpn(o, "end control point number"),
                sequential: o.function.ends_with("Sequential"),
                count: o.f("particles to map from start to end", 100.0).max(1.0),
            },
            _ => continue,
        };
        init.push(i);
        init_cap.push(o.i("operator end cap state", -1) as i8);
    }
    let mut oper = Vec::new();
    let mut oper_cap = Vec::new();
    let mut decays = false;
    for o in &def.operators {
        let op = match o.function.as_str() {
            "Movement Basic" => Oper::Move { gravity: v3(o.v3("gravity", [0.0; 3])), drag: o.f("drag", 0.0) },
            "Lifespan Decay" => {
                decays = true;
                Oper::Decay
            }
            "Radius Scale" => Oper::RadiusScale {
                st: o.f("start_time", 0.0),
                et: o.f("end_time", 1.0),
                ss: o.f("radius_start_scale", 1.0),
                es: o.f("radius_end_scale", 1.0),
                bias: o.f("scale_bias", 0.5),
            },
            "Alpha Fade and Decay" | "Alpha Fade and Decay for Tracers" => {
                decays = true;
                Oper::FadeAndDecay {
                    sa: o.f("start_alpha", 1.0),
                    ea: o.f("end_alpha", 0.0),
                    sfi: o.f("start_fade_in_time", 0.0),
                    efi: o.f("end_fade_in_time", 0.5),
                    sfo: o.f("start_fade_out_time", 0.5),
                    efo: o.f("end_fade_out_time", 1.0),
                }
            }
            "Alpha Fade Out Random" => Oper::FadeOutRandom { min: o.f("fade out time min", 0.25), max: o.f("fade out time max", 0.25), prop: o.b("proportional 0/1", true) },
            "Alpha Fade In Random" => Oper::FadeInRandom { min: o.f("fade in time min", 0.25), max: o.f("fade in time max", 0.25), prop: o.b("proportional 0/1", true) },
            "Alpha Fade Out Simple" => Oper::FadeOutSimple { t: o.f("proportional fade out time", 0.25) },
            "Alpha Fade In Simple" => Oper::FadeInSimple { t: o.f("proportional fade in time", 0.25) },
            "Color Fade" => Oper::ColorFade { to: col(o.color("color_fade", [255; 4])), st: o.f("fade_start_time", 0.0), et: o.f("fade_end_time", 1.0) },
            "Rotation Spin Roll" => Oper::SpinRoll { rate: o.f("spin_rate_degrees", 0.0).to_radians(), min: o.f("spin_rate_min", 0.0).to_radians(), stop: o.f("spin_stop_time", 0.0) },
            "Oscillate Vector" if o.i("oscillation field", 6) == 6 => Oper::OscillateColor {
                rate: (v3(o.v3("oscillation rate min", [0.0; 3])), v3(o.v3("oscillation rate max", [0.0; 3]))),
                freq: (v3(o.v3("oscillation frequency min", [1.0; 3])), v3(o.v3("oscillation frequency max", [1.0; 3]))),
                mult: o.f("oscillation multiplier", 2.0),
                phase: o.f("oscillation start phase", 0.5),
            },
            "Restart Effect after Duration" => Oper::Restart { min: o.f("Minimum Restart Time", 0.0), max: o.f("Maximum Restart Time", 1.0), kill: o.b("Kill particles on restart", false) },
            "Remap Scalar" => Oper::Remap {
                inp: field(o.i("input field", 8)),
                out: field(o.i("output field", 3)),
                imin: o.f("input minimum", 0.0),
                imax: o.f("input maximum", 1.0),
                omin: o.f("output minimum", 0.0),
                omax: o.f("output maximum", 1.0),
                scale: o.b("output is scalar of initial random range", false),
                abs: o.b("input use absolute value", false),
            },
            "Oscillate Scalar" => Oper::Oscillate {
                field: field(o.i("oscillation field", 7)),
                rate: (o.f("oscillation rate min", 0.0), o.f("oscillation rate max", 0.0)),
                freq: (o.f("oscillation frequency min", 1.0), o.f("oscillation frequency max", 1.0)),
                st: (o.f("start time min", 0.0), o.f("start time max", 0.0)),
                et: (o.f("end time min", 1.0), o.f("end time max", 1.0)),
                prop: o.b("proportional 0/1", true),
                mult: o.f("oscillation multiplier", 2.0),
                phase: o.f("oscillation start phase", 0.5),
            },
            "Remap Distance to Control Point to Scalar" => Oper::DistToCp {
                cp: cpn(o, "control point"),
                out: field(o.i("output field", 3)),
                dmin: o.f("distance minimum", 0.0),
                dmax: o.f("distance maximum", 128.0),
                omin: o.f("output minimum", 0.0),
                omax: o.f("output maximum", 1.0),
                active: o.b("only active within specified distance", false),
                scale_init: o.b("output is scalar of initial random range", false),
                scale_cur: o.b("output is scalar of current value", false),
            },
            "Remap Distance Between Two Control Points to Scalar" => Oper::DistBetweenCps {
                a: cpn(o, "starting control point"),
                b: o.i("ending control point", 1).clamp(0, NCP as i32 - 1) as usize,
                out: field(o.i("output field", 3)),
                dmin: o.f("distance minimum", 0.0),
                dmax: o.f("distance maximum", 128.0),
                omin: o.f("output minimum", 0.0),
                omax: o.f("output maximum", 1.0),
                scale_init: o.b("output is scalar of initial random range", false),
                scale_cur: o.b("output is scalar of current value", false),
            },
            "Ramp Scalar Linear Simple" => Oper::Ramp { field: field(o.i("ramp field", 3)), rate: o.f("ramp rate", 0.0), st: o.f("start time", 0.0), et: o.f("end time", 1.0e9) },
            "Rotation Orient Relative to CP" => Oper::OrientToCp { cp: cpn(o, "Control Point"), offset: o.f("Rotation Offset", 0.0).to_radians(), strength: o.f("Spin Strength", 1.0) },
            "Graph Scalar" => Oper::Graph {
                field: field(o.i("output field", 3)),
                pts: o.points("graph"),
                omin: o.f("output minimum", 0.0),
                omax: o.f("output maximum", 1.0),
                // Respawn's "output op": 1 (the default) scales the initial value, 3 scales
                // the current one (a 0.999..1.02 pulse on the heat shield fan, 1..4 growth on
                // smoke); 0 sets it outright. Fields reset to their base each frame, so both
                // scalings are a multiply here.
                mul: matches!(o.i("output op", 1), 1 | 3),
                // 2 adds to the value: the A-Wall's models flash (alpha + 10 x a 0.3 s curve
                // that ends at 0) and then stay at their base alpha.
                add: o.i("output op", 1) == 2,
                lifespans: o.b("graph time is in lifespans", true),
                time: o.f("graph time", 1.0).max(1e-3),
                looped: o.b("graph loop", false),
            },
            "Lerp Initial Scalar" => Oper::LerpInitial { field: field(o.i("output field", 3)), to: o.f("value to lerp to", 1.0), st: o.f("start time", 0.0), et: o.f("end time", 1.0) },
            "Movement Lock to Control Point" => Oper::LockToCp { cp: cpn(o, "control_point_number") },
            "Movement Max Velocity" => Oper::MaxVelocity { max: o.f("Maximum Velocity", 0.0) },
            // Source's default for this one is control point 1 (the heat shield's refraction
            // child fades by the distance between CP 1 = the player and the shield).
            "Set Control Point To Player" => Oper::CpToPlayer { cp: o.i("Control Point Number", 1).clamp(0, NCP as i32 - 1) as usize },
            "Set Control Point Positions" => {
                let mut sets = Vec::new();
                for (num, loc, d) in [
                    ("First Control Point Number", "First Control Point Location", 1),
                    ("Second Control Point Number", "Second Control Point Location", 2),
                    ("Third Control Point Number", "Third Control Point Location", 3),
                    ("Fourth Control Point Number", "Fourth Control Point Location", 4),
                ] {
                    let n = o.i(num, d).clamp(0, NCP as i32 - 1) as usize;
                    sets.push((n, v3(o.v3(loc, [128.0 * d as f32, 0.0, 0.0]))));
                }
                Oper::SetCps { base: cpn(o, "Control Point to offset positions from"), sets, world: o.b("Set positions in world space", false) }
            }
            _ => continue,
        };
        oper.push(op);
        oper_cap.push(o.i("operator end cap state", -1) as i8);
    }
    let mut force = Vec::new();
    for o in &def.forces {
        force.push(match o.function.as_str() {
            "Pull towards control point" => Force::Pull { cp: cpn(o, "control point number"), amount: o.f("amount of force", 100.0), falloff: o.f("falloff power", 2.0) },
            "random force" => Force::Random { min: v3(o.v3("min force", [0.0; 3])), max: v3(o.v3("max force", [0.0; 3])) },
            "turbulent force" => Force::Turbulent { amount: o.f("Noise amount 0", 1.0) + o.f("Noise amount 1", 0.5) + o.f("Noise amount 2", 0.25) + o.f("Noise amount 3", 0.125) },
            "twist around axis" => Force::Twist { cp: cpn(o, "control point"), amount: o.f("amount of force", 0.0), axis: v3(o.v3("twist axis", [0.0, 0.0, 1.0])), local: o.b("object local space axis 0/1", false) },
            _ => continue,
        });
    }
    let mut cons = Vec::new();
    for o in &def.constraints {
        cons.push(match o.function.as_str() {
            "Collision via traces" => Constraint::Collide { bounce: o.f("amount of bounce", 0.0), slide: o.f("amount of slide", 0.0), kill_speed: o.f("minimum speed to kill on collision", -1.0) },
            "Constrain distance to control point" => Constraint::Distance { cp: cpn(o, "control point number"), min: o.f("minimum distance", 0.0), max: o.f("maximum distance", 100.0) },
            _ => continue,
        });
    }
    let mut render = Vec::new();
    for o in &def.renderers {
        render.push(match o.function.as_str() {
            "render_animated_sprites" | "render_screen_velocity_rotate" => Render::Sprites { rate: o.f("animation rate", 0.1), fit: o.b("animation_fit_lifetime", false), orient: o.i("orientation_type", -1) },
            "render_sprite_trail" => {
                let t = o.v3("tail color and alpha scale factor", [1.0; 3]);
                let tail = match o.params.get("tail color and alpha scale factor") {
                    Some(tf_assets::pcf::Value::Vec4(v)) => *v,
                    _ => [t[0], t[1], t[2], 1.0],
                };
                Render::Trail { rate: o.f("animation rate", 0.1), min: o.f("min length", 0.0), max: o.f("max length", 2000.0), fade_in: o.f("length fade in time", 0.0), tail }
            }
            "render_rope" => Render::Rope,
            "Render light source" => Render::Light { scale: o.f("color scale", 1.0), radius: o.f("radius scale", 1.0), by_alpha: o.b("color scale by alpha", true) },
            "Render models" => {
                let Some(m) = o.s("sequence 0 model").filter(|m| !m.is_empty()) else { continue };
                Render::Models {
                    model: format!("models/{}", m.replace('\\', "/").to_ascii_lowercase()),
                    pitch: o.f("Model-space pitch degrees", 0.0).to_radians(),
                    yaw: o.f("Model-space yaw degrees", 0.0).to_radians(),
                    roll: o.f("Model-space roll degrees", 0.0).to_radians(),
                    orient_normal: o.b("orient model z to normal", false),
                }
            }
            "Emit Decal" => {
                // `..._subrect` names an atlas rectangle of the material without the suffix.
                let materials: Vec<String> = (0..4)
                    .filter_map(|i| o.s(&format!("material {i}")))
                    .filter(|m| !m.is_empty())
                    .map(|m| m.trim_end_matches(".vmt").trim_end_matches("_subrect").to_string())
                    .collect();
                if materials.is_empty() {
                    continue;
                }
                Render::Decal { materials }
            }
            _ => continue,
        });
    }
    let children = def.children.iter().filter_map(|c| by_name.get(&c.name.to_ascii_lowercase()).map(|&i| (i, c.delay, c.end_cap))).collect();
    let hdr = def.hdr_scale.max(0.0);
    Compiled {
        material: def.material.clone(),
        max: def.max_particles.min(4000),
        initial: def.initial_particles,
        radius: def.radius,
        color: col(def.color) * hdr,
        hdr,
        alpha: def.color[3] as f32 / 255.0,
        emit,
        init,
        init_cap,
        oper,
        oper_cap,
        force,
        cons,
        render,
        children,
        decays,
    }
}

// ---------------------------------------------------------------------------------------------
// Library (parsed on a worker thread at startup)

pub struct Library {
    defs: Vec<SystemDef>,
    by_name: HashMap<String, usize>,
    compiled: Vec<Option<Arc<Compiled>>>,
    frontend: Option<Arc<Vpk>>,
}

impl Library {
    fn load(game: std::path::PathBuf) -> Library {
        let t = std::time::Instant::now();
        let path = game.join("vpk/englishclient_frontend.bsp.pak000_dir.vpk");
        let vpk = match Vpk::open(&path) {
            Ok(v) => Arc::new(v),
            Err(e) => {
                log::warn!("particles: {}: {e:#}", path.display());
                return Library { defs: Vec::new(), by_name: HashMap::new(), compiled: Vec::new(), frontend: None };
            }
        };
        let mut files: Vec<String> = vpk.entries.keys().filter(|p| p.ends_with(".pcf")).cloned().collect();
        files.sort();
        let parsed = crate::world::parallel_map(&files, |f| vpk.read(f).ok().and_then(|b| tf_assets::pcf::parse_pcf(&b).map_err(|e| log::warn!("{f}: {e:#}")).ok()));
        let mut defs = Vec::new();
        let mut by_name = HashMap::new();
        for d in parsed.into_iter().flatten().flatten().flatten() {
            by_name.entry(d.name.to_ascii_lowercase()).or_insert(defs.len());
            defs.push(d);
        }
        log::info!("particles: {} systems from {} files in {:?}", defs.len(), files.len(), t.elapsed());
        let n = defs.len();
        Library { defs, by_name, compiled: vec![None; n], frontend: Some(vpk) }
    }
    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }
    fn compiled(&mut self, i: usize) -> Arc<Compiled> {
        if self.compiled[i].is_none() {
            self.compiled[i] = Some(Arc::new(compile(&self.defs[i], &self.by_name)));
        }
        self.compiled[i].clone().unwrap()
    }
}

// ---------------------------------------------------------------------------------------------
// Instances

#[derive(Clone, Copy, Default)]
struct P {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    radius: f32,
    base_radius: f32,
    color: Vec3,
    base_color: Vec3,
    alpha: f32,
    base_alpha: f32,
    trail: f32,
    base_trail: f32,
    rot: f32,
    spin: f32,
    seq: u16,
    flip: bool,
    /// Per-particle random in 0..1 (random fade times).
    r: f32,
    id: u32,
    dead: bool,
}

struct Node {
    sys: Arc<Compiled>,
    delay: f32,
    age: f32,
    started: bool,
    instant_done: Vec<bool>,
    acc: Vec<f32>,
    dist_acc: f32,
    particles: Vec<P>,
    next_id: u32,
    layer: Option<usize>,
    /// Model-renderer instances (one entity per particle per Render::Models, pooled).
    models: Vec<Entity>,
    /// The node's decal has been laid.
    decal_done: bool,
    /// The parent system's node (for `Position From Parent Particles`).
    parent: Option<usize>,
    /// Node age at which `Restart Effect after Duration` restarts it (0: not chosen yet).
    restart_at: f32,
}

struct Effect {
    id: u64,
    nodes: Vec<Node>,
    cps: [Cp; NCP],
    cp_prev: [Vec3; NCP],
    cp_vel: [Vec3; NCP],
    age: f32,
    stop_at: f32,
    stopped: bool,
    /// End-cap-only initializers have been re-run on the living particles.
    capped: bool,
    attached: Option<Entity>,
    /// End-cap children to start when the effect is stopped.
    end_caps: Vec<(usize, f32)>,
    /// A first-person system (size fade instead of the near fade).
    vm: bool,
    /// Drawn by the viewmodel camera (cockpit effects: on top of the cockpit model).
    overlay: bool,
}

fn new_node(sys: Arc<Compiled>, delay: f32) -> Node {
    let n = sys.emit.len();
    Node { sys, delay, age: 0.0, started: false, instant_done: vec![false; n], acc: vec![0.0; n], dist_acc: 0.0, particles: Vec::new(), next_id: 0, layer: None, models: Vec::new(), decal_done: false, parent: None, restart_at: 0.0 }
}

/// The system tree under `root`, with each child's delay added to its parent's.
fn collect_nodes(lib: &mut Library, root: usize, delay: f32, depth: u32, out: &mut Vec<Node>, end_caps: &mut Vec<(usize, f32)>) {
    collect_under(lib, root, delay, depth, None, out, end_caps);
}

fn collect_under(lib: &mut Library, root: usize, delay: f32, depth: u32, parent: Option<usize>, out: &mut Vec<Node>, end_caps: &mut Vec<(usize, f32)>) {
    let sys = lib.compiled(root);
    let children = sys.children.clone();
    let me = out.len();
    out.push(Node { parent, ..new_node(sys, delay) });
    if depth > 6 {
        return;
    }
    for (c, d, end_cap) in children {
        if end_cap {
            end_caps.push((c, d));
        } else {
            collect_under(lib, c, delay + d, depth + 1, Some(me), out, end_caps);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Materials

struct Layer {
    key: String,
    additive: bool,
    gain: f32,
    orient: i32,
    sheet: Vec<Sequence>,
    mesh: Handle<Mesh>,
    entity: Entity,
}

/// Parse a VMT's `$key value` pairs (lowercased keys) and its shader name.
fn parse_vmt(text: &str) -> (String, HashMap<String, String>) {
    let mut shader = String::new();
    let mut kv = HashMap::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() || line == "{" || line == "}" {
            continue;
        }
        let toks: Vec<String> = {
            let mut out = Vec::new();
            let mut cur = String::new();
            let mut quoted = false;
            for c in line.chars() {
                match c {
                    '"' => {
                        if quoted {
                            out.push(std::mem::take(&mut cur));
                        }
                        quoted = !quoted;
                    }
                    c if c.is_whitespace() && !quoted => {
                        if !cur.is_empty() {
                            out.push(std::mem::take(&mut cur));
                        }
                    }
                    c => cur.push(c),
                }
            }
            if !cur.is_empty() {
                out.push(cur);
            }
            out
        };
        match toks.as_slice() {
            [s] if shader.is_empty() => shader = s.to_ascii_lowercase(),
            [k, v, ..] => {
                kv.insert(k.to_ascii_lowercase(), v.clone());
            }
            _ => {}
        }
    }
    (shader, kv)
}

// ---------------------------------------------------------------------------------------------
// Resource and systems

#[derive(Resource)]
pub struct Pfx {
    lib: Option<Library>,
    rx: Mutex<Option<mpsc::Receiver<Library>>>,
    /// Materials precached by the loader thread (see `precache`).
    pre_rx: Mutex<Option<mpsc::Receiver<Vec<(String, Option<(vtf::Vtf, HashMap<String, String>)>)>>>>,
    effects: Vec<Effect>,
    layers: Vec<Layer>,
    layer_of: HashMap<String, Option<usize>>,
    /// Materials decoded ahead (in parallel) for this frame's new layers.
    decoded: HashMap<String, Option<(vtf::Vtf, HashMap<String, String>)>>,
    impacts: HashMap<String, ImpactTable>,
    lights: Vec<Entity>,
    /// Generic effects (particles::Effect) waiting for the library to resolve them.
    pub(crate) pending: Vec<crate::particles::Effect>,
    rng: u64,
    next_id: u64,
    total: usize,
    /// Model-renderer models by path (None: failed to load).
    model_cache: HashMap<String, Option<Arc<Vec<(Handle<Mesh>, FxMat)>>>>,
    /// Decal materials by name (None: not found) and the decals laid, oldest first.
    decal_mats: HashMap<String, Option<Handle<StandardMaterial>>>,
    decals: std::collections::VecDeque<Entity>,
    decal_mesh: Option<Handle<Mesh>>,
}

impl Pfx {
    pub fn ready(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| !l.defs.is_empty())
    }
    fn rand(&mut self) -> f32 {
        rand(&mut self.rng)
    }
    fn spawn(&mut self, name: &str, cps: &[Cp], stop_after: Option<f32>, attached: Option<Entity>) -> Option<u64> {
        let lib = self.lib.as_mut()?;
        let root = lib.find(name)?;
        let mut nodes = Vec::new();
        let mut end_caps = Vec::new();
        collect_nodes(lib, root, 0.0, 0, &mut nodes, &mut end_caps);
        let mut all = [Cp::default(); NCP];
        for (i, c) in all.iter_mut().enumerate() {
            *c = cps.get(i).copied().unwrap_or_else(|| cps.first().copied().unwrap_or_default());
        }
        self.next_id += 1;
        let id = self.next_id;
        let stop_at = stop_after.unwrap_or(if attached.is_some() { f32::INFINITY } else { DEFAULT_STOP });
        self.effects.push(Effect { id, nodes, cps: all, cp_prev: all.map(|c| c.pos), cp_vel: [Vec3::ZERO; NCP], age: 0.0, stop_at, stopped: false, capped: false, attached, end_caps, vm: false, overlay: false });
        Some(id)
    }

    /// The effect names of an impact table for a surface (concrete when the table has none for
    /// it), one per FX block.
    fn impact_names(&mut self, gd: &crate::gamedata::GameData, table: &str, surface: &str) -> Vec<String> {
        let key = table.to_ascii_lowercase();
        if !self.impacts.contains_key(&key) {
            let path = format!("scripts/impacts/{key}.txt");
            let text = gd
                .read_file(&path)
                .ok()
                .or_else(|| self.lib.as_ref().and_then(|l| l.frontend.as_ref()).and_then(|v| v.read(&path).ok()))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            self.impacts.insert(key.clone(), parse_impact_table(&text));
        }
        self.impacts[&key]
            .fx
            .iter()
            .filter_map(|block| block.get(surface).or_else(|| block.get("C")).or_else(|| block.get("M")).cloned())
            .filter(|n| !n.eq_ignore_ascii_case("none") && !n.is_empty())
            .collect()
    }

    /// The effect names of an impact table for what a round struck: each FX block's most
    /// specific entry for the surface; a hit on the player takes the FX_victim entry for the
    /// exact surface instead when the table has one.
    fn hit_names(&mut self, gd: &crate::gamedata::GameData, table: &str, surface: crate::particles::Surface, victim: bool) -> Vec<String> {
        let _ = self.impact_names(gd, table, "C");
        let t = &self.impacts[&table.to_ascii_lowercase()];
        let keys = surface.keys();
        let mut out = Vec::new();
        if victim {
            for block in &t.victim {
                if let Some(n) = block.get(keys[0]) {
                    out.push(n.clone());
                }
            }
        }
        if out.is_empty() {
            for block in &t.fx {
                if let Some(n) = keys.iter().find_map(|k| block.get(*k)) {
                    out.push(n.clone());
                }
            }
        }
        out.retain(|n| !n.eq_ignore_ascii_case("none") && !n.is_empty());
        out
    }

    /// Play a generic effect with the game's systems; false if they aren't available.
    fn spawn_generic(&mut self, gd: &crate::gamedata::GameData, e: crate::particles::Effect) -> bool {
        use crate::particles::Effect as E;
        match e {
            E::Muzzle { at, dir, scale, energy } => {
                let name = match (scale >= 1.5, scale < 2.5, energy) {
                    (true, _, true) => "P_muz_TPA",
                    (true, true, false) => "wpn_muzzleflash_xo_FP",
                    (true, false, false) => "wpn_muzzleflash_xo",
                    (false, _, true) => "wpn_muzzleflash_smg_elec_FP",
                    (false, _, false) => "wpn_muzzleflash_smg_FP",
                };
                self.spawn(name, &[Cp::facing(to_game(at), dir_to_game(dir))], None, None).is_some()
            }
            E::Tracer { from, to, width, color } => {
                let name = if color.z > color.x * 1.2 {
                    "P_wpn_tracer_xo16_elec"
                } else if width >= 0.1 {
                    "weapon_tracers_xo16"
                } else {
                    "P_wpn_tracer"
                };
                let (a, b) = (to_game(from), to_game(to));
                let f = Cp::facing(a, b - a);
                self.spawn(name, &[f, Cp { pos: b, ..f }], None, None).is_some()
            }
            E::Impact { at, normal, scale, energy } => {
                let table = if energy {
                    "titan_particle_accelerator"
                } else if scale >= 1.5 {
                    "titan_bullet"
                } else {
                    "default"
                };
                self.spawn_table(gd, table, "C", Cp::on_surface(to_game(at), dir_to_game(normal)))
            }
            E::Hit { at, normal, table, surface, victim, .. } => {
                let cp = Cp::on_surface(to_game(at), dir_to_game(normal));
                let names = self.hit_names(gd, table, surface, victim);
                log::debug!("hit {table} {surface:?} victim {victim}: {names:?}");
                let mut any = false;
                for n in names {
                    let id = self.spawn(&n, &[cp, cp], None, None);
                    // Holes only go on level geometry: on a Titan, a body or a shield they
                    // would hang in the air once it moves.
                    if let (Some(id), false) = (id, surface == crate::particles::Surface::World) {
                        if let Some(fx) = self.effects.iter_mut().find(|f| f.id == id) {
                            fx.nodes.iter_mut().for_each(|n| n.decal_done = true);
                        }
                    }
                    any |= id.is_some();
                }
                any
            }
            E::Explosion { at, scale } => {
                let table = if scale < 0.7 {
                    "exp_small"
                } else if scale < 1.3 {
                    "exp_rocket_shoulder"
                } else {
                    "exp_xlarge"
                };
                self.spawn_table(gd, table, "C", Cp::on_surface(to_game(at), Vec3::Z))
            }
            E::TitanDeath { at } => self.spawn("xo_exp_death", &[Cp::on_surface(to_game(at), Vec3::Z)], None, None).is_some(),
            E::DustRing { at, .. } => self.spawn_table(gd, "titan_landing", "D", Cp::on_surface(to_game(at), Vec3::Z)),
            E::TrailPuff { .. } => false,
        }
    }

    fn spawn_table(&mut self, gd: &crate::gamedata::GameData, table: &str, surface: &str, cp: Cp) -> bool {
        let names = self.impact_names(gd, table, surface);
        let mut any = false;
        for n in names {
            any |= self.spawn(&n, &[cp, cp], None, None).is_some();
        }
        any
    }
}

/// An impact table's effect blocks: `FX` (everyone sees them, one map per block) and
/// `FX_victim` (what the hit player sees instead, where present).
#[derive(Default, Debug)]
struct ImpactTable {
    fx: Vec<HashMap<String, String>>,
    victim: Vec<HashMap<String, String>>,
}

/// `ImpactTable { FX { "C" "impact_concrete" ... } FX { ... } FX_victim { ... } }`.
fn parse_impact_table(text: &str) -> ImpactTable {
    let mut out = ImpactTable::default();
    let mut cur: Option<HashMap<String, String>> = None;
    let mut depth = 0;
    let mut pending_fx = 0;
    let mut cur_victim = false;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("FX") {
            pending_fx = 1;
            continue;
        }
        if line.eq_ignore_ascii_case("FX_victim") {
            pending_fx = 2;
            continue;
        }
        if line.starts_with('{') {
            depth += 1;
            if pending_fx > 0 {
                cur = Some(HashMap::new());
                cur_victim = pending_fx == 2;
                pending_fx = 0;
            }
            continue;
        }
        if line.starts_with('}') {
            depth -= 1;
            if let Some(m) = cur.take() {
                if cur_victim {
                    out.victim.push(m);
                } else {
                    out.fx.push(m);
                }
            }
            continue;
        }
        pending_fx = 0;
        if let Some(m) = cur.as_mut() {
            let parts: Vec<&str> = line.split('"').filter(|s| !s.trim().is_empty()).collect();
            if parts.len() >= 2 {
                m.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }
    let _ = depth;
    out
}

fn rand(rng: &mut u64) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng >> 40) as f32 / (1u64 << 24) as f32
}

fn rexp(rng: &mut u64, a: f32, b: f32, exp: f32) -> f32 {
    let r = rand(rng);
    let r = if (exp - 1.0).abs() > 1e-4 && exp > 0.0 { r.powf(exp) } else { r };
    a + (b - a) * r
}

fn rvec(rng: &mut u64, a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + (b.x - a.x) * rand(rng), a.y + (b.y - a.y) * rand(rng), a.z + (b.z - a.z) * rand(rng))
}

fn unit(rng: &mut u64) -> Vec3 {
    for _ in 0..16 {
        let v = Vec3::new(rand(rng) * 2.0 - 1.0, rand(rng) * 2.0 - 1.0, rand(rng) * 2.0 - 1.0);
        let l = v.length_squared();
        if l > 1e-4 && l <= 1.0 {
            return v / l.sqrt();
        }
    }
    Vec3::Z
}

fn graph(pts: &[[f32; 2]], x: f32) -> f32 {
    match pts {
        [] => 1.0,
        [p] => p[1],
        _ => {
            if x <= pts[0][0] {
                return pts[0][1];
            }
            for w in pts.windows(2) {
                if x <= w[1][0] {
                    let k = ((x - w[0][0]) / (w[1][0] - w[0][0]).max(1e-6)).clamp(0.0, 1.0);
                    return w[0][1] + (w[1][1] - w[0][1]) * k;
                }
            }
            pts[pts.len() - 1][1]
        }
    }
}

/// Source's Bias(): 0.5 is linear.
fn bias(x: f32, b: f32) -> f32 {
    let b = b.clamp(1e-3, 0.999);
    x.powf(b.ln() / 0.5f32.ln())
}

fn frac(x: f32, a: f32, b: f32) -> f32 {
    if b <= a {
        if x >= a {
            1.0
        } else {
            0.0
        }
    } else {
        ((x - a) / (b - a)).clamp(0.0, 1.0)
    }
}

/// Make particle `p` from the node's initializers.
/// Whether an initializer/operator with this end cap state runs now.
fn cap_active(cap: i8, emitting: bool) -> bool {
    match cap {
        0 => emitting,
        1 => !emitting,
        _ => true,
    }
}

/// Run a system's initializers on a particle. `recap` re-runs only the end-cap-only ones
/// (state 1) on a living particle when its effect is stopped, the way the gun shield picks up
/// its 5 s lifetime and fade only once it is dropped.
#[allow(clippy::too_many_arguments)]
fn apply_inits(sys: &Compiled, p: &mut P, cps: &[Cp; NCP], cp_vel: &[Vec3; NCP], parent: &[(Vec3, Vec3)], sheet_len: &dyn Fn(u16) -> f32, rng: &mut u64, index: u32, emitting: bool, recap: bool) {
    let mut had_life = false;
    for (i, cap) in sys.init.iter().zip(&sys.init_cap) {
        if if recap { *cap != 1 } else { !cap_active(*cap, emitting) } {
            continue;
        }
        match *i {
            Init::Lifetime { min, max, exp } => {
                p.life = rexp(rng, min, max, exp);
                had_life = true;
            }
            Init::LifetimeFromSequence { fps } => {
                p.life = sheet_len(p.seq) / fps.max(1e-3);
                had_life = true;
            }
            Init::Radius { min, max, exp } => p.base_radius = rexp(rng, min, max, exp),
            Init::Alpha { min, max, exp } => p.base_alpha = rexp(rng, min, max, exp),
            Init::Color { c1, c2 } => p.base_color = c1.lerp(c2, rand(rng)) * sys.hdr,
            Init::Sphere { cp, dmin, dmax, bias, abs, local_bias, smin, smax, sexp, lmin, lmax } => {
                let c = &cps[cp];
                let mut d = unit(rng) * bias;
                for k in 0..3 {
                    if abs[k] != 0.0 {
                        d[k] = d[k].abs();
                    }
                }
                let d = if local_bias { c.local(d) } else { d };
                let dist = dmin + (dmax - dmin) * rand(rng);
                p.pos = c.pos + d * dist;
                let speed = rexp(rng, smin, smax, sexp);
                p.vel = d.normalize_or_zero() * speed + c.local(rvec(rng, lmin, lmax));
            }
            Init::Box { cp, min, max } => p.pos = cps[cp].pos + rvec(rng, min, max),
            Init::Ring { cp, radius, thickness, smin, smax, xy } => {
                let c = &cps[cp];
                let a = rand(rng) * std::f32::consts::TAU;
                let r = radius + (rand(rng) - 0.5) * thickness;
                let out = c.fwd * a.cos() + c.left * a.sin();
                p.pos = c.pos + out * r;
                let s = smin + (smax - smin) * rand(rng);
                p.vel = if xy { out * s } else { (out + c.up * (rand(rng) - 0.5)).normalize_or_zero() * s };
            }
            Init::Offset { cp, min, max, local, prop } => {
                let mut o = rvec(rng, min, max);
                if prop {
                    o *= p.base_radius;
                }
                p.pos += if local { cps[cp].local(o) } else { o };
            }
            Init::Warp { cp, min, max } => {
                let k = rvec(rng, min, max);
                let c = &cps[cp];
                let d = p.pos - c.pos;
                let (x, y, z) = (d.dot(c.fwd) * k.x, d.dot(c.left) * k.y, d.dot(c.up) * k.z);
                p.pos = c.pos + c.fwd * x + c.left * y + c.up * z;
            }
            Init::Velocity { cp, lmin, lmax, rmin, rmax } => {
                p.vel += cps[cp].local(rvec(rng, lmin, lmax)) + unit(rng) * (rmin + (rmax - rmin) * rand(rng));
            }
            Init::VelocityNoise { cp, min, max, local } => {
                let v = rvec(rng, min, max);
                p.vel += if local { cps[cp].local(v) } else { v };
            }
            Init::InheritCpVelocity { cp, scale } => p.vel += cp_vel[cp] * scale,
            Init::Rotation { initial, min, max, flip } => {
                let mut r = (initial + min + (max - min) * rand(rng)).to_radians();
                if flip && rand(rng) < 0.5 {
                    r = -r;
                }
                p.rot = r;
            }
            Init::RotationSpeed { constant, min, max, flip } => {
                let mut s = (constant + min + (max - min) * rand(rng)).to_radians();
                if flip && rand(rng) < 0.5 {
                    s = -s;
                }
                p.spin = s;
            }
            Init::YawFlip { pct } => p.flip = rand(rng) < pct,
            Init::Sequence { min, max } => {
                let (a, b) = (min.min(max), min.max(max));
                p.seq = (a + ((rand(rng) * (b - a + 1) as f32) as i32).min(b - a)).max(0) as u16;
            }
            Init::Trail { min, max, exp } => p.base_trail = rexp(rng, min, max, exp),
            Init::FromParent { scale } => {
                if parent.is_empty() {
                    p.dead = true;
                } else {
                    let (pos, vel) = parent[((rand(rng) * parent.len() as f32) as usize).min(parent.len() - 1)];
                    p.pos = pos;
                    p.vel += vel * scale;
                }
            }
            Init::Remap { inp, out, imin, imax, omin, omax, scale, active } => {
                let x = match inp {
                    Field::Radius => p.base_radius,
                    Field::Alpha => p.base_alpha,
                    Field::Id => p.id as f32,
                    Field::Lifetime => p.life,
                    _ => 0.0,
                };
                let (lo, hi) = (imin.min(imax), imin.max(imax));
                if !(active && (x < lo || x > hi)) {
                    let v = remap(x, imin, imax, omin, omax);
                    let mul = scale || matches!(out, Field::Alpha2);
                    match out {
                        Field::Radius => p.base_radius = if mul { p.base_radius * v } else { v },
                        Field::Alpha => p.base_alpha = if mul { p.base_alpha * v } else { v },
                        Field::Alpha2 => p.base_alpha *= v.max(0.0),
                        Field::Trail => p.base_trail = if mul { p.base_trail * v } else { v },
                        Field::Roll => p.rot = if mul { p.rot * v } else { v },
                        Field::RollSpeed => p.spin = if mul { p.spin * v } else { v },
                        Field::Lifetime => {
                            p.life = if mul { p.life * v } else { v };
                            had_life = true;
                        }
                        _ => {}
                    }
                }
            }
            Init::Scalar { field: f, min, max, exp } => {
                let v = rexp(rng, min, max, exp);
                match f {
                    Field::Radius => p.base_radius = v,
                    Field::Alpha => p.base_alpha = v,
                    Field::Trail => p.base_trail = v,
                    Field::Roll => p.rot = v,
                    Field::RollSpeed => p.spin = v,
                    Field::Lifetime => {
                        p.life = v;
                        had_life = true;
                    }
                    _ => {}
                }
            }
            Init::ColorVector { min, max } => p.base_color = rvec(rng, min, max) * sys.hdr,
            Init::Between { end, smin, smax, spread } => {
                let target = cps[end].pos + unit(rng) * spread;
                let d = target - p.pos;
                let s = (smin + (smax - smin) * rand(rng)).max(1.0);
                p.vel = d.normalize_or(cps[0].fwd) * s;
                p.life = d.length() / s;
                had_life = true;
            }
            Init::Path { start, end, sequential, count } => {
                let k = if sequential { (index as f32 % count) / count } else { rand(rng) };
                p.pos = cps[start].pos.lerp(cps[end].pos, k);
            }
        }
    }
    let _ = had_life;
}

fn init_particle(sys: &Compiled, cps: &[Cp; NCP], cp_vel: &[Vec3; NCP], parent: &[(Vec3, Vec3)], sheet_len: &dyn Fn(u16) -> f32, rng: &mut u64, index: u32, emitting: bool) -> P {
    let mut p = P {
        pos: cps[0].pos,
        radius: sys.radius,
        base_radius: sys.radius,
        color: sys.color,
        base_color: sys.color,
        alpha: sys.alpha,
        base_alpha: sys.alpha,
        trail: 0.1,
        base_trail: 0.1,
        life: 1.0,
        r: rand(rng),
        id: index,
        ..Default::default()
    };
    apply_inits(sys, &mut p, cps, cp_vel, parent, sheet_len, rng, index, emitting, false);
    p.radius = p.base_radius;
    p.alpha = p.base_alpha;
    p.color = p.base_color;
    p.trail = p.base_trail;
    p
}

fn set_field(p: &mut P, f: Field, v: f32, mul: bool) {
    let apply = |cur: &mut f32| *cur = if mul { *cur * v } else { v };
    match f {
        Field::Radius => apply(&mut p.radius),
        Field::Alpha => apply(&mut p.alpha),
        Field::Trail => apply(&mut p.trail),
        Field::Roll => apply(&mut p.rot),
        // The second alpha (Respawn's renderers post-multiply it, or erode by it): a
        // multiplier on alpha here, starting from 1.
        Field::Alpha2 if !no_alpha2() => p.alpha *= v.max(0.0),
        _ => {}
    }
}

/// A rotation taking model Z to `n` with model X as close to world down as it can be.
fn normal_basis(n: Vec3) -> Quat {
    let z = n.normalize_or(Vec3::Z);
    let down = -Vec3::Z;
    let x = (down - z * down.dot(z)).try_normalize().unwrap_or_else(|| (Vec3::X - z * Vec3::X.dot(z)).normalize());
    Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z)).normalize()
}

fn get_field(p: &P, f: Field) -> f32 {
    match f {
        Field::Radius => p.radius,
        Field::Alpha => p.alpha,
        Field::Trail => p.trail,
        Field::Roll => p.rot,
        Field::Alpha2 => 1.0,
        Field::Lifetime => p.life,
        Field::Age => p.age,
        Field::Id => p.id as f32,
        _ => 0.0,
    }
}

fn get_base(p: &P, f: Field) -> f32 {
    match f {
        Field::Radius => p.base_radius,
        Field::Alpha => p.base_alpha,
        Field::Trail => p.base_trail,
        Field::Roll => p.rot,
        Field::Alpha2 => 1.0,
        Field::Lifetime => p.life,
        Field::Age => p.age,
        Field::Id => p.id as f32,
        _ => 0.0,
    }
}

/// TF_PFX_NO_ALPHA2=1 ignores the second alpha (for comparing).
fn no_alpha2() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("TF_PFX_NO_ALPHA2").is_some())
}

/// Source's RemapValClamped.
fn remap(v: f32, imin: f32, imax: f32, omin: f32, omax: f32) -> f32 {
    let k = if (imax - imin).abs() < 1e-6 { if v >= imax { 1.0 } else { 0.0 } } else { ((v - imin) / (imax - imin)).clamp(0.0, 1.0) };
    omin + (omax - omin) * k
}

/// Write a remapped value: scaling the field's initial value, its current one, or outright.
fn put(p: &mut P, f: Field, v: f32, scale_init: bool, scale_cur: bool) {
    if scale_init || scale_cur || matches!(f, Field::Alpha2) {
        set_field(p, f, v, true);
    } else {
        set_field(p, f, v, false);
    }
}

/// Advance one node by `dt`.
#[allow(clippy::too_many_arguments)]
fn step_node(
    n: &mut Node,
    cps: &mut [Cp; NCP],
    cp_delta: &[Vec3; NCP],
    cp_vel: &[Vec3; NCP],
    emitting: bool,
    dt: f32,
    rng: &mut u64,
    world: Option<&crate::player::Collision>,
    budget: &mut usize,
    sheet_len: &dyn Fn(u16) -> f32,
    player: Vec3,
    parent: &[(Vec3, Vec3)],
) {
    if n.delay > 0.0 {
        n.delay -= dt;
        return;
    }
    let sys = n.sys.clone();
    n.age += dt;
    for o in &sys.oper {
        if let Oper::Restart { min, max, kill } = *o {
            if n.restart_at <= 0.0 {
                n.restart_at = (min + (max - min).max(0.0) * rand(rng)).max(0.01);
            }
            if emitting && n.age >= n.restart_at {
                n.age = 0.0;
                n.instant_done.iter_mut().for_each(|d| *d = false);
                n.acc.iter_mut().for_each(|a| *a = 0.0);
                if kill {
                    n.particles.clear();
                }
                n.restart_at = (min + (max - min).max(0.0) * rand(rng)).max(0.01);
            }
        }
    }
    let t0 = (n.age - dt).max(0.0);
    let old_len = n.particles.len();
    // Operators that move control points run first so particles born this frame read the
    // points where they are now (the heat shield fan sits on CP 10, set from CP 0).
    for o in &sys.oper {
        if let Oper::CpToPlayer { cp } = o {
            cps[*cp].pos = player; // (already applied before the deltas; kept for new effects)
        }
        if let Oper::SetCps { base, sets, world } = o {
            let b = cps[*base];
            for &(i, off) in sets {
                if i != *base {
                    cps[i] = Cp { pos: if *world { off } else { b.pos + b.local(off) }, ..b };
                }
            }
        }
    }
    // Emit.
    let mut new = 0u32;
    if !n.started {
        n.started = true;
        new += sys.initial as u32;
    }
    if emitting {
        for (k, e) in sys.emit.iter().enumerate() {
            match *e {
                Emit::Instant { count, min, start } => {
                    if !n.instant_done[k] && n.age >= start {
                        n.instant_done[k] = true;
                        let c = if min != u32::MAX && min < count { min + (rand(rng) * (count - min + 1) as f32) as u32 } else { count };
                        new += c;
                    }
                }
                Emit::Continuous { rate, duration, start } => {
                    let end = if duration > 0.0 { start + duration } else { f32::INFINITY };
                    let a = t0.max(start);
                    let b = n.age.min(end);
                    if b > a {
                        n.acc[k] += (b - a) * rate;
                        let c = n.acc[k].floor();
                        n.acc[k] -= c;
                        new += c as u32;
                    }
                }
                Emit::Distance { spacing } => {
                    n.dist_acc += cp_delta[0].length();
                    let c = (n.dist_acc / spacing).floor();
                    n.dist_acc -= c * spacing;
                    new += c as u32;
                }
            }
        }
    }
    let room = sys.max.saturating_sub(n.particles.len()).min(*budget);
    let new = (new as usize).min(room);
    *budget -= new;
    for _ in 0..new {
        let id = n.next_id;
        n.next_id += 1;
        let p = init_particle(&sys, cps, cp_vel, parent, sheet_len, rng, id, emitting);
        n.particles.push(p);
    }
    // Simulate.
    for (pi, p) in n.particles.iter_mut().enumerate() {
        // Particles born this frame were placed at the control point's current position, so
        // the frame's control-point delta is not theirs to follow.
        let newborn = pi >= old_len;
        p.age += dt;
        let t = if p.life > 0.0 { (p.age / p.life).clamp(0.0, 1.0) } else { 0.0 };
        p.radius = p.base_radius;
        p.alpha = p.base_alpha;
        p.color = p.base_color;
        p.trail = p.base_trail;
        let mut accel = Vec3::ZERO;
        for f in &sys.force {
            match *f {
                Force::Pull { cp, amount, falloff } => {
                    let d = cps[cp].pos - p.pos;
                    let l = d.length().max(1.0);
                    accel += d / l * amount / l.powf(falloff).max(1e-3);
                }
                Force::Random { min, max } => accel += rvec(rng, min, max),
                Force::Turbulent { amount } => accel += unit(rng) * amount * 100.0,
                Force::Twist { cp, amount, axis, local } => {
                    let ax = if local { cps[cp].local(axis) } else { axis }.normalize_or(Vec3::Z);
                    let r = p.pos - cps[cp].pos;
                    accel += ax.cross(r).normalize_or_zero() * amount;
                }
            }
        }
        for (o, cap) in sys.oper.iter().zip(&sys.oper_cap) {
            if !cap_active(*cap, emitting) {
                continue;
            }
            match *o {
                Oper::Move { gravity, drag } => {
                    p.vel += (gravity + accel) * dt;
                    if drag > 0.0 {
                        p.vel *= (1.0 - drag.min(1.0)).powf(dt * 30.0);
                    }
                    p.pos += p.vel * dt;
                }
                Oper::Decay => {
                    if p.life > 0.0 && p.age >= p.life {
                        p.dead = true;
                    }
                }
                Oper::RadiusScale { st, et, ss, es, bias: b } => {
                    let k = bias(frac(t, st, et), b);
                    p.radius *= ss + (es - ss) * k;
                }
                Oper::FadeAndDecay { sa, ea, sfi, efi, sfo, efo } => {
                    let a = if t < efi {
                        sa + (1.0 - sa) * frac(t, sfi, efi)
                    } else if t >= sfo {
                        1.0 + (ea - 1.0) * frac(t, sfo, efo)
                    } else {
                        1.0
                    };
                    p.alpha *= a;
                    if t >= efo && p.life > 0.0 {
                        p.dead = true;
                    }
                }
                Oper::FadeOutRandom { min, max, prop } => {
                    let ft = min + (max - min) * p.r;
                    let ft = if prop { ft * p.life } else { ft };
                    if p.life > 0.0 && ft > 0.0 {
                        p.alpha *= ((p.life - p.age) / ft).clamp(0.0, 1.0);
                    }
                }
                Oper::FadeInRandom { min, max, prop } => {
                    let ft = min + (max - min) * p.r;
                    let ft = if prop { ft * p.life } else { ft };
                    if ft > 0.0 {
                        p.alpha *= (p.age / ft).clamp(0.0, 1.0);
                    }
                }
                Oper::FadeOutSimple { t: ft } => {
                    if ft > 0.0 {
                        p.alpha *= ((1.0 - t) / ft).clamp(0.0, 1.0);
                    }
                }
                Oper::FadeInSimple { t: ft } => {
                    if ft > 0.0 {
                        p.alpha *= (t / ft).clamp(0.0, 1.0);
                    }
                }
                Oper::ColorFade { to, st, et } => {
                    let k = frac(t, st, et);
                    p.color = p.color.lerp(to * sys.hdr, k);
                }
                Oper::SpinRoll { rate, min, stop } => {
                    let r = if stop > 0.0 { min + (rate - min) * (1.0 - p.age / stop).max(0.0) } else { rate };
                    p.rot += r * dt;
                }
                Oper::OscillateColor { rate, freq, mult, phase } => {
                    let h = |k: u32| ((p.id.wrapping_mul(2654435761).wrapping_add(k.wrapping_mul(40503))) >> 8) as f32 / (1u32 << 24) as f32;
                    let ph = phase * std::f32::consts::PI;
                    let mut c = p.color;
                    for i in 0..3 {
                        let r = rate.0[i] + (rate.1[i] - rate.0[i]) * h(11 + i as u32);
                        let w = (mult * (freq.0[i] + (freq.1[i] - freq.0[i]) * h(21 + i as u32))).max(1e-3) * std::f32::consts::PI;
                        c[i] = (c[i] + r * ((w * p.age + ph).sin() - ph.sin()) / w).max(0.0);
                    }
                    p.color = c;
                }
                Oper::Remap { inp, out, imin, imax, omin, omax, scale, abs } => {
                    let mut x = get_field(p, inp);
                    if abs {
                        x = x.abs();
                    }
                    put(p, out, remap(x, imin, imax, omin, omax), scale, false);
                }
                Oper::Oscillate { field: f, rate, freq, st, et, prop, mult, phase } => {
                    // Per-particle randoms from the particle's id.
                    let h = |k: u32| ((p.id.wrapping_mul(2654435761).wrapping_add(k.wrapping_mul(40503))) >> 8) as f32 / (1u32 << 24) as f32;
                    let r = rate.0 + (rate.1 - rate.0) * h(1);
                    let fq = (freq.0 + (freq.1 - freq.0) * h(2)).max(1e-3);
                    let (s0, e0) = (st.0 + (st.1 - st.0) * h(3), et.0 + (et.1 - et.0) * h(4));
                    let now = if prop { t } else { p.age };
                    if now >= s0 && now <= e0 {
                        let w = mult * fq * std::f32::consts::PI;
                        let v = get_field(p, f) + r * ((w * p.age + phase * std::f32::consts::PI).sin() - (phase * std::f32::consts::PI).sin()) / w;
                        let v = if matches!(f, Field::Alpha) { v.clamp(0.0, 1.0) } else if matches!(f, Field::Radius) { v.max(0.0) } else { v };
                        if matches!(f, Field::Alpha2) {
                            p.alpha *= (1.0 + r * ((w * p.age + phase * std::f32::consts::PI).sin() - (phase * std::f32::consts::PI).sin()) / w).clamp(0.0, 1.0);
                        } else {
                            set_field(p, f, v, false);
                        }
                    }
                }
                Oper::DistToCp { cp, out, dmin, dmax, omin, omax, active, scale_init, scale_cur } => {
                    let d = p.pos.distance(cps[cp].pos);
                    let (lo, hi) = (dmin.min(dmax), dmin.max(dmax));
                    if !(active && (d < lo || d > hi)) {
                        put(p, out, remap(d, dmin, dmax, omin, omax), scale_init, scale_cur);
                    }
                }
                Oper::DistBetweenCps { a, b, out, dmin, dmax, omin, omax, scale_init, scale_cur } => {
                    let d = cps[a].pos.distance(cps[b].pos);
                    put(p, out, remap(d, dmin, dmax, omin, omax), scale_init, scale_cur);
                }
                Oper::Ramp { field: f, rate, st, et } => {
                    let k = (p.age.min(et) - st).max(0.0);
                    if k > 0.0 {
                        let v = get_field(p, f) + rate * k;
                        if matches!(f, Field::Alpha2) {
                            p.alpha *= (1.0 + rate * k).clamp(0.0, 1.0);
                        } else {
                            set_field(p, f, if matches!(f, Field::Alpha) { v.clamp(0.0, 1.0) } else { v }, false);
                        }
                    }
                }
                Oper::OrientToCp { cp, offset, strength } => {
                    let c = &cps[cp];
                    let d = p.pos - c.pos;
                    let r = d.dot(c.left).atan2(d.dot(c.fwd)) + std::f32::consts::PI + offset;
                    p.rot += (r - p.rot) * strength;
                }
                Oper::Graph { field: f, ref pts, omin, omax, mul, add, lifespans, time, looped } => {
                    let mut x = if lifespans { t } else { p.age / time };
                    if looped {
                        x = x.fract();
                    }
                    let v = omin + (omax - omin) * graph(pts, x);
                    if add {
                        set_field(p, f, get_field(p, f) + v, false);
                    } else {
                        set_field(p, f, v, mul);
                    }
                }
                Oper::LerpInitial { field: f, to, st, et } => {
                    let k = frac(t, st, et);
                    let b = get_base(p, f);
                    set_field(p, f, b + (to - b) * k, false);
                }
                Oper::LockToCp { cp } => {
                    if !newborn {
                        p.pos += cp_delta[cp];
                    }
                }
                Oper::MaxVelocity { max } => {
                    if max > 0.0 {
                        p.vel = p.vel.clamp_length_max(max);
                    }
                }
                Oper::SetCps { .. } | Oper::CpToPlayer { .. } | Oper::Restart { .. } => {}
            }
        }
        if !sys.decays && p.life > 0.0 && p.age >= p.life {
            p.dead = true;
        }
        for c in &sys.cons {
            match *c {
                Constraint::Collide { bounce, slide, kill_speed } => {
                    let Some(w) = world else { continue };
                    let step = p.vel * dt;
                    let len = step.length();
                    if len < 0.01 {
                        continue;
                    }
                    let from = p.pos - step;
                    if let Some(h) = w.0.raycast(SVec3::from(from.to_array()), SVec3::from((step / len).to_array()), len + p.radius * 0.5) {
                        let nrm = Vec3::from(h.normal.to_array());
                        p.pos = Vec3::from(h.point.to_array()) + nrm * 0.5;
                        let vn = nrm * p.vel.dot(nrm);
                        let vt = p.vel - vn;
                        // "amount of slide" keeps that much of the tangential speed, "amount of
                        // bounce" reflects that much of the normal speed.
                        p.vel = vt * slide - vn * bounce;
                        if kill_speed >= 0.0 && p.vel.length() < kill_speed {
                            p.dead = true;
                        }
                    }
                }
                Constraint::Distance { cp, min, max } => {
                    let d = p.pos - cps[cp].pos;
                    let l = d.length();
                    if l > max && max > 0.0 {
                        p.pos = cps[cp].pos + d / l * max;
                    } else if l < min {
                        p.pos = cps[cp].pos + d.normalize_or(Vec3::Z) * min;
                    }
                }
            }
        }
        p.rot += p.spin * dt;
    }
    n.particles.retain(|p| !p.dead);
}

impl Node {
    /// Whether this node will never make or show another particle.
    fn finished(&self, emitting: bool) -> bool {
        if self.delay > 0.0 {
            return !emitting;
        }
        let more = emitting
            && self.sys.emit.iter().enumerate().any(|(k, e)| match *e {
                Emit::Instant { .. } => !self.instant_done[k],
                Emit::Continuous { duration, start, .. } => duration <= 0.0 || self.age < start + duration,
                Emit::Distance { .. } => true,
            });
        !more && self.particles.is_empty() && self.started
    }
}

/// Start loading the particle library on a worker thread once the game data is open.
pub fn start_pfx(mut commands: Commands, gd: Option<Res<crate::gamedata::GameData>>, existing: Option<Res<Pfx>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let (Some(gd), None) = (gd, existing) else { return };
    let (tx, rx) = mpsc::channel();
    let (pre_tx, pre_rx) = mpsc::channel();
    let root = gd.root.clone();
    let vpk_paths = gd.vpk_paths.clone();
    std::thread::spawn(move || {
        let mut lib = Library::load(root);
        let keys = if std::env::var_os("TF_PFX_NO_PRECACHE").is_some() { Vec::new() } else { precache_keys(&mut lib, &vpk_paths) };
        let frontend = lib.frontend.clone();
        let _ = tx.send(lib);
        if !keys.is_empty() {
            let _ = pre_tx.send(precache(&keys, &vpk_paths, frontend));
        }
    });
    let mut lights = Vec::new();
    // Light-source particles are off by default: with the map's irradiance volume present,
    // any visible point light makes Bevy 0.18 draw BT in the wrong pose, unlit (not seen on
    // the arms or with TF_NO_PROBES). TF_PFX_LIGHTS=1 turns the pool on.
    let n = if std::env::var_os("TF_PFX_LIGHTS").is_some_and(|v| v == "1") { LIGHTS } else { 0 };
    for _ in 0..n {
        lights.push(
            commands
                .spawn((PointLight { intensity: 0.0, range: 1.0, shadows_enabled: false, ..default() }, Transform::default(), Visibility::Visible))
                .id(),
        );
    }
    let _ = &mut materials;
    commands.insert_resource(Pfx {
        lib: None,
        rx: Mutex::new(Some(rx)),
        pre_rx: Mutex::new(Some(pre_rx)),
        effects: Vec::new(),
        layers: Vec::new(),
        layer_of: HashMap::new(),
        decoded: HashMap::new(),
        impacts: HashMap::new(),
        lights,
        pending: Vec::new(),
        rng: 0x51A5_7E11_C0DE,
        next_id: 0,
        total: 0,
        model_cache: HashMap::new(),
        decal_mats: HashMap::new(),
        decals: Default::default(),
        decal_mesh: None,
    });
}

/// Load (once) the render layer for a material path; None if it can't be drawn.

/// Systems the code plays by name (abilities, shields, cockpit), precached with the scripts'.
const CODE_SYSTEMS: &[&str] = &[
    "P_heal",
    "P_health_hex",
    "P_pilot_amped_shield",
    "P_drone_shield_wall_XO",
    "P_titan_gun_shield_3P",
    "P_titan_gun_shield_FP",
    "P_wpn_HeatShield",
    "P_wpn_HeatShield_FP",
    "wpn_vortex_shield_charging",
    "wpn_vortex_shield_charging_FP",
    "wpn_vortex_projectile_rifle",
    "xo_cockpit_spark_01",
];

/// The particle materials worth decoding ahead (the engine precaches each weapon's systems):
/// every system the weapon scripts name (`fx_*`, tracers, trails) and every system in the
/// impact tables they use, with their children.
fn precache_keys(lib: &mut Library, vpk_paths: &[std::path::PathBuf]) -> Vec<String> {
    let t = std::time::Instant::now();
    let vpks: Vec<Vpk> = vpk_paths.iter().filter_map(|p| Vpk::open(p).ok()).collect();
    let read = |p: &str| vpks.iter().find(|v| v.contains(p)).and_then(|v| v.read(p).ok()).map(|b| String::from_utf8_lossy(&b).into_owned());
    let mut names: Vec<String> = Vec::new();
    let mut tables = std::collections::BTreeSet::from(["default".to_string()]);
    let scripts: Vec<String> = vpks.iter().flat_map(|v| v.entries.keys()).filter(|k| k.starts_with("scripts/weapons/") && k.ends_with(".txt")).cloned().collect();
    for f in &scripts {
        let Some(text) = read(f) else { continue };
        for line in text.lines() {
            let mut q = line.split('"').skip(1).step_by(2);
            let (Some(k), Some(v)) = (q.next(), q.next()) else { continue };
            let k = k.to_ascii_lowercase();
            if k == "impact_effect_table" {
                tables.insert(v.to_ascii_lowercase());
            } else if k.starts_with("fx_") || k.starts_with("tracer_effect") || k.starts_with("projectile_trail_effect") {
                names.push(v.to_string());
            }
        }
    }
    for t in &tables {
        let Some(text) = read(&format!("scripts/impacts/{t}.txt")) else { continue };
        for line in text.lines() {
            let mut q = line.split('"').skip(1).step_by(2);
            if let (Some(_), Some(v)) = (q.next(), q.next()) {
                names.push(v.to_string());
            }
        }
    }
    names.extend(CODE_SYSTEMS.iter().map(|s| s.to_string()));
    let mut keys = std::collections::BTreeSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<usize> = names.iter().filter_map(|n| lib.find(n)).collect();
    while let Some(i) = stack.pop() {
        if !seen.insert(i) {
            continue;
        }
        let c = lib.compiled(i);
        if !c.material.is_empty() && !c.render.is_empty() {
            keys.insert(c.material.replace('\\', "/").to_ascii_lowercase());
        }
        stack.extend(c.children.iter().map(|(ch, _, _)| *ch));
    }
    log::info!("particles: precaching {} materials of {} systems ({} weapon scripts, {} impact tables) found in {:?}", keys.len(), seen.len(), scripts.len(), tables.len(), t.elapsed());
    keys.into_iter().collect()
}

/// Decode `keys` on every core with the loader thread's own VPK handles.
fn precache(keys: &[String], vpk_paths: &[std::path::PathBuf], frontend: Option<Arc<Vpk>>) -> Vec<(String, Option<(vtf::Vtf, HashMap<String, String>)>)> {
    let t = std::time::Instant::now();
    let vpks: Vec<Vpk> = vpk_paths.iter().filter_map(|p| Vpk::open(p).ok()).collect();
    let read = |p: &str| vpks.iter().find(|v| v.contains(p)).and_then(|v| v.read(p).ok()).or_else(|| frontend.as_ref().and_then(|v| v.read(p).ok()));
    let out = crate::world::parallel_map(keys, |k| decode_material(k, &read));
    log::info!("particles: precached {} materials in {:?}", keys.len(), t.elapsed());
    keys.iter().cloned().zip(out.into_iter().map(Option::flatten)).collect()
}

/// Read a particle material's .vmt and decode its base texture (with `$texcolorfromalpha`'s
/// ramp applied); None if it can't be drawn (refraction, or missing). Pure, so new effects'
/// materials decode in parallel.
fn decode_material(key: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<(vtf::Vtf, HashMap<String, String>)> {
    let vmt_path = format!("materials/{}", if key.ends_with(".vmt") { key.to_string() } else { format!("{key}.vmt") });
    let text = String::from_utf8_lossy(&read(&vmt_path)?).into_owned();
    let (shader, kv) = parse_vmt(&text);
    if shader.contains("refract") || kv.contains_key("$refractamount") || kv.contains_key("$forcerefract") {
        return None;
    }
    let tex = kv.get("$basetexture")?.replace('\\', "/").to_ascii_lowercase();
    // Some name the source art (`dirt_burst_full.tga`): the compiled texture is the .vtf.
    let tex = tex.trim_end_matches(".vtf").trim_end_matches(".tga").trim_end_matches(".psd");
    let mut v = vtf::decode(&read(&format!("materials/{tex}.vtf"))?, 512).ok()?;
    let flag = |k: &str| kv.get(k).is_some_and(|v| v.trim() != "0");
    if let Some(r) = kv.get("$ramptexture").filter(|_| flag("$texcolorfromalpha")) {
        let r = r.replace('\\', "/").to_ascii_lowercase();
        if let Some(r) = read(&format!("materials/{}.vtf", r.trim_end_matches(".vtf"))).and_then(|b| vtf::decode(&b, 256).ok()) {
        let row = (r.height / 2) as usize * r.width as usize;
        for px in v.rgba.chunks_exact_mut(4) {
            let x = (px[3] as usize * (r.width as usize - 1)) / 255;
            px[..3].copy_from_slice(&r.rgba[(row + x) * 4..][..3]);
        }
        }
    }
    Some((v, kv))
}

#[allow(clippy::too_many_arguments)]
fn layer_for(
    pfx: &mut Pfx,
    material: &str,
    overlay: bool,
    gd: &crate::gamedata::GameData,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<SoftMaterial>,
) -> Option<usize> {
    let key = material.replace('\\', "/").to_ascii_lowercase();
    let layer_key = if overlay { format!("{key}#overlay") } else { key.clone() };
    if let Some(l) = pfx.layer_of.get(&layer_key) {
        return *l;
    }
    let frontend = pfx.lib.as_ref().and_then(|l| l.frontend.clone());
    let read = |p: &str| gd.read_file(p).ok().or_else(|| frontend.as_ref().and_then(|v| v.read(p).ok()));
    let result = (|| {
        let (mut v, kv) = match pfx.decoded.remove(&key) {
            Some(d) => d?,
            None => decode_material(&key, &read)?,
        };
        let flag = |k: &str| kv.get(k).is_some_and(|v| v.trim() != "0");
        let additive = flag("$additive") || flag("$addself");
        let overbright = kv.get("$overbrightfactor").and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).max(0.1);
        let orient = kv.get("$orientation").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
        let mut img = Image::new(
            Extent3d { width: v.width, height: v.height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            std::mem::take(&mut v.rgba),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = bevy::image::ImageSampler::linear();
        // TF_NO_DEPTHBLEND=1 turns the soft-particle fade off; TF_DEPTHBLEND_SCALE=k
        // multiplies the fade distance (both for checking it).
        let depth_scale = std::env::var("TF_DEPTHBLEND_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let depth_blend = if flag("$depthblend") && std::env::var_os("TF_NO_DEPTHBLEND").is_none() { kv.get("$depthblendscale").and_then(|s| s.parse::<f32>().ok()).unwrap_or(DEPTH_BLEND_DEFAULT) * UNIT * depth_scale } else { 0.0 };
        let ignorez = flag("$ignorez");
        let material = mats.add(SoftMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(images.add(img)),
                unlit: true,
                alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
                cull_mode: None,
                double_sided: true,
                fog_enabled: !additive,
                ..default()
            },
            extension: SoftParticle { params: Vec4::new(depth_blend, 0.0, 0.0, 0.0), ignorez },
        });
        let mesh = meshes.add(empty_mesh());
        let entity = commands
            .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster, NotShadowReceiver))
            .id();
        if overlay {
            commands.entity(entity).insert(bevy::camera::visibility::RenderLayers::layer(crate::vmcam::VM_LAYER));
        }
        // `$overbrightfactor` runs up to 30 on the glows (`_15ob`, `_30ob`); applied in full
        // under this tonemapper every glow is a flat white disc, so it is compressed to its
        // square root (15 -> 3.9) and the camera's bloom carries the rest. An approximation of
        // the game's HDR pipeline, not its numbers.
        let gain = if additive { ADD_GAIN } else { BLEND_GAIN } * overbright.sqrt();
        Some(Layer { key: key.clone(), additive, gain, orient, sheet: v.sheet, mesh, entity })
    })();
    let idx = result.map(|l| {
        pfx.layers.push(l);
        pfx.layers.len() - 1
    });
    if idx.is_none() {
        log::debug!("particle material {key}: not drawable");
    }
    pfx.layer_of.insert(layer_key, idx);
    idx
}

fn empty_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3]);
    m.insert_indices(Indices::U32(vec![0, 1, 2]));
    m
}

#[derive(Default)]
struct Batch {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
    /// (depth, first vertex) of each quad, for back-to-front sorting.
    quads: Vec<(f32, u32)>,
}

impl Batch {
    fn quad(&mut self, v: [Vec3; 4], uv: [[f32; 2]; 4], c: [[f32; 4]; 4], depth: f32) {
        let base = self.pos.len() as u32;
        for k in 0..4 {
            self.pos.push(v[k].to_array());
            self.uv.push(uv[k]);
            self.col.push(c[k]);
        }
        self.quads.push((depth, base));
    }
}

/// Spawn requests, follow trails, simulate every effect and rebuild the per-material meshes.
#[allow(clippy::too_many_arguments)]
pub fn update_pfx(
    mut commands: Commands,
    time: Res<Time>,
    pfx: Option<ResMut<Pfx>>,
    gd: Option<Res<crate::gamedata::GameData>>,
    world: Option<Res<crate::player::Collision>>,
    requests: Query<(Entity, &PfxRequest, Has<PfxOverlay>)>,
    impacts: Query<(Entity, &ImpactRequest)>,
    mut trails: Query<(Entity, &GlobalTransform, &mut PfxTrail)>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<SoftMaterial>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<crate::uber::UberMaterial>>),
    mut model_res: (Option<ResMut<crate::convert::Cache>>, Option<Res<crate::pilotweapon::WorldRoot>>),
    mut vis: Query<&mut Visibility>,
    mut lights: Query<(&mut PointLight, &mut Transform)>,
    old: Option<ResMut<crate::particles::Particles>>,
    lenses: Query<(&Projection, Has<crate::vmcam::ViewmodelCamera>), Or<(With<MainCamera>, With<crate::vmcam::ViewmodelCamera>)>>,
) {
    // Set Control Point To Player: Source's C_OP_SetControlPointToPlayer uses the view origin
    // (the camera), so screen-space systems such as the FX_victim glows sit at the eye.
    let player = camera.single().map(|c| to_game(c.translation())).unwrap_or(Vec3::ZERO);
    let (Some(mut pfx), Some(gd)) = (pfx, gd) else {
        for (e, ..) in &requests {
            commands.entity(e).despawn();
        }
        for (e, _) in &impacts {
            commands.entity(e).despawn();
        }
        return;
    };
    if pfx.lib.is_none() {
        let got = pfx.rx.lock().ok().and_then(|g| g.as_ref().and_then(|rx| rx.try_recv().ok()));
        if let Some(lib) = got {
            pfx.lib = Some(lib);
        }
    }
    let pre = pfx.pre_rx.lock().ok().and_then(|g| g.as_ref().and_then(|rx| rx.try_recv().ok()));
    if let Some(list) = pre {
        for (k, d) in list {
            if !pfx.layer_of.contains_key(&k) {
                pfx.decoded.insert(k, d);
            }
        }
    }
    let ready = pfx.ready();
    let dt = time.delta_secs().min(0.1);
    let (meshes, images, mats) = (&mut *assets.0, &mut *assets.1, &mut *assets.2);

    // Requests.
    let fov = |vm: bool| lenses.iter().find(|(_, v)| *v == vm).and_then(|(p, _)| if let Projection::Perspective(pp) = p { Some(pp.fov) } else { None });
    let fp_lens = camera.single().ok().zip(fov(false)).zip(fov(true)).map(|((c, w), v)| (*c, w, v));
    for (e, r, overlay) in &requests {
        commands.entity(e).despawn();
        if ready {
            // First-person systems are emitted at viewmodel points but drawn through the world
            // camera: move them to where the viewmodel lens shows that point. (Overlay systems
            // are drawn by the viewmodel camera itself.)
            let mut cps = r.cps.clone();
            if let (true, false, Some((cam, w, v))) = (r.vm, overlay, fp_lens) {
                for c in &mut cps {
                    c.pos = to_game(crate::vmcam::fx_to_world(crate::player::to_bevy(c.pos), &cam, w, v));
                }
            }
            let spawned = pfx.spawn(&r.name, &cps, r.stop_after, None);
            if spawned.is_none() {
                log::debug!("particle system {} not spawned", r.name);
            }
            if let (Some(id), true) = (spawned, r.vm) {
                if let Some(fx) = pfx.effects.iter_mut().find(|f| f.id == id) {
                    fx.vm = true;
                    fx.overlay = overlay;
                }
            }
        }
    }
    for (e, r) in &impacts {
        commands.entity(e).despawn();
        if ready {
            pfx.spawn_table(&gd, &r.table, "C", r.cp);
        }
    }
    let pending = std::mem::take(&mut pfx.pending);
    let mut old = old;
    for e in pending {
        if !(ready && pfx.spawn_generic(&gd, e)) {
            if let Some(o) = old.as_mut() {
                crate::particles::spawn_effect(o, e);
            }
        }
    }
    // Trails follow their entities.
    let mut alive_trails = Vec::new();
    for (ent, gt, mut tr) in &mut trails {
        let at = to_game(gt.translation());
        let dir = tr.last.map(|l| at - l).filter(|d| d.length() > 1e-3).unwrap_or(Vec3::X);
        tr.last = Some(at);
        let cp = if tr.oriented {
            let (fwd, up) = (dir_to_game(gt.forward().as_vec3()), dir_to_game(gt.up().as_vec3()));
            Cp { pos: at, fwd, left: up.cross(fwd), up }
        } else {
            Cp::facing(at, dir)
        };
        if tr.effect.is_none() && ready {
            tr.effect = pfx.spawn(&tr.name.clone(), &[cp], None, Some(ent));
        }
        if let Some(id) = tr.effect {
            alive_trails.push(id);
            if let Some(fx) = pfx.effects.iter_mut().find(|f| f.id == id) {
                fx.cps[0] = cp;
            }
        }
    }
    // Stop effects whose entity is gone; start end caps.
    let mut caps = Vec::new();
    for fx in pfx.effects.iter_mut() {
        if fx.attached.is_some() && !alive_trails.contains(&fx.id) && !fx.stopped {
            fx.stopped = true;
            caps.push((fx.cps, std::mem::take(&mut fx.end_caps)));
        }
    }
    for (cps, list) in caps {
        for (sys, delay) in list {
            if let Some(lib) = pfx.lib.as_mut() {
                let mut nodes = Vec::new();
                let mut more = Vec::new();
                collect_nodes(lib, sys, delay, 0, &mut nodes, &mut more);
                pfx.next_id += 1;
                let id = pfx.next_id;
                pfx.effects.push(Effect { id, nodes, cps, cp_prev: cps.map(|c| c.pos), cp_vel: [Vec3::ZERO; NCP], age: 0.0, stop_at: DEFAULT_STOP, stopped: false, capped: false, attached: None, end_caps: Vec::new(), vm: false, overlay: false });
            }
        }
    }

    // Layers for nodes that don't have one yet.
    let mut needed = Vec::new();
    for (fi, fx) in pfx.effects.iter().enumerate() {
        for (ni, n) in fx.nodes.iter().enumerate() {
            if n.layer.is_none() && !n.sys.material.is_empty() && !n.sys.render.is_empty() {
                needed.push((fi, ni, n.sys.material.clone(), fx.overlay));
            }
        }
    }
    let t0 = std::time::Instant::now();
    let loads = needed.len();
    // Decode the new materials' textures on every core first: one at a time, the first frag
    // explosion stalled the frame for 350 ms.
    {
        let mut keys: Vec<String> = needed.iter().map(|(_, _, m, _)| m.replace('\\', "/").to_ascii_lowercase()).filter(|k| !pfx.decoded.contains_key(k) && !pfx.layer_of.contains_key(k) && !pfx.layer_of.contains_key(&format!("{k}#overlay"))).collect();
        keys.sort();
        keys.dedup();
        if keys.len() > 1 {
            let frontend = pfx.lib.as_ref().and_then(|l| l.frontend.clone());
            let gdr: &crate::gamedata::GameData = &gd;
            let read = |p: &str| gdr.read_file(p).ok().or_else(|| frontend.as_ref().and_then(|v| v.read(p).ok()));
            let out = crate::world::parallel_map(&keys, |k| {
                let t = std::time::Instant::now();
                let d = decode_material(k, &read);
                log::trace!("pfx decode {k}: {:?}", t.elapsed());
                d
            });
            log::debug!("pfx: decoded {} materials in {:?}", out.len(), t0.elapsed());
            for (k, d) in keys.into_iter().zip(out) {
                pfx.decoded.insert(k, d.flatten());
            }
        }
    }
    for (fi, ni, m, overlay) in needed {
        let l = layer_for(&mut pfx, &m, overlay, &gd, &mut commands, images, meshes, mats);
        pfx.effects[fi].nodes[ni].layer = l.or(Some(usize::MAX));
    }
    if loads > 0 {
        log::debug!("pfx: {loads} layers resolved in {:?}", t0.elapsed());
    }

    // Simulate.
    let world = world.as_deref();
    let mut budget = MAX_PARTICLES.saturating_sub(pfx.total);
    let mut rng = pfx.rng;
    let layers = std::mem::take(&mut pfx.layers);
    for fx in pfx.effects.iter_mut() {
        fx.age += dt;
        if fx.age >= fx.stop_at {
            fx.stopped = true;
        }
        // Set Control Point To Player moves its point before the deltas are taken, so locked
        // particles follow the camera from the first frame.
        for n in &fx.nodes {
            for o in &n.sys.oper {
                if let Oper::CpToPlayer { cp } = o {
                    fx.cps[*cp].pos = player;
                }
            }
        }
        let mut delta = [Vec3::ZERO; NCP];
        for k in 0..NCP {
            delta[k] = fx.cps[k].pos - fx.cp_prev[k];
            fx.cp_vel[k] = if dt > 0.0 { delta[k] / dt } else { Vec3::ZERO };
            fx.cp_prev[k] = fx.cps[k].pos;
        }
        let emitting = !fx.stopped;
        let cp_vel = fx.cp_vel;
        let recap = fx.stopped && !fx.capped;
        fx.capped |= fx.stopped;
        for ni in 0..fx.nodes.len() {
            let parent: Vec<(Vec3, Vec3)> = match fx.nodes[ni].parent {
                Some(pi) if fx.nodes[ni].sys.init.iter().any(|i| matches!(i, Init::FromParent { .. })) => fx.nodes[pi].particles.iter().filter(|p| !p.dead).map(|p| (p.pos, p.vel)).collect(),
                _ => Vec::new(),
            };
            let n = &mut fx.nodes[ni];
            let sheet = n.layer.and_then(|l| layers.get(l)).map(|l| l.sheet.clone()).unwrap_or_default();
            let sheet_len = |s: u16| sheet.get(s as usize).map(|q| q.frames.len() as f32).unwrap_or(1.0);
            if recap {
                let sys = n.sys.clone();
                for p in n.particles.iter_mut() {
                    let id = p.id;
                    apply_inits(&sys, p, &fx.cps, &cp_vel, &[], &sheet_len, &mut rng, id, false, true);
                }
            }
            step_node(n, &mut fx.cps, &delta, &cp_vel, emitting, dt, &mut rng, world, &mut budget, &sheet_len, player, &parent);
        }
    }
    pfx.layers = layers;
    pfx.rng = rng;
    // Decals: laid once per node when its first particle is born, under the world root in
    // game units, the oldest removed past MAX_DECALS.
    {
        let (cache, root) = (&mut model_res.0, model_res.1.as_deref());
        let Pfx { effects, decal_mats, decals, decal_mesh, rng: prng, .. } = &mut *pfx;
        for fx in effects.iter_mut() {
            let cp = fx.cps[0];
            for n in fx.nodes.iter_mut().filter(|n| !n.decal_done && !n.particles.is_empty()) {
                let Some(Render::Decal { materials }) = n.sys.render.iter().find(|r| matches!(r, Render::Decal { .. })) else { continue };
                n.decal_done = true;
                if std::env::var_os("TF_NO_DECALS").is_some() {
                    continue;
                }
                let (Some(cache), Some(root)) = (cache.as_deref_mut(), root) else { continue };
                *prng ^= *prng << 13;
                *prng ^= *prng >> 7;
                *prng ^= *prng << 17;
                let name = &materials[(*prng as usize) % materials.len()];
                let mat = decal_mats
                    .entry(name.clone())
                    .or_insert_with(|| {
                        let (pi, m) = gd.material(name)?;
                        let &g = m.textures.first().filter(|&&g| g != 0)?;
                        let img = crate::convert::image(&gd, cache, images, pi, g)?;
                        Some(assets.3.add(StandardMaterial {
                            base_color_texture: Some(img),
                            alpha_mode: AlphaMode::Blend,
                            perceptual_roughness: 1.0,
                            depth_bias: 50.0,
                            ..default()
                        }))
                    })
                    .clone();
                let Some(mat) = mat else { continue };
                let mesh = decal_mesh.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
                let spin = (*prng >> 20) as f32 / (1u64 << 44) as f32 * std::f32::consts::TAU;
                let normal = cp.up.normalize_or(Vec3::Z);
                let tf = Transform {
                    translation: cp.pos + normal * 0.25,
                    rotation: Quat::from_rotation_arc(Vec3::Z, normal) * Quat::from_rotation_z(spin),
                    scale: Vec3::splat(DECAL_SIZE),
                };
                let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), tf, NotShadowCaster)).id();
                commands.entity(root.0).add_child(e);
                decals.push_back(e);
                while decals.len() > MAX_DECALS {
                    if let Some(old) = decals.pop_front() {
                        commands.entity(old).despawn();
                    }
                }
            }
        }
    }
    // Model renderers: an entity per particle (per renderer), pooled per node, under the world
    // root in game units.
    {
        let (mut cache, root) = model_res;
        let Pfx { effects, model_cache, .. } = &mut *pfx;
        let eye_game = camera.single().map(|c| to_game(c.translation())).unwrap_or(Vec3::ZERO);
        for fx in effects.iter_mut() {
            let cps = fx.cps;
            for n in fx.nodes.iter_mut() {
                let renders: Vec<Render> = n.sys.render.iter().filter(|r| matches!(r, Render::Models { .. })).cloned().collect();
                if renders.is_empty() {
                    continue;
                }
                let want = n.particles.len() * renders.len();
                while n.models.len() < want {
                    let Render::Models { model, .. } = &renders[n.models.len() % renders.len()] else { unreachable!() };
                    let parts = model_cache
                        .entry(model.clone())
                        .or_insert_with(|| match cache.as_deref_mut() {
                            Some(cache) => match crate::world::build_static_model(&gd, model, cache, meshes, images, &mut assets.3) {
                                // The tint without the system's HDR gain: additive fans
                                // (heat shield, hdr 5) blow out to white with it.
                                Ok((parts, _)) => Some(Arc::new(fx_model_materials(&gd, cache, images, model, parts, &mut assets.3, &mut assets.4, if std::env::var_os("TF_PFX_GAMMA_COLORS").is_some() { n.sys.color / n.sys.hdr.max(1.0) } else { to_srgb(n.sys.color / n.sys.hdr.max(1.0)) }, n.sys.hdr.max(1.0)))),
                                Err(e) => {
                                    log::warn!("particle model {model}: {e:#}");
                                    None
                                }
                            },
                            None => None,
                        })
                        .clone();
                    let e = commands.spawn((Transform::default(), Visibility::Hidden)).id();
                    if let Some(parts) = parts {
                        for (m, mat) in parts.iter() {
                            if let Some(aabb) = meshes.get(m).and_then(|m| bevy::camera::primitives::MeshAabb::compute_aabb(m)) {
                                log::debug!("model instance mesh {model}: aabb {:?}..{:?}", aabb.min(), aabb.max());
                            }
                            let c = commands.spawn((Mesh3d(m.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver)).id();
                            match mat {
                                FxMat::Std(h) => commands.entity(c).insert(MeshMaterial3d(h.clone())),
                                FxMat::Uber(h) => commands.entity(c).insert(MeshMaterial3d(h.clone())),
                            };
                            commands.entity(e).add_child(c);
                        }
                    }
                    if let Some(r) = root.as_deref() {
                        commands.entity(r.0).add_child(e);
                    }
                    n.models.push(e);
                }
                let cp = cps[0];
                let basis = Quat::from_mat3(&Mat3::from_cols(cp.fwd, cp.up.cross(cp.fwd), cp.up));
                let mut used = 0;
                for p in &n.particles {
                    for r in &renders {
                        let Render::Models { pitch, yaw, roll, orient_normal, .. } = r else { unreachable!() };
                        let e = n.models[used];
                        used += 1;
                        // First-person models (shell casings) launch from beside the eye; hide
                        // them until they are clear of it, or the first frames fill the view.
                        let at_eye = fx.vm && p.pos.distance(eye_game) < FP_MODEL_CLEARANCE;
                        if at_eye || p.alpha <= 0.01 || p.radius <= 0.0 || std::env::var_os("TF_NO_FX_MODELS").is_some() {
                            commands.entity(e).insert(Visibility::Hidden);
                            continue;
                        }
                        // Source's AngleMatrix order: yaw about Z, pitch about Y, roll about X.
                        // Model Z on the normal with a fixed roll: model X toward world down (shortest-arc
                        // rotations flip with the facing, which stood the A-Wall's models on
                        // their heads facing one way and buried them the other).
                        let base = if *orient_normal { normal_basis(cp.fwd) } else { basis };
                        let rot = base * Quat::from_rotation_z(*yaw) * Quat::from_rotation_y(*pitch) * Quat::from_rotation_x(*roll);
                        if fx.age.rem_euclid(0.5) < 0.02 {
                            let cam_game = camera.single().map(|c| to_game(c.translation())).unwrap_or(Vec3::ZERO);
                            log::trace!("model instance {} age {:.1} at {:?} radius {} alpha {} cp0 {:?} fwd {:?} camera {:?} dist {:.0} model -x/+z goes to {:?} / {:?}", n.sys.material, fx.age, p.pos, p.radius, p.alpha, cp.pos, cp.fwd, cam_game, p.pos.distance(cam_game), rot * Vec3::NEG_X, rot * Vec3::Z);
                        }
                        let k = std::env::var("TF_FX_MODEL_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
                        commands.entity(e).insert((Transform { translation: p.pos, rotation: rot, scale: Vec3::splat(p.radius * k) }, Visibility::Visible));
                    }
                }
                for e in &n.models[used..] {
                    commands.entity(*e).insert(Visibility::Hidden);
                }
            }
        }
    }
    let keep = |fx: &Effect| {
        let emitting = !fx.stopped;
        fx.age < HARD_LIMIT.max(if fx.attached.is_some() { f32::INFINITY } else { 0.0 }) && !fx.nodes.iter().all(|n| n.finished(emitting))
    };
    for fx in pfx.effects.iter().filter(|fx| !keep(fx)) {
        for n in &fx.nodes {
            for e in &n.models {
                commands.entity(*e).despawn();
            }
        }
    }
    pfx.effects.retain(keep);
    pfx.total = pfx.effects.iter().flat_map(|f| f.nodes.iter()).map(|n| n.particles.len()).sum();

    // Draw.
    let Ok(cam) = camera.single() else { return };
    let eye = to_game(cam.translation());
    let (right, up, fwd) = (dir_to_game(cam.right().as_vec3()), dir_to_game(cam.up().as_vec3()), dir_to_game(cam.forward().as_vec3()));
    let mut batches: Vec<Batch> = (0..pfx.layers.len()).map(|_| Batch::default()).collect();
    // Sprites fade out as they reach the eye (the game's sprite cards fade against the near
    // plane too; without it rounds and smoke arriving at the camera white the screen out).
    let near_fade = |pos: Vec3| ((pos - eye).dot(fwd) - NEAR_FADE_START) / (NEAR_FADE_END - NEAR_FADE_START);
    let near_fade = move |pos: Vec3| near_fade(pos).clamp(0.0, 1.0);
    // First-person effects sit close to the eye on purpose (muzzle flashes), so they fade by
    // size instead: out as a sprite grows to cover the view (radius near its distance), as
    // first-person smoke drifting over the camera would otherwise fill the screen.
    let vm_fade = move |pos: Vec3, radius: f32| (((pos - eye).dot(fwd) / radius.max(0.01) - 1.0) * 0.5).clamp(0.0, 1.0);
    let mut light_cands: Vec<(f32, Vec3, Vec3, f32)> = Vec::new();
    for fx in &pfx.effects {
        for n in &fx.nodes {
            for r in &n.sys.render {
                if let Render::Light { scale, radius, by_alpha } = *r {
                    for p in &n.particles {
                        let a = if by_alpha { p.alpha } else { 1.0 };
                        let c = p.color * scale * a;
                        let range = (p.radius * radius).max(1.0);
                        light_cands.push((c.max_element() * range, p.pos, c, range));
                    }
                }
            }
            let Some(li) = n.layer.filter(|&l| l != usize::MAX) else { continue };
            let layer = &pfx.layers[li];
            let batch = &mut batches[li];
            for r in &n.sys.render {
                match *r {
                    Render::Sprites { rate, fit, orient } => {
                        let orient = if orient >= 0 { orient } else { layer.orient };
                        for p in &n.particles {
                            let alpha = p.alpha * if fx.vm { vm_fade(p.pos, p.radius) } else { near_fade(p.pos) };
                            if alpha <= 0.002 || p.radius <= 0.0 {
                                continue;
                            }
                            let rect = sheet_rect(&layer.sheet, p, rate, fit);
                            let c = p.color * layer.gain;
                            let c = [c.x, c.y, c.z, alpha.min(1.0)];
                            let (sr, sc) = p.rot.sin_cos();
                            let (ax, ay) = match orient {
                                1 => {
                                    let side = Vec3::Z.cross(eye - p.pos).normalize_or(right);
                                    (side, Vec3::Z)
                                }
                                2 | 3 => {
                                    let nrm = if orient == 2 { Vec3::Z } else { fx.cps[0].up };
                                    let a = nrm.any_orthonormal_vector();
                                    (a, nrm.cross(a))
                                }
                                _ => (right, up),
                            };
                            let rx = (ax * sc + ay * sr) * p.radius;
                            let ry = (ay * sc - ax * sr) * p.radius;
                            let (u0, u1) = if p.flip { (rect[2], rect[0]) } else { (rect[0], rect[2]) };
                            batch.quad(
                                [p.pos - rx + ry, p.pos + rx + ry, p.pos + rx - ry, p.pos - rx - ry],
                                [[u0, rect[1]], [u1, rect[1]], [u1, rect[3]], [u0, rect[3]]],
                                [c; 4],
                                (p.pos - eye).dot(fwd),
                            );
                        }
                    }
                    Render::Trail { rate, min, max, fade_in, tail } => {
                        for p in &n.particles {
                            let alpha = p.alpha * if fx.vm { vm_fade(p.pos, p.radius) } else { near_fade(p.pos) };
                            if alpha <= 0.002 || p.radius <= 0.0 {
                                continue;
                            }
                            let speed = p.vel.length();
                            let dir = if speed > 1e-3 { p.vel / speed } else { fx.cps[0].fwd };
                            let mut len = (speed * p.trail).clamp(min, max.max(min));
                            if fade_in > 0.0 && p.age < fade_in {
                                len *= p.age / fade_in;
                            }
                            let len = len.max(p.radius * 0.5);
                            let rect = sheet_rect(&layer.sheet, p, rate, false);
                            let to_eye = (eye - p.pos).normalize_or(Vec3::Z);
                            let cr = dir.cross(to_eye);
                            let side = if cr.length() > 0.05 { cr.normalize() } else { right } * p.radius;
                            let c = p.color * layer.gain;
                            let head = [c.x, c.y, c.z, alpha.min(1.0)];
                            let tl = [c.x * tail[0], c.y * tail[1], c.z * tail[2], (alpha * tail[3]).min(1.0)];
                            let back = p.pos - dir * len;
                            batch.quad(
                                [p.pos - side, p.pos + side, back + side, back - side],
                                [[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]],
                                [head, head, tl, tl],
                                (p.pos - eye).dot(fwd),
                            );
                        }
                    }
                    Render::Rope => {
                        let mut ps: Vec<&P> = n.particles.iter().filter(|p| p.alpha > 0.002).collect();
                        ps.sort_by_key(|p| p.id);
                        let total = ps.len().max(2) as f32 - 1.0;
                        for (k, w) in ps.windows(2).enumerate() {
                            let (a, b) = (w[0], w[1]);
                            let d = (b.pos - a.pos).normalize_or(Vec3::X);
                            let side = |p: &P| {
                                let cr = d.cross((eye - p.pos).normalize_or(Vec3::Z));
                                (if cr.length() > 0.05 { cr.normalize() } else { right }) * p.radius
                            };
                            let (sa, sb) = (side(a), side(b));
                            let ca = a.color * layer.gain;
                            let cb = b.color * layer.gain;
                            let (v0, v1) = (k as f32 / total, (k + 1) as f32 / total);
                            batch.quad(
                                [a.pos - sa, a.pos + sa, b.pos + sb, b.pos - sb],
                                [[0.0, v0], [1.0, v0], [1.0, v1], [0.0, v1]],
                                [[ca.x, ca.y, ca.z, a.alpha.min(1.0)], [ca.x, ca.y, ca.z, a.alpha.min(1.0)], [cb.x, cb.y, cb.z, b.alpha.min(1.0)], [cb.x, cb.y, cb.z, b.alpha.min(1.0)]],
                                (a.pos - eye).dot(fwd),
                            );
                        }
                    }
                    Render::Light { .. } | Render::Models { .. } | Render::Decal { .. } => {}
                }
            }
        }
    }
    for (li, mut b) in batches.into_iter().enumerate() {
        let layer = &pfx.layers[li];
        if let Ok(mut v) = vis.get_mut(layer.entity) {
            let want = if b.quads.is_empty() { Visibility::Hidden } else { Visibility::Visible };
            if *v != want {
                *v = want;
            }
        }
        let Some(mesh) = meshes.get_mut(&layer.mesh) else { continue };
        if b.quads.is_empty() {
            if mesh.count_vertices() != 3 {
                *mesh = empty_mesh();
            }
            continue;
        }
        if !layer.additive {
            b.quads.sort_by(|a, c| c.0.total_cmp(&a.0));
        }
        for &(_, base) in &b.quads {
            b.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let pos: Vec<[f32; 3]> = b.pos.iter().map(|p| to_bevy(Vec3::from(*p)).to_array()).collect();
        let normals = vec![(-cam.forward().as_vec3()).to_array(); pos.len()];
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.col);
        mesh.insert_indices(Indices::U32(b.idx));
    }
    // Lights: the strongest few light-source particles.
    light_cands.sort_by(|a, b| b.0.total_cmp(&a.0));
    let ents = pfx.lights.clone();
    for (k, e) in ents.into_iter().enumerate() {
        let Ok((mut l, mut tf)) = lights.get_mut(e) else { continue };
        match light_cands.get(k) {
            // Off is intensity 0 (the entity stays visible).
            Some(&(_, pos, c, range)) if c.max_element() > 0.01 => {
                l.color = Color::srgb(c.x.min(1.0), c.y.min(1.0), c.z.min(1.0));
                l.intensity = LIGHT_LUMENS * c.max_element().min(4.0);
                l.range = range * UNIT;
                tf.translation = to_bevy(pos);
            }
            _ => {
                if l.intensity != 0.0 {
                    l.intensity = 0.0;
                }
            }
        }
    }
    let _ = dir_to_bevy;
}

fn sheet_rect(sheet: &[Sequence], p: &P, rate: f32, fit: bool) -> [f32; 4] {
    let Some(s) = sheet.get(p.seq as usize).or_else(|| sheet.first()) else { return [0.0, 0.0, 1.0, 1.0] };
    if s.frames.len() <= 1 {
        return s.rect_at(0.0);
    }
    let t = if fit && p.life > 0.0 {
        p.age / p.life
    } else {
        // "animation rate": passes through the sequence per second.
        let x = p.age * rate;
        if s.clamp {
            x.min(0.999)
        } else {
            x.fract()
        }
    };
    s.rect_at(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_table_blocks() {
        let t = "ImpactTable\n{\n Info\n {\n \"x\" \"y\"\n }\n FX\n {\n \"C\" \"impact_concrete\" // c\n \"D\" \"none\"\n \"E\" \"impact_titan\"\n }\n FX\n {\n \"C\" \"tracer_sparks\"\n }\n FX_victim\n {\n \"E\" \"impact_victim\"\n }\n}\n";
        let b = parse_impact_table(t);
        assert_eq!(b.fx.len(), 2);
        assert_eq!(b.fx[0]["C"], "impact_concrete");
        assert_eq!(b.fx[0]["E"], "impact_titan");
        assert_eq!(b.fx[1]["C"], "tracer_sparks");
        assert_eq!(b.victim.len(), 1);
        assert_eq!(b.victim[0]["E"], "impact_victim");
    }

    #[test]
    fn graph_lerps() {
        let g = [[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]];
        assert!((graph(&g, 0.25) - 0.5).abs() < 1e-5);
        assert!((graph(&g, 0.75) - 0.5).abs() < 1e-5);
        assert_eq!(graph(&g, 2.0), 0.0);
    }

    #[test]
    fn vmt_keys() {
        let (s, kv) = parse_vmt("\"SpriteCard\"\n{\n\t$basetexture \"particle/x\"\n\t\"$additive\" \"1\" // y\n}\n");
        assert_eq!(s, "spritecard");
        assert_eq!(kv["$basetexture"], "particle/x");
        assert_eq!(kv["$additive"], "1");
    }
}
