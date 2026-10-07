//! The Pilot's Cloak and Phase Shift in first person. Cloak: while Cloak (`mp_ability_cloak`) runs, the arms and gun
//! draw as a refracting outline (cloak.wgsl) instead of their own materials. The game's cloak
//! look is engine-side (its scripts only call `EnableCloak`), so the strength of the warp, the
//! tint and the rim are by eye.

use crate::pilotctl::Control;
use crate::pilotability::PilotStatus;
use bevy::camera::visibility::RenderLayers;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};

#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct CloakParams {
    /// x: refraction offset (fraction of the screen), y: rim strength, z: rim exponent, w: tint.
    pub params: Vec4,
    pub rim_color: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct CloakExt {
    #[uniform(100)]
    pub params: CloakParams,
}

impl MaterialExtension for CloakExt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://tf_viewer/cloak.wgsl".into()
    }
}

pub type CloakMaterial = ExtendedMaterial<StandardMaterial, CloakExt>;

pub struct CloakPlugin;

impl Plugin for CloakPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "cloak.wgsl");
        app.add_plugins(MaterialPlugin::<CloakMaterial>::default());
    }
}

/// A viewmodel mesh drawn cloaked, with its own material to put back.
#[derive(Component)]
pub struct Cloaked(Handle<StandardMaterial>);

fn tuned(d: Vec4) -> Vec4 {
    let v: Vec<f32> = std::env::var("TF_CLOAK").unwrap_or_default().split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() == 4 { Vec4::new(v[0], v[1], v[2], v[3]) } else { d }
}

/// Swap the first-person meshes to the cloak material while the Pilot is cloaked.
#[allow(clippy::type_complexity)]
pub fn cloak_viewmodel(
    mut commands: Commands,
    status: Res<PilotStatus>,
    control: Res<Control>,
    mut mats: ResMut<Assets<CloakMaterial>>,
    mut shared: Local<Option<Handle<CloakMaterial>>>,
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>, &RenderLayers), Without<Cloaked>>,
    cloaked: Query<(Entity, &Cloaked)>,
) {
    let on = status.cloaked() && *control == Control::Pilot && std::env::var_os("TF_NO_CLOAK_FX").is_none();
    if on {
        let mat = shared
            .get_or_insert_with(|| {
                mats.add(CloakMaterial {
                    // In the transmissive phase (specular_transmission), drawn opaque-style.
                    base: StandardMaterial { specular_transmission: 1.0, unlit: true, alpha_mode: AlphaMode::Opaque, ..default() },
                    // TF_CLOAK="warp rim exponent tint" overrides the look (testing).
                    extension: CloakExt { params: CloakParams { params: tuned(Vec4::new(0.016, 0.05, 3.0, 0.3)), rim_color: Vec4::new(0.55, 0.75, 1.0, 1.0) } },
                })
            })
            .clone();
        for (e, m, layers) in &meshes {
            if layers.intersects(&RenderLayers::layer(crate::vmcam::VM_LAYER)) {
                commands.entity(e).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(mat.clone()), Cloaked(m.0.clone())));
            }
        }
    } else {
        for (e, c) in &cloaked {
            commands.entity(e).remove::<(MeshMaterial3d<CloakMaterial>, Cloaked)>().insert(MeshMaterial3d(c.0.clone()));
        }
    }
}

/// Phase Shift's first-person look, as sh_phase_shift.gnut plays it: the game's colour
/// correction (`materials/correction/mp_ability_shifter.raw`) over the picture while phased,
/// `P_phase_shift_screen` running and `P_phase_shift_screen_start` when it starts and ends, and
/// `P_phase_shift_main` 30 units in front of the Pilot on coming out. The correction fades
/// in and out over 0.15 s (a guess; the engine blends it).
#[allow(clippy::type_complexity)]
pub fn phase_shift_fx(
    mut commands: Commands,
    time: Res<Time>,
    status: Res<PilotStatus>,
    control: Res<Control>,
    gd: Res<crate::gamedata::GameData>,
    mut images: ResMut<Assets<Image>>,
    mut state: Local<(Option<Option<Handle<Image>>>, bool, f32, Option<Entity>)>,
    cams: Query<(Entity, Option<&crate::colorcorr::ColorCorrection>), With<crate::vmcam::ViewmodelCamera>>,
    main: Query<&GlobalTransform, With<crate::player::MainCamera>>,
    titan: Query<&crate::titankit::KitState>,
) {
    let (lut, was, weight, screen) = &mut *state;
    let lut = lut
        .get_or_insert_with(|| {
            let raw = gd.read_file("materials/correction/mp_ability_shifter.raw").ok()?;
            crate::colorcorr::lut_image(&raw).map(|img| images.add(img))
        })
        .clone();
    // Ronin's Phase Dash phases BT the same way (sh_phase_shift.gnut handles Titans too).
    let titan_phased = *control == Control::Titan && titan.iter().any(|k| k.phase > 0.0);
    let phased = ((status.phased() && *control == Control::Pilot) || titan_phased) && std::env::var_os("TF_NO_PHASE_FX").is_none();
    let cam = main.single().ok();
    let origin = Vec3::ZERO;
    if phased && !*was {
        *screen = Some(commands.spawn((Transform::from_translation(origin), crate::pfx::PfxTrail::new("P_phase_shift_screen"))).id());
        crate::pfx::emit_named(&mut commands, "P_phase_shift_screen_start", origin, Vec3::Y);
    } else if !phased && *was {
        if let Some(e) = screen.take() {
            commands.entity(e).despawn();
        }
        crate::pfx::emit_named(&mut commands, "P_phase_shift_screen_start", origin, Vec3::Y);
        if let Some(c) = cam {
            // At the Pilot's feet, 30 units ahead along the view's level heading.
            let fwd = (c.forward().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::NEG_Z);
            let at = c.translation() - Vec3::Y * 60.0 * crate::player::UNIT + fwd * 30.0 * crate::player::UNIT;
            crate::pfx::emit_named(&mut commands, if *control == Control::Titan { "P_phase_shift_main_XO" } else { "P_phase_shift_main" }, at, Vec3::Y);
        }
    }
    *was = phased;
    let target = if phased { 1.0 } else { 0.0 };
    let step = time.delta_secs() / 0.15;
    *weight = if *weight < target { (*weight + step).min(target) } else { (*weight - step).max(target) };
    let Some(lut) = lut else { return };
    for (e, cc) in &cams {
        if *weight > 0.0 {
            if cc.is_none_or(|c| (c.weight - *weight).abs() > 1e-4) {
                commands.entity(e).insert(crate::colorcorr::ColorCorrection { lut: lut.clone(), weight: *weight });
            }
        } else if cc.is_some() {
            commands.entity(e).remove::<crate::colorcorr::ColorCorrection>();
        }
    }
}
