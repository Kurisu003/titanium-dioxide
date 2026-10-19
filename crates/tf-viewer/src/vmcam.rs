//! First-person models (gun viewmodels, the Pilot's arms, BT's cockpit) are drawn by their own
//! camera on top of the world, at the player settings' `viewmodelfov` (70 for Pilots, 75 in a
//! Titan) whatever the world camera's zoom, as the game draws them. They never clip into
//! walls, and a sniper scope stays the size its model was made for while the world zooms.

use crate::player::MainCamera;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

/// Render layer of first-person models (the skybox uses 1).
pub const VM_LAYER: usize = 2;

/// Marks the root of a first-person model; its meshes are moved to `VM_LAYER`.
#[derive(Component)]
pub struct Viewmodel;

#[derive(Component)]
pub struct ViewmodelCamera;

/// `viewmodelfov` from the player settings (pilot_base.set, titan_base.set), horizontal
/// degrees at 4:3 like every Source FOV.
pub const PILOT_VM_FOV: f32 = 70.0;
pub const TITAN_VM_FOV: f32 = 75.0;

/// Near plane of the viewmodel camera (metres): an ADS scope's tube starts at the eye.
const VM_NEAR: f32 = 0.025;

/// The game's horizontal 4:3 FOV as the vertical FOV our cameras use.
pub fn vertical_from_43(h_deg: f32) -> f32 {
    2.0 * ((h_deg.to_radians() * 0.5).tan() * 0.75).atan()
}

pub fn spawn_camera(commands: &mut Commands) {
    commands.spawn((
        Camera3d::default(),
        // After the world, keeping its picture; its own depth buffer starts empty.
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy::render::view::Hdr,
        Msaa::Off,
        bevy::camera::Exposure {
            ev100: crate::env::ev100(),
        },
        crate::env::tonemapping(),
        crate::colorcorr::GameTonemap,
        // Bevy 0.18 also clips at `near_clip_plane`, whose default is the 0.1 near plane.
        Projection::Perspective({
            // TF_VM_NEAR=metres overrides the near plane (testing).
            let near = std::env::var("TF_VM_NEAR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(VM_NEAR);
            PerspectiveProjection {
                fov: vertical_from_43(PILOT_VM_FOV),
                near,
                near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -near),
                ..default()
            }
        }),
        RenderLayers::layer(VM_LAYER),
        Transform::default(),
        ViewmodelCamera,
    ));
}

/// Follow the world camera; the lens is the viewmodel FOV for whoever is in control.
#[allow(clippy::type_complexity)]
pub fn follow_camera(
    control: Res<crate::pilotctl::Control>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
    settings: Res<crate::settings::Settings>,
    main: Query<
        (&Transform, Option<&bevy::light::EnvironmentMapLight>),
        (With<MainCamera>, Without<ViewmodelCamera>),
    >,
    mut cam: Query<
        (
            Entity,
            &mut Transform,
            &mut Projection,
            Option<&bevy::light::EnvironmentMapLight>,
        ),
        With<ViewmodelCamera>,
    >,
    mut commands: Commands,
) {
    // The world camera's Transform as set this frame (it has no parent); its GlobalTransform
    // is last frame's until transform propagation, which left the viewmodels a frame (4 units
    // at sprint speed) ahead of their camera.
    let Ok((main_t, env)) = main.single() else {
        return;
    };
    let Ok((e, mut t, mut p, my_env)) = cam.single_mut() else {
        return;
    };
    *t = *main_t;
    let fov = if matches!(*control, crate::pilotctl::Control::Titan) {
        TITAN_VM_FOV
    } else {
        PILOT_VM_FOV
    };
    let mut half = (vertical_from_43(fov) * 0.5).tan();
    // Aiming down sights zooms the Pilot's viewmodel by the same tangent ratio as the world
    // (zoom_fov against the hip FOV, over the zoom's ads_fov window): in 1080p footage the
    // R-201's sight and its ammo digits grow 2.1x from hip to aimed, as 35 over 70 gives, so
    // the sight fills a third of the screen. The Titan's guns only zoom the world.
    if matches!(*control, crate::pilotctl::Control::Pilot) {
        if let Some(g) = loadouts.single().ok().and_then(|l| l.active()) {
            let hip = (crate::settings::BASE_FOV_DEG * settings.fov_scale).to_radians();
            let k = (g.def.zoom_fov.to_radians() * 0.5).tan() / (hip * 0.5).tan();
            let a = crate::vmmotion::ads_fov_blend(g.ads, g.def.ads_fov_frac);
            half *= 1.0 + (k - 1.0) * a;
        }
    }
    if let Projection::Perspective(pp) = p.as_mut() {
        pp.fov = 2.0 * half.atan();
    }
    // Reflections: the same cubemap as the world camera.
    if let Some(env) = env {
        if my_env.is_none_or(|m| m.specular_map != env.specular_map) {
            commands.entity(e).insert(env.clone());
        }
    }
}

