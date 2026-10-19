//! The grenade trajectory preview (`grenade_arc_indicator_*`): while a grenade launcher is up
//! (aimed down sights unless `grenade_arc_indicator_show_from_hip`), the path its round would
//! take is drawn ahead, following `grenade_arc_indicator_bounce_count` bounces, with a ring
//! where it ends.
//!
//! The look is `P_grenade_arc_proto` (wpn_grenade_arc_proto.pcf): a rope of colour
//! (121, 140, 252) and radius 32 that fades in and out over 50 units at its ends, 20 dots of
//! radius 2 scrolling along it at 0.2 of its length a second, and `grenade_arc_impact_proto`,
//! a ring of radius 10 lying on the surface it reaches. It is built here as one additive mesh
//! rebuilt each frame (the rope's beam texture as a bright core with a soft edge).

use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::pilotweapon::PilotLoadout;
use crate::player::{to_bevy, Collision, MainCamera, PlayerInput, UNIT};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

const COLOR: Vec3 = Vec3::new(121.0 / 255.0, 140.0 / 255.0, 252.0 / 255.0);
const ROPE_RADIUS: f32 = 32.0;
const FADE: f32 = 50.0;
const DOTS: usize = 20;
const DOT_RADIUS: f32 = 2.0;
const DOT_SCROLL: f32 = 0.2;
const RING_RADIUS: f32 = 10.0;
/// Simulation step and the longest path drawn (seconds of flight).
const STEP: f32 = 1.0 / 60.0;
const MAX_FLIGHT: f32 = 4.0;

#[derive(Component)]
pub struct GrenadeArcMesh;

/// The path a grenade launched at `pos` with `vel` takes: its points (game units) and the
/// normal of the surface it ends on.
pub fn simulate(
    world: &Collision,
    mut pos: Vec3,
    mut vel: Vec3,
    gravity: f32,
    def: &crate::weapons::GrenadeDef,
    bounces: u32,
) -> (Vec<Vec3>, Option<Vec3>) {
    let mut pts = vec![pos];
    let mut left = bounces;
    let flight = if def.fuse > 0.0 {
        def.fuse.min(MAX_FLIGHT)
    } else {
        MAX_FLIGHT
    };
    let mut t = 0.0;
    while t < flight {
        vel.z -= 750.0 * gravity * STEP;
        let step = vel * STEP;
        let len = step.length().max(1e-4);
        let dir = step / len;
        match world.0.raycast(
            SVec3::from(pos.to_array()),
            SVec3::from(dir.to_array()),
            len,
        ) {
            Some(h) => {
                pos += dir * h.t;
                pts.push(pos);
                let n = Vec3::from(h.normal.to_array());
                let n = if n.dot(dir) > 0.0 { -n } else { n };
                // A sticky round, or no bounces left: the path ends here.
                if def.stick_fuse.is_some() || left == 0 {
                    return (pts, Some(n));
                }
                left -= 1;
                let vn = n * vel.dot(n);
                let vt = vel - vn;
                let head_on = (-dir).dot(n).clamp(0.0, 1.0);
                let (shallow, sharp, along) = def.bounce;
                vel = vt * (shallow + (sharp - shallow) * head_on) - vn * along;
                pos += n;
            }
            None => {
                pos += step;
                pts.push(pos);
            }
        }
        t += STEP;
    }
    (pts, None)
}

