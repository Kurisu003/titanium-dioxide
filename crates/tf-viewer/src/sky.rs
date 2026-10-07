//! The 3D skybox: the vista models placed around the map's `sky_camera`. As in Source, the
//! skybox scene is drawn around the viewer with no parallax; here it is scaled up so that it
//! sits behind everything in the map (the sky camera's own skyscale), and it ignores lighting, fog and shadows.

use crate::convert::Cache;
use crate::env::MapEnv;
use crate::gamedata::GameData;
use crate::player::{MainCamera, UNIT};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use tf_assets::bsp::Bsp;

/// Props within this distance of the sky camera belong to the skybox.
const SKY_RADIUS: f32 = 9000.0;

#[derive(Component)]
pub struct Sky;

/// Draws the skybox before the main camera, which then renders the world over it with its own
/// depth buffer, so skybox geometry never covers the world (as in Source).
#[derive(Component)]
pub struct SkyCamera;

/// Render layer for skybox geometry.
pub const SKY_LAYER: usize = 1;

#[allow(clippy::too_many_arguments)]
pub fn spawn_sky(
    commands: &mut Commands,
    root: Entity,
    gd: &GameData,
    bsp: &Bsp,
    env: &MapEnv,
    cache: &mut Cache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera { order: -1, ..default() },
        bevy::render::view::Hdr,
        // Explicit so the cockpit boot (pilotctl::cockpit_boot) can dim it with the others.
        bevy::camera::Exposure::default(),
        Msaa::Off,
        bevy::camera::visibility::RenderLayers::layer(SKY_LAYER),
        Transform::default(),
        SkyCamera,
    ));
    let Some(cam) = env.sky_camera else {
        log::info!("no sky camera; plain sky");
        return;
    };
    let Ok(sp) = bsp.static_props() else { return };
    let anchor = commands.spawn((Transform::default(), Visibility::default(), Sky)).id();
    commands.entity(root).add_child(anchor);
    let mut n = 0;
    for p in &sp.props {
        let name = &sp.model_names[p.model as usize];
        if !name.starts_with("models/vistas/") || Vec3::from(p.origin).distance(cam) > SKY_RADIUS {
            continue;
        }
        let Ok((parts, _)) = crate::world::build_static_model(gd, name, cache, meshes, images, materials) else { continue };
        let [pitch, yaw, roll] = p.angles.map(f32::to_radians);
        let rot = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll);
        let tf = Transform {
            translation: (Vec3::from(p.origin) - cam) * env.sky_scale,
            rotation: rot,
            scale: Vec3::splat(p.scale.max(0.0001) * env.sky_scale),
        };
        let holder = commands.spawn((tf, Visibility::default())).id();
        commands.entity(anchor).add_child(holder);
        for (mesh, mat) in parts {
            let sky_mat = materials.get(&mat).cloned().map(|mut m| {
                m.unlit = true;
                m.fog_enabled = false;
                materials.add(m)
            });
            let e = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(sky_mat.unwrap_or(mat)),
                    Transform::IDENTITY,
                    NotShadowCaster,
                    NotShadowReceiver,
                    bevy::camera::visibility::RenderLayers::layer(SKY_LAYER),
                ))
                .id();
            commands.entity(holder).add_child(e);
        }
        n += 1;
        log::info!("skybox: {name}");
    }
    log::info!("{n} skybox models");
}

/// Keep the skybox centred on the camera (game space under the world root), and the sky
/// camera looking the same way with the same lens.
#[allow(clippy::type_complexity)]
pub fn follow_camera(
    cam: Query<(&Transform, &Projection), (With<MainCamera>, Without<SkyCamera>, Without<Sky>)>,
    mut sky_cam: Query<(&mut Transform, &mut Projection), (With<SkyCamera>, Without<Sky>)>,
    mut sky: Query<&mut Transform, (With<Sky>, Without<SkyCamera>)>,
) {
    // The world camera's Transform as set this frame (no parent); its GlobalTransform lags one.
    let Ok((c, proj)) = cam.single() else { return };
    if let Ok((mut t, mut p)) = sky_cam.single_mut() {
        *t = *c;
        *p = proj.clone();
    }
    let b = c.translation;
    let game = Vec3::new(b.x, -b.z, b.y) / UNIT;
    for mut t in &mut sky {
        t.translation = game;
    }
}