/// Move first-person meshes to their layer as they appear. They are always in front of the
/// camera, so they skip frustum culling: that also lets the loading screen's warm-up compile
/// their pipelines while they sit at the origin, out of view.
pub fn tag_meshes(
    mut commands: Commands,
    roots: Query<Entity, With<Viewmodel>>,
    children: Query<&Children>,
    untagged: Query<(), (With<Mesh3d>, Without<RenderLayers>)>,
) {
    for root in &roots {
        for e in children.iter_descendants(root) {
            if untagged.contains(e) {
                commands.entity(e).insert((
                    RenderLayers::layer(VM_LAYER),
                    NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                ));
            }
        }
    }
}

/// Where a point on a first-person model appears in the world: the same screen position at
/// the same depth through the world camera's lens (Source's FormatViewModelAttachment), so
/// tracers and world effects start where the gun is seen.
pub fn to_world(p: Vec3, cam: &GlobalTransform, world_fov: f32, vm_fov: f32) -> Vec3 {
    let inv = cam.affine().inverse();
    let mut c = inv.transform_point3(p);
    let k = (world_fov * 0.5).tan() / (vm_fov * 0.5).tan();
    c.x *= k;
    c.y *= k;
    cam.affine().transform_point3(c)
}

/// Where to put a first-person (`_FP`) particle system emitted at a viewmodel point, given that
/// particles are drawn through the world camera: the same screen position, with the depth
/// stretched so the particles also keep the size they'd have through the viewmodel lens (in
/// ADS the world zooms in, which would otherwise magnify a muzzle flash and pull it inward).
pub fn fx_to_world(p: Vec3, cam: &GlobalTransform, world_fov: f32, vm_fov: f32) -> Vec3 {
    let inv = cam.affine().inverse();
    let mut c = inv.transform_point3(p);
    let k = (world_fov * 0.5).tan() / (vm_fov * 0.5).tan();
    c.z /= k.max(1e-3);
    cam.affine().transform_point3(c)
}

/// Keep only the first shadow cascade of the sun for the viewmodel camera. Bevy builds the full
/// set (four, out to 150 m) for every camera the light reaches, but first-person models are all
/// within a few metres of the eye, well inside the first cascade (0.1-10 m, blending into the
/// next only past 8 m), so the viewmodel camera never samples the others. That saved three
/// shadow passes of the whole map, and their visibility checks, every frame.
pub fn one_shadow_cascade(
    vm: Query<Entity, With<ViewmodelCamera>>,
    mut lights: Query<&mut bevy::light::Cascades>,
) {
    let Ok(vm) = vm.single() else { return };
    for mut c in &mut lights {
        if let Some(v) = c.cascades.get_mut(&vm) {
            v.truncate(1);
        }
    }
}

/// The game plays its FirstPersonSequences (mounts, dismounts, the rodeo) on the first-person
/// proxy, which is drawn in the world at the world FOV, so the hands land on the hatch they
/// were animated against. While one plays, the arms and whatever hangs off them (the rodeo's
/// battery and grenade) move to the world layer: through the viewmodel camera's 70° lens a
/// prop that sits exactly in the socket draws elsewhere on screen than the hull's hole.
pub fn arms_world_layer(
    arms: Option<Res<crate::fparms::FpArms>>,
    rodeo: Res<crate::rodeo::Rodeo>,
    seq: Res<crate::fparms::FpSeq>,
    children: Query<&Children>,
    mut layers: Query<&mut RenderLayers, With<Mesh3d>>,
) {
    let Some(arms) = arms else { return };
    let want = if rodeo.riding() || seq.active.is_some() {
        RenderLayers::layer(0)
    } else {
        RenderLayers::layer(VM_LAYER)
    };
    for e in children.iter_descendants(arms.anchor) {
        if let Ok(mut l) = layers.get_mut(e) {
            if *l != want {
                *l = want.clone();
            }
        }
    }
}