#[allow(clippy::too_many_arguments)]
pub fn grenade_arc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    control: Res<Control>,
    input: Res<PlayerInput>,
    settings: Res<PilotSettings>,
    world: Res<Collision>,
    pilots: Query<(&PlayerPilot, &PilotLoadout)>,
    cams: Query<&GlobalTransform, With<MainCamera>>,
    mut arcs: Query<(&Mesh3d, &mut Visibility), With<GrenadeArcMesh>>,
    mut scroll: Local<f32>,
    (ability_defs, status, throw): (
        Option<Res<crate::pilotability::PilotAbilityDefs>>,
        Res<crate::pilotability::PilotStatus>,
        Res<crate::pilotability::OrdnanceThrow>,
    ),
) {
    *scroll = (*scroll + time.delta_secs() * DOT_SCROLL).fract();
    // A held grenade with a trajectory preview (Ordnance Expert).
    let held = (*control == Control::Pilot)
        .then(|| pilots.single().ok())
        .flatten()
        .zip(ability_defs.as_deref())
        .and_then(|((p, _), defs)| {
            let eye = Vec3::from(p.state.eye(&settings.0).to_array());
            crate::pilotability::ordnance_preview(&world, defs, &status, &throw, &p.state, eye)
        });
    if let (Some((pts, end)), Ok(cam)) = (held, cams.single()) {
        let mesh = build(&pts, end, cam.translation(), *scroll);
        show(&mut commands, &mut meshes, &mut materials, &mut arcs, mesh);
        return;
    }
    let shown = (*control == Control::Pilot)
        .then(|| pilots.single().ok())
        .flatten()
        .and_then(|(p, lo)| {
            let w = lo.guns.get(lo.active)?;
            let g = w.def.grenade?;
            let arc = g.arc?;
            let up = lo.switching.is_none() && !w.reloading() && w.ammo > 0;
            (up && (arc.from_hip || w.ads > 0.5)).then_some((p, w, g, arc))
        });
    let Some((p, w, g, arc)) = shown else {
        for (_, mut v) in &mut arcs {
            *v = Visibility::Hidden;
        }
        return;
    };
    let Some(speed) = w.def.projectile_speed else {
        return;
    };
    let Ok(cam) = cams.single() else {
        return;
    };
    let eye = Vec3::from(p.state.eye(&settings.0).to_array());
    let (yaw, pitch) = (input.yaw, input.pitch);
    let mut dir = Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    );
    if g.pitch_offset != 0.0 {
        let side = dir.cross(Vec3::Z).normalize_or_zero();
        dir = Quat::from_axis_angle(side, g.pitch_offset.to_radians()) * dir;
    }
    let (pts, end) = simulate(
        &world,
        eye + dir * 40.0,
        dir * speed,
        w.def.gravity,
        &g,
        arc.bounces,
    );
    let mesh = build(&pts, end, cam.translation(), *scroll);
    show(&mut commands, &mut meshes, &mut materials, &mut arcs, mesh);
}

/// Put this frame's arc mesh up (made the first time).
fn show(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    arcs: &mut Query<(&Mesh3d, &mut Visibility), With<GrenadeArcMesh>>,
    mesh: Mesh,
) {
    match arcs.iter_mut().next() {
        Some((m, mut v)) => {
            if let Some(slot) = meshes.get_mut(&m.0) {
                *slot = mesh;
            }
            *v = Visibility::Inherited;
        }
        None => {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    ..default()
                })),
                Transform::IDENTITY,
                Visibility::Inherited,
                bevy::light::NotShadowCaster,
                bevy::camera::visibility::NoFrustumCulling,
                GrenadeArcMesh,
                Name::new("grenade arc"),
            ));
        }
    }
}

