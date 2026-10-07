//! Effect surfaces (godrays, atmosphere cards, waterfalls, scrolling water and screens) with the
//! game's own shader features. A material's shader set name spells out what its shader does
//! (`uberUnlitVcoltVcolaAdfAefAddNoCocNoTsaaUv1atUV21Samp2_wld`), and its CPU data holds the
//! constants (see `tf_assets::material::ShaderParams`). The features drawn here:
//! - `Uv1at`: UV1's translate is a scroll speed (units per second);
//! - `Adf`: alpha distance fade, `saturate(distance * scale + bias)` in game units;
//! - `Aef`: alpha edge fade by the view angle, `saturate((|N·V| - outer) / (inner - outer))^exp`;
//! - `Vcolt` / `Vcola`: vertex colour tints the colour / scales the alpha;
//! - `Add`: additive blending, `Trans`: alpha blending, `Unlit`: no lighting;
//! - albedo tint and opacity.

use crate::gamedata::GameData;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct UberParams {
    /// UV1 matrix (m00, m01, m10, m11).
    pub uv: Vec4,
    /// UV1 translate (or scroll speed), 1 if animated, game units per world unit.
    pub uv_t: Vec4,
    /// Distance fade scale and bias, edge fade exponent and inner.
    pub fade: Vec4,
    /// Edge fade outer, flags, opacity, unused.
    pub fade2: Vec4,
    /// Albedo tint.
    pub tint: Vec4,
    /// UV2 matrix and translate (scroll speed if `uv_t.z`), for the distortion layer.
    pub uv2: Vec4,
    pub uv2_t: Vec4,
}

pub const UNLIT: u32 = 1;
pub const ADD: u32 = 2;
pub const VCOLT: u32 = 4;
pub const VCOLA: u32 = 8;
pub const ADF: u32 = 16;
pub const AEF: u32 = 32;
/// The opacity texture multiplies the alpha (sampled through UV1 like the colour).
pub const OPA: u32 = 64;
/// The distortion texture (a two-channel offset map, through UV2) shifts UV1.
pub const DIST: u32 = 128;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct UberExt {
    #[uniform(100)]
    pub params: UberParams,
    #[texture(101)]
    #[sampler(102)]
    pub opacity: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub distort: Option<Handle<Image>>,
}

impl MaterialExtension for UberExt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/uber.wgsl".into()
    }
    fn deferred_fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/uber.wgsl".into()
    }
}

pub type UberMaterial = ExtendedMaterial<StandardMaterial, UberExt>;

pub struct UberPlugin;

impl Plugin for UberPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "uber.wgsl");
        app.add_plugins(MaterialPlugin::<UberMaterial>::default());
    }
}

/// What an effect material needs, from its shader set and constants.
#[derive(Clone, Debug)]
pub struct UberInfo {
    pub flags: u32,
    pub scroll: bool,
    pub params: tf_assets::material::ShaderParams,
    pub blended: bool,
}

/// The effect features of a material, or None for an ordinary surface (drawn with the plain
/// StandardMaterial path).
pub fn info(gd: &GameData, name: &str) -> Option<UberInfo> {
    // Off by default: it cost ~40% of the frame rate on 2026-10-05 (98 -> 60 fps on
    // mp_forwardbase_kodai); TF_UBER=1 turns the effect path on.
    if std::env::var_os("TF_UBER").is_none() {
        return None;
    }
    info_always(gd, name)
}

