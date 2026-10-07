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
        Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
        bevy::render::view::Hdr,
        Msaa::Off,
        bevy::camera::Exposure { ev100: crate::env::EV100 },
        // Bevy 0.18 also clips at `near_clip_plane`, whose default is the 0.1 near plane.
        Projection::Perspective({
            // TF_VM_NEAR=metres overrides the near plane (testing).
            let near = std::env::var("TF_VM_NEAR").ok().and_then(|v| v.parse().ok()).unwrap_or(VM_NEAR);
            PerspectiveProjection { fov: vertical_from_43(PILOT_VM_FOV), near, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -near), ..default() }
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
    main: Query<(&Transform, Option<&bevy::light::EnvironmentMapLight>), (With<MainCamera>, Without<ViewmodelCamera>)>,
    mut cam: Query<(Entity, &mut Transform, &mut Projection, Option<&bevy::light::EnvironmentMapLight>), With<ViewmodelCamera>>,
    mut commands: Commands,
) {
    // The world camera's Transform as set this frame (it has no parent); its GlobalTransform
    // is last frame's until transform propagation, which left the viewmodels a frame (4 units
    // at sprint speed) ahead of their camera.
    let Ok((main_t, env)) = main.single() else { return };
    let Ok((e, mut t, mut p, my_env)) = cam.single_mut() else { return };
    *t = *main_t;
    let fov = if matches!(*control, crate::pilotctl::Control::Titan) { TITAN_VM_FOV } else { PILOT_VM_FOV };
    if let Projection::Perspective(pp) = p.as_mut() {
        pp.fov = vertical_from_43(fov);
    }
    // Reflections: the same cubemap as the world camera.
    if let Some(env) = env {
        if my_env.is_none_or(|m| m.specular_map != env.specular_map) {
            commands.entity(e).insert(env.clone());
        }
    }
}

/// Move first-person meshes to their layer as they appear.
pub fn tag_meshes(
    mut commands: Commands,
    roots: Query<Entity, With<Viewmodel>>,
    children: Query<&Children>,
    untagged: Query<(), (With<Mesh3d>, Without<RenderLayers>)>,
) {
    for root in &roots {
        for e in children.iter_descendants(root) {
            if untagged.contains(e) {
                commands.entity(e).insert((RenderLayers::layer(VM_LAYER), NotShadowCaster));
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