/// The rope, its dots and the end ring as one camera-facing mesh (Bevy space).
fn build(pts: &[Vec3], end: Option<Vec3>, cam: Vec3, scroll: f32) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let pts: Vec<Vec3> = pts.iter().map(|p| to_bevy(*p)).collect();
    let mut dist = vec![0.0f32; pts.len()];
    for i in 1..pts.len() {
        dist[i] = dist[i - 1] + pts[i].distance(pts[i - 1]) / UNIT;
    }
    let total = dist.last().copied().unwrap_or(0.0).max(1e-3);
    let fade = |d: f32| (d / FADE).min((total - d) / FADE).clamp(0.0, 1.0);
    // Additive: the brightness rides on the colour (alpha 1, so it isn't squared by the
    // premultiply).
    let rgba = |a: f32| [COLOR.x * a, COLOR.y * a, COLOR.z * a, 1.0];
    // The rope: five vertices across (edge, glow, core, glow, edge).
    let across = [(-1.0, 0.0), (-0.4, 0.3), (0.0, 1.0), (0.4, 0.3), (1.0, 0.0)];
    let half = ROPE_RADIUS * 0.5 * UNIT;
    for (i, p) in pts.iter().enumerate() {
        let tangent = if i + 1 < pts.len() {
            pts[i + 1] - *p
        } else if i > 0 {
            *p - pts[i - 1]
        } else {
            Vec3::X
        };
        // Seen end-on (the start of the path, from behind the gun) the strip narrows away
        // instead of spinning to an arbitrary side.
        let side = tangent
            .normalize_or(Vec3::X)
            .cross((cam - *p).normalize_or(Vec3::Y));
        let a = fade(dist[i]);
        for (k, b) in across {
            pos.push((*p + side * (k * half)).to_array());
            col.push(rgba(a * b));
        }
    }
    for i in 0..pts.len().saturating_sub(1) as u32 {
        for k in 0..4u32 {
            let (a, b) = (i * 5 + k, (i + 1) * 5 + k);
            idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    // The dots, scrolling along it.
    let at = |d: f32| -> Vec3 {
        let i = dist
            .partition_point(|x| *x < d)
            .clamp(1, pts.len().max(2) - 1);
        let (d0, d1) = (dist[i - 1], dist[i]);
        let f = ((d - d0) / (d1 - d0).max(1e-4)).clamp(0.0, 1.0);
        pts[i - 1].lerp(pts[i], f)
    };
    if pts.len() >= 2 {
        let r = DOT_RADIUS * UNIT;
        for n in 0..DOTS {
            let d = ((n as f32 / DOTS as f32 + scroll).fract()) * total;
            let c = at(d);
            let to_cam = (cam - c).normalize_or(Vec3::Y);
            let u = to_cam.any_orthonormal_vector();
            let v = to_cam.cross(u);
            // A round sprite: bright centre, dark rim.
            let base = pos.len() as u32;
            pos.push(c.to_array());
            col.push(rgba(fade(d)));
            let rim = 8u32;
            for s in 0..rim {
                let ang = s as f32 / rim as f32 * std::f32::consts::TAU;
                pos.push((c + (u * ang.cos() + v * ang.sin()) * r).to_array());
                col.push(rgba(0.0));
            }
            for s in 0..rim {
                idx.extend([base, base + 1 + s, base + 1 + (s + 1) % rim]);
            }
        }
    }
    // The impact ring, lying on the surface.
    if let (Some(n), Some(last)) = (end, pts.last()) {
        let n = to_bevy(n).normalize_or(Vec3::Y);
        let c = *last + n * (0.5 * UNIT);
        let u = n.any_orthonormal_vector();
        let v = n.cross(u);
        let (outer, inner) = (RING_RADIUS * UNIT, RING_RADIUS * 0.7 * UNIT);
        let seg = 32u32;
        let base = pos.len() as u32;
        for s in 0..seg {
            let ang = s as f32 / seg as f32 * std::f32::consts::TAU;
            let d = u * ang.cos() + v * ang.sin();
            pos.push((c + d * outer).to_array());
            col.push(rgba(0.0));
            pos.push((c + d * (outer + inner) * 0.5).to_array());
            col.push(rgba(1.0));
            pos.push((c + d * inner).to_array());
            col.push(rgba(0.0));
        }
        for s in 0..seg {
            let (a, b) = (base + s * 3, base + ((s + 1) % seg) * 3);
            for k in 0..2 {
                idx.extend([a + k, b + k, a + k + 1, a + k + 1, b + k, b + k + 1]);
            }
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
    .with_inserted_indices(Indices::U32(idx))
}
