//! Ziplines: the map's `move_rope` entities with `Zipline` 1 (in `_script.ent`), each running to
//! its `NextKey`. The Pilot grabs one with the use key (within `zipline_use_range`) or by
//! jumping into it, rides it (`tf_sim::pilot::ZipRide`) and drops off with jump or crouch.

use crate::gamedata::GameData;
use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::player::{to_bevy, PlayerInput};
use bevy::prelude::*;
use std::collections::HashMap;
use tf_sim::pilot::Zipline;

#[derive(Resource, Default)]
pub struct Ziplines(pub Vec<Zipline>);

/// The map's ziplines.
pub fn load(gd: &GameData, map: &str) -> Vec<Zipline> {
    let Ok(bytes) = gd.read_file(&format!("maps/{map}_script.ent")) else { return Vec::new() };
    let ents = tf_assets::bsp::parse_entities(&String::from_utf8_lossy(&bytes));
    let get = |e: &Vec<(String, String)>, k: &str| e.iter().find(|(kk, _)| kk.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
    let num = |e: &Vec<(String, String)>, k: &str| get(e, k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
    let origin = |e: &Vec<(String, String)>| {
        let v: Vec<f32> = get(e, "origin")?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.len() == 3).then(|| tf_sim::glam::Vec3::new(v[0], v[1], v[2]))
    };
    let ropes: Vec<&Vec<(String, String)>> = ents.iter().filter(|e| get(e, "classname").as_deref() == Some("move_rope")).collect();
    let by_name: HashMap<String, &Vec<(String, String)>> = ropes.iter().filter_map(|e| Some((get(e, "targetname")?, *e))).collect();
    let mut lines = Vec::new();
    for e in &ropes {
        if num(e, "Zipline") != 1.0 {
            continue;
        }
        let Some(next) = get(e, "NextKey").and_then(|n| by_name.get(&n).copied()) else { continue };
        let (Some(a), Some(b)) = (origin(e), origin(next)) else { continue };
        let sag = if num(e, "ZiplineSagEnable") != 0.0 { num(e, "ZiplineSagHeight") } else { 0.0 };
        lines.push(Zipline { a, b, sag, detach: num(e, "ZiplineAutoDetachDistance") });
    }
    if !lines.is_empty() {
        log::info!("{map}: {} ziplines", lines.len());
    }
    lines
}

/// Grab a zipline: the use key near one (taking the key, so it doesn't also try to embark), or
/// jumping into one.
pub fn zipline_grab(
    lines: Option<Res<Ziplines>>,
    control: Res<Control>,
    rodeo: Res<crate::rodeo::Rodeo>,
    settings: Res<PilotSettings>,
    mut input: ResMut<PlayerInput>,
    mut pilots: Query<&mut PlayerPilot>,
) {
    let Some(lines) = lines else { return };
    if lines.0.is_empty() || *control != Control::Pilot || rodeo.riding() {
        return;
    }
    let Ok(mut pilot) = pilots.single_mut() else { return };
    if input.interact && pilot.state.try_zipline(&lines.0, true, &settings.0) {
        input.interact = false;
    } else {
        pilot.state.try_zipline(&lines.0, false, &settings.0);
    }
}

/// The cables, built once the map's ziplines are known: a thin tube along each line's sag.
#[allow(clippy::too_many_arguments)]
pub fn zipline_cables(
    mut commands: Commands,
    lines: Option<Res<Ziplines>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut done: Local<bool>,
) {
    let Some(lines) = lines else { return };
    if *done {
        return;
    }
    *done = true;
    if lines.0.is_empty() {
        return;
    }
    // `cable/zipline` is a Source-style material we don't have: a dark steel cable stands in.
    let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.13), metallic: 0.6, perceptual_roughness: 0.45, ..default() });
    for line in &lines.0 {
        commands.spawn((Mesh3d(meshes.add(cable_mesh(line))), MeshMaterial3d(mat.clone()), Transform::IDENTITY, bevy::light::NotShadowCaster));
    }
}

/// Cable radius in game units (the entities' `Width` is 2; drawn a little thicker so it reads
/// at range, a guess).
const CABLE_RADIUS: f32 = 1.5;

fn cable_mesh(line: &Zipline) -> Mesh {
    const SEGS: usize = 64;
    const SIDES: usize = 6;
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..=SEGS {
        let t = i as f32 / SEGS as f32;
        let p = Vec3::from(line.point(t).to_array());
        let tan = Vec3::from(line.tangent(t).to_array()).normalize_or(Vec3::X);
        let side = tan.cross(Vec3::Z).normalize_or(Vec3::Y);
        let up = side.cross(tan);
        for k in 0..SIDES {
            let a = k as f32 / SIDES as f32 * std::f32::consts::TAU;
            let n = side * a.cos() + up * a.sin();
            pos.push(to_bevy(p + n * CABLE_RADIUS).to_array());
            nrm.push(Vec3::new(n.x, n.z, -n.y).to_array());
        }
    }
    for i in 0..SEGS {
        for k in 0..SIDES {
            let (a, b) = ((i * SIDES + k) as u32, (i * SIDES + (k + 1) % SIDES) as u32);
            let (c, d) = (a + SIDES as u32, b + SIDES as u32);
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    mesh
}

/// Riding sounds: `Player_Zipline_Attach`, the `Player_Zipline_Loop` hum while riding and
/// `Player_Zipline_Detach` on letting go.
pub fn zipline_sounds(mut commands: Commands, pilots: Query<&PlayerPilot>, mut riding: Local<bool>) {
    let now = pilots.single().is_ok_and(|p| p.state.zipline.is_some());
    if now && !*riding {
        crate::audio::event(&mut commands, "Player_Zipline_Attach");
        log::debug!("zipline attach");
    }
    if now {
        crate::audio::loop_hold(&mut commands, "zipline", "Player_Zipline_Loop", None);
    } else if *riding {
        crate::audio::loop_stop(&mut commands, "zipline", Some("Player_Zipline_Detach"));
    }
    *riding = now;
}
