//! The Pilot's third-person body: Cooper's campaign model (`sp_medium_reaper_m.mdl`, the body
//! for the first-person `pov_pilot_medium_reaper_m` arms). It follows the player and is drawn in
//! the free camera and during executions; its legs play the human locomotion sequences for the
//! movement state (run blend by direction, crouch run, slide, wallrun left/right, air float).

use crate::actor::{spawn_actor, Actor, ActorSpec, Layer};
use crate::pilotctl::{Control, PlayerPilot};
use crate::player::CameraMode;
use bevy::prelude::*;
use tf_sim::pilot::PilotMove;

pub const PILOT_BODY_MODEL: &str = "models/humans/pilots/sp_medium_reaper_m.mdl";

const IDLE: &str = "CQB_Idle_MP";
const RUN: [&str; 4] = ["Run_forward_mp", "Run_Backward_mp", "Run_mp_left", "Run_mp_right"];
const CROUCH_RUN: &str = "CrouchRun_forward_mp";
const SLIDE: &str = "MP_Pt_Slide_Float";
const WALLRUN_LEFT: &str = "a_pt_wallrun_left";
const WALLRUN_RIGHT: &str = "a_pt_wallrun_right";
const AIR: &str = "Jump_float_MP";
const WALL_HANG: &str = "pt_wallrun_hang_idle";

#[derive(Resource)]
pub struct PilotBody {
    pub anchor: Entity,
    pub actor: Entity,
    cycle: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_pilot_body(
    mut commands: Commands,
    mut done: Local<bool>,
    gd: Res<crate::gamedata::GameData>,
    root: Option<Res<crate::pilotweapon::WorldRoot>>,
    mut cache: ResMut<crate::convert::Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    if *done {
        return;
    }
    let Some(root) = root else { return };
    *done = true;
    let mut sequences: Vec<&str> = vec![IDLE, CROUCH_RUN, SLIDE, WALLRUN_LEFT, WALLRUN_RIGHT, AIR];
    sequences.extend(RUN);
    sequences.extend(crate::executions::PILOT_EXECUTIONS.iter().map(|e| e.0));
    let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden)).id();
    commands.entity(root.0).add_child(anchor);
    let spec = ActorSpec { path: PILOT_BODY_MODEL, sequences: &sequences, grids: &[], body: &[] };
    match spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
        Ok(a) => {
            commands.insert_resource(PilotBody { anchor, actor: a.entity, cycle: 0.0 });
        }
        Err(e) => log::warn!("pilot body: {e:#}"),
    }
}

/// Place and animate the body from the Pilot's movement; shown only in the free camera
/// (executions show it themselves).
#[allow(clippy::too_many_arguments)]
pub fn update_pilot_body(
    time: Res<Time>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    exec: Res<crate::executions::PilotExecution>,
    body: Option<ResMut<PilotBody>>,
    pilots: Query<&PlayerPilot>,
    mut tfs: Query<(&mut Transform, &mut Visibility)>,
    mut actors: Query<&mut Actor>,
) {
    let Some(mut body) = body else { return };
    if exec.active() {
        return;
    }
    let Ok(pilot) = pilots.single() else { return };
    let show = *control == Control::Pilot && *mode == CameraMode::Free;
    let s = &pilot.state;
    let pos = Vec3::from(s.pos.to_array());
    if let Ok((mut tf, mut vis)) = tfs.get_mut(body.anchor) {
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        tf.translation = pos;
        tf.rotation = Quat::from_rotation_z(s.yaw);
    }
    if !show {
        return;
    }
    let Ok(mut a) = actors.get_mut(body.actor) else { return };
    a.autoplay = false;
    a.aim = None;
    let dt = time.delta_secs();
    let v = Vec2::new(s.vel.x, s.vel.y);
    let speed = v.length();
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let left = Vec2::new(-s.yaw.sin(), s.yaw.cos());
    let clip = |a: &Actor, n: &str| a.clip(n);
    let looped = |a: &Actor, c: usize, cycle: &mut f32, rate: f32| {
        *cycle = (*cycle + dt * rate / a.clips[c].duration.max(1e-3)).rem_euclid(1.0);
        Layer { clip: c, cycle: *cycle, weight: 1.0 }
    };
    let mut cycle = body.cycle;
    let layers: Vec<Layer> = match s.mode {
        PilotMove::Slide => clip(&a, SLIDE).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default(),
        PilotMove::WallRun { normal, .. } => {
            // The wall on the Pilot's left has its normal pointing to the right.
            let wall_left = Vec2::new(normal.x, normal.y).dot(left) < 0.0;
            let n = if wall_left { WALLRUN_LEFT } else { WALLRUN_RIGHT };
            clip(&a, n).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default()
        }
        PilotMove::WallHang { .. } => clip(&a, WALL_HANG).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default(),
        PilotMove::Air | PilotMove::Mantle { .. } => clip(&a, AIR).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default(),
        PilotMove::Ground if speed < 20.0 => clip(&a, IDLE).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default(),
        PilotMove::Ground => {
            if s.crouched {
                clip(&a, CROUCH_RUN).map(|c| vec![looped(&a, c, &mut cycle, 1.0)]).unwrap_or_default()
            } else {
                // Blend the four run directions by the movement direction; play them at the
                // speed their root motion covers.
                let d = Vec2::new(v.dot(fwd), v.dot(left)) / speed;
                let w = [d.x.max(0.0), (-d.x).max(0.0), d.y.max(0.0), (-d.y).max(0.0)];
                let ids: Vec<Option<usize>> = RUN.iter().map(|n| clip(&a, n)).collect();
                let lead = ids[0].or(ids.iter().flatten().next().copied());
                let rate = lead.map(|c| speed / a.clips[c].ground_speed.max(1.0)).unwrap_or(1.0);
                if let Some(c) = lead {
                    cycle = (cycle + dt * rate / a.clips[c].duration.max(1e-3)).rem_euclid(1.0);
                }
                ids.iter().zip(w).filter_map(|(c, w)| c.filter(|_| w > 0.01).map(|c| Layer { clip: c, cycle, weight: w })).collect()
            }
        }
    };
    body.cycle = cycle;
    if !layers.is_empty() {
        a.layers = layers;
    }
}