/// `info` without the world-surface switch (for the few effect models, which are cheap).
pub fn info_always(gd: &GameData, name: &str) -> Option<UberInfo> {
    let (_, m) = gd.material(name)?;
    let params = m.params?;
    let shader = gd.shader_set_name(m.shader_set)?;
    let tokens = tf_assets::material::shader_flags(&shader);
    let has = |t: &str| tokens.iter().any(|x| x == t);
    let mut flags = 0;
    for (t, f) in [("Unlit", UNLIT), ("Add", ADD), ("Vcolt", VCOLT), ("Vcola", VCOLA), ("Adf", ADF), ("Aef", AEF)] {
        if has(t) {
            flags |= f;
        }
    }
    // The colour texture (slot 0) samples through the UV transform its digit names; `Uv<n>at`
    // animates that transform's translate.
    let slot = tf_assets::material::uv_slots(&shader).first().copied().unwrap_or(0) as usize;
    let uv = if (1..=3).contains(&slot) { params.uv[slot - 1] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
    let scroll = slot > 0 && has(&format!("Uv{slot}at")) && (uv[4] != 0.0 || uv[5] != 0.0);
    let mut params = params;
    params.uv[0] = uv;
    // Only surfaces that need one of these features take the effect path.
    if flags & (ADD | ADF | AEF | UNLIT) == 0 && !scroll {
        return None;
    }
    log::debug!("effect material {name}: {shader} flags {flags:#x} scroll {scroll} {params:?}");
    Some(UberInfo { flags, scroll, params, blended: has("Trans") || has("Add") || m.blended() })
}

/// Give every mesh whose material needs the effect path (registered in the material cache by
/// `convert::material`) its effect material instead. The first time the cache exists all meshes
/// are checked; after that, only newly spawned ones.
#[allow(clippy::type_complexity)]
pub fn swap_materials(
    mut commands: Commands,
    cache: Option<Res<crate::convert::Cache>>,
    std_mats: Res<Assets<StandardMaterial>>,
    mut uber: ResMut<Assets<UberMaterial>>,
    mut made: Local<std::collections::HashMap<AssetId<StandardMaterial>, Handle<UberMaterial>>>,
    all: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
    added: Query<(Entity, &MeshMaterial3d<StandardMaterial>), Added<MeshMaterial3d<StandardMaterial>>>,
) {
    let Some(cache) = cache else { return };
    if cache.uber.is_empty() {
        return;
    }
    let mut swap = |e: Entity, m: &MeshMaterial3d<StandardMaterial>| {
        let id = m.0.id();
        let Some(info) = cache.uber.get(&id) else { return };
        let h = match made.get(&id) {
            Some(h) => h.clone(),
            None => {
                let Some(base) = std_mats.get(id) else { return };
                let h = material(base, info, &mut uber);
                made.insert(id, h.clone());
                h
            }
        };
        commands.entity(e).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h));
    };
    if cache.is_added() {
        for (e, m) in &all {
            swap(e, m);
        }
        log::info!("effect materials: {} of {} kinds in use", made.len(), cache.uber.len());
    } else {
        for (e, m) in &added {
            swap(e, m);
        }
    }
}

/// Build the effect material on top of the ordinary one (its textures and blend mode).
pub fn material(base: &StandardMaterial, info: &UberInfo, uber: &mut Assets<UberMaterial>) -> Handle<UberMaterial> {
    let p = &info.params;
    let mut base = base.clone();
    if info.flags & ADD != 0 {
        base.alpha_mode = AlphaMode::Add;
    } else if info.blended {
        base.alpha_mode = AlphaMode::Blend;
    }
    if info.flags & UNLIT != 0 {
        base.unlit = true;
    }
    if info.flags & (ADD | UNLIT) != 0 {
        // Light shafts and haze are seen from both sides.
        base.double_sided = true;
        base.cull_mode = None;
        base.emissive = LinearRgba::BLACK;
    }
    let uv = p.uv[0];
    let params = UberParams {
        uv: Vec4::new(uv[0], uv[1], uv[2], uv[3]),
        uv_t: Vec4::new(uv[4], uv[5], if info.scroll { 1.0 } else { 0.0 }, 1.0 / crate::player::UNIT),
        fade: Vec4::new(p.distance_fade[0], p.distance_fade[1], p.edge_fade[0], p.edge_fade[1]),
        fade2: Vec4::new(p.edge_fade[2], info.flags as f32, p.opacity.clamp(0.0, 1.0), 0.0),
        tint: Vec4::new(p.albedo_tint[0], p.albedo_tint[1], p.albedo_tint[2], 1.0),
        uv2: Vec4::new(1.0, 0.0, 0.0, 1.0),
        uv2_t: Vec4::ZERO,
    };
    uber.add(UberMaterial { base, extension: UberExt { params, opacity: None, distort: None } })
}
