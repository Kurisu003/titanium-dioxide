//! Synced executions, from the game's melee scripts and datatables:
//! - Titan executions (`_melee_synced_titan.gnut`, `datatable/titan_executions.rpak`): BT melees
//!   a doomed enemy Titan and both play their paired sequences from one shared reference point
//!   (the victim), riding each sequence's root motion. The attacker is invulnerable
//!   (TITAN_EXECUTION_ATTACKER_IS_INVULNERABLE) and the victim dies when the sequence ends.
//!   The campaign picks BT's execution per map (`_sp_loadouts.nut` titanExecution).
//! - Pilot executions on grunts (`sh_melee_synced_human.gnut`): melee from behind
//!   (direction <-1,0,0>, minDot 0.2) within HUMAN_EXECUTION_RANGE 115 and HUMAN_EXECUTION_ANGLE
//!   40 of where you look. The victim is the reference (isAttackerRef false), the camera is in
//!   third person, and the victim dies at the end. Otherwise melee is the empty-handed punch
//!   (`melee_pilot_emptyhanded.txt`).
//! The camera follows the attacker's animated camera bone (jx_c_camera) and looks at the victim.

use crate::actor::{Actor, Layer};
use crate::combat::TitanHealth;
use crate::pilotctl::{Control, PlayerPilot};
use crate::player::{to_bevy, CameraMode, PlayerInput, PlayerTitan};
use crate::targets::Enemy;
use crate::MainCamera;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

/// BT's executions: (campaign ref, attacker sequence, victim sequence).
pub const BT_EXECUTIONS: [(&str, &str, &str); 3] = [
    ("execution_bt_flip", "bt_synced_titan_execute_flip_takedown_A", "titan_synced_bt_execute_flip_takedown_V"),
    ("execution_bt_kickshoot", "bt_synced_titan_execute_kickshoot_A", "titan_synced_bt_execute_kickshoot_V"),
    ("execution_bt_pilotrip", "bt_synced_titan_execute_pilot_rip_A", "titan_synced_bt_execute_pilot_rip_V"),
];

/// Pilot executions on humans that need no extra props: (attacker, victim).
pub const PILOT_EXECUTIONS: [(&str, &str); 2] = [
    ("pt_mp_execution_attacker_nb", "pt_mp_execution_victim_nb"),
    ("pt_mp_execution_attacker_kick", "pt_mp_execution_victim_kick"),
];

/// GetTitanLevelLoadoutDefaultsForMapname: the campaign's execution for each level.
pub fn bt_execution_for_map(map: &str) -> usize {
    match map {
        "sp_sewers1" | "sp_skyway_v1" => 1,
        "sp_boomtown_end" => 2,
        _ => 0,
    }
}

/// Reach for starting a Titan execution: BT's punch range plus the victim's size.
const TITAN_EXECUTION_RANGE: f32 = 280.0;
const TITAN_EXECUTION_ANGLE: f32 = 60.0;
/// FirstPersonSequenceStruct blendTime: the attacker slides into the sequence's start.
const BLEND_TIME: f32 = 0.25;
const HUMAN_EXECUTION_RANGE: f32 = 115.0;
const HUMAN_EXECUTION_ANGLE: f32 = 40.0;
const HUMAN_EXECUTION_MIN_DOT: f32 = 0.2;

/// Every Titan victim sequence, for the enemy actors' sequence lists.
pub fn titan_victim_sequences() -> Vec<&'static str> {
    BT_EXECUTIONS.iter().map(|e| e.2).collect()
}

/// Grunt victim sequences.
/// A uniform random number in 0..1 from a counter (melee view kick).
fn rand01(n: &mut usize) -> f32 {
    *n = n.wrapping_mul(1103515245).wrapping_add(12345);
    ((*n >> 8) % 10000) as f32 / 10000.0
}

pub fn human_victim_sequences() -> Vec<&'static str> {
    PILOT_EXECUTIONS.iter().map(|e| e.1).collect()
}

/// How much of the empty-handed melee sequences (`ptpov_emptyhand.mdl` melee_0N_seq, 1.0 s
/// long) plays before the gun raises: the arms are out of view by mid-sequence, so the rest is
/// cut (reconstruction; `melee_attack_animtime` is 0). AE_MELEE_ATTACK opens at cycle 0.2.
pub const MELEE_ANIM_SECS: f32 = 0.5;
pub const MELEE_HIT_SECS: f32 = 0.2;

/// The empty-handed melee in progress: seconds in, the sequence picked, whom it lunges at and
/// whether it connected.
#[derive(Resource, Default)]
pub struct PilotMelee {
    pub t: Option<f32>,
    pub seq: usize,
    pub hit: bool,
    struck: bool,
    target: Option<Entity>,
    lunge: Option<(Vec3, Vec3)>,
    pub raise: f32,
}

impl PilotMelee {
    /// The gun is out of the hands (melee swing or the raise after it).
    pub fn busy(&self) -> bool {
        self.t.is_some()
    }
}

/// Pilot melee from `melee_pilot_emptyhanded.txt`.
#[derive(Resource, Clone, Debug)]
pub struct PilotMeleeDef {
    pub damage: f32,
    pub range: f32,
    pub lunge_range: f32,
    /// `melee_lunge_target_angle` (degrees either side) and `melee_lunge_time`.
    pub lunge_angle: f32,
    pub lunge_time: f32,
    /// `melee_raise_recovery_animtime_normal` / `_quick` (after a hit): how long the gun takes
    /// to come back up.
    pub raise_normal: f32,
    pub raise_quick: f32,
    /// The punch's view kick (`viewkick_*`, spring "melee").
    pub kick: crate::vmmotion::KickDef,
}

impl PilotMeleeDef {
    pub fn load(read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let s = tf_assets::settings::PlayerSettings::load("scripts/weapons/melee_pilot_emptyhanded.txt", true, read).unwrap_or_default();
        let f = |k: &str, d: f32| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).and_then(|v| v.parse().ok()).unwrap_or(d);
        Self {
            damage: f("melee_damage", 100.0),
            range: f("melee_range", 60.0),
            lunge_range: f("melee_lunge_target_range", 110.0),
            lunge_angle: f("melee_lunge_target_angle", 30.0),
            lunge_time: f("melee_lunge_time", 0.3),
            raise_normal: f("melee_raise_recovery_animtime_normal", 0.75),
            raise_quick: f("melee_raise_recovery_animtime_quick", 0.4),
            kick: crate::vmmotion::KickDef::from_settings(&s),
        }
    }
}

pub struct Running {
    pub victim: Entity,
    pub t: f32,
    /// The shared reference: the victim's position and the facing both sequences start from.
    origin: Vec3,
    yaw: f32,
    attacker: &'static str,
    victim_seq: &'static str,
    duration: f32,
    /// Where the attacker was, for the blend in.
    start: Vec3,
    prev_mode: CameraMode,
    /// BT's health when it started (the attacker is invulnerable).
    health: Option<crate::vitals::Vitals>,
}

#[derive(Resource, Default)]
pub struct TitanExecution(pub Option<Running>);

#[derive(Resource, Default)]
pub struct PilotExecution(pub Option<Running>);

impl PilotExecution {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

/// Root motion of `clip` at `t`, placed at the shared reference: (position, yaw).
fn place(actor: &Actor, clip: &str, t: f32, origin: Vec3, yaw: f32) -> Option<(usize, Vec3, f32, f32)> {
    let c = actor.clip(clip)?;
    let cl = &actor.clips[c];
    let m = cl.movement_at(t).unwrap_or([0.0; 4]);
    let pos = origin + Quat::from_rotation_z(yaw) * Vec3::new(m[0], m[1], m[2]);
    Some((c, pos, yaw + m[3].to_radians(), cl.duration))
}

/// Drive one enemy through its victim sequence.
fn drive_victim(actors: &mut Query<&mut Actor>, e: &mut Enemy, etf: &mut Transform, seq: &str, t: f32, origin: Vec3, yaw: f32) {
    let Ok(mut a) = actors.get_mut(e.actor) else { return };
    let Some((c, pos, y, d)) = place(&a, seq, t, origin, yaw) else { return };
    a.autoplay = false;
    a.aim = None;
    a.layers = vec![Layer { clip: c, cycle: (t / d.max(1e-3)).min(0.999), weight: 1.0 }];
    e.pos = pos;
    e.state.pos = SVec3::new(pos.x, pos.y, pos.z);
    e.state.vel = SVec3::ZERO;
    e.state.yaw = y;
    etf.translation = pos;
    etf.rotation = Quat::from_rotation_z(y);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn titan_execution(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    args: Res<crate::Args>,
    mut mode: ResMut<CameraMode>,
    mut input: ResMut<PlayerInput>,
    mut exec: ResMut<TitanExecution>,
    mut titans: Query<(&mut PlayerTitan, &mut Transform, &mut TitanHealth), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &mut Transform), Without<PlayerTitan>>,
    mut actors: Query<&mut Actor>,
) {
    let dt = time.delta_secs().min(0.1);
    let Ok((mut titan, mut tf, mut health)) = titans.single_mut() else { return };
    // TF_EXEC_TEST: debug toggle that dooms the first landed enemy Titan and stands it in front
    // of BT, so a scripted melee executes it.
    if let Some(want) = std::env::var("TF_EXEC_TEST").ok().filter(|_| exec.0.is_none() && *control == Control::Titan) {
        let bt = Vec3::from(titan.state.pos.to_array());
        // Along the view, so the crosshair is on it in the cockpit.
        let fwd = Vec3::new(input.yaw.cos(), input.yaw.sin(), 0.0);
        // TF_EXEC_TEST=<class> (ion, scorch, ...) picks that class; anything else, the first.
        let want = want.to_ascii_lowercase();
        if let Some((_, mut e, mut etf)) = enemies.iter_mut().find(|(_, e, _)| e.alive() && !e.infantry && !e.arriving() && !e.held && e.sound_type.is_none_or(|t| want == "1" || t == want)) {
            if !e.doomed() {
                let hp = e.v.health + e.v.shield;
                e.damage_unblockable(hp, false);
            }
            let p = bt + fwd * 260.0;
            // Keep BT whole while the test victim stands there shooting.
            health.v.health = health.v.max_health;
            health.v.shield = health.v.max_shield;
            health.v.doomed = None;
            e.pos = p;
            e.state.pos = SVec3::new(p.x, p.y, p.z);
            e.state.yaw = input.yaw + std::f32::consts::PI;
            etf.translation = p;
            etf.rotation = Quat::from_rotation_z(e.state.yaw);
        }
    }
    if exec.0.is_none() {
        let can = *control == Control::Titan && input.melee && health.dead_for.is_none() && titan.override_anim.is_none() && *mode != CameraMode::Free;
        if !can {
            return;
        }
        let bt = Vec3::from(titan.state.pos.to_array());
        let fwd = Vec2::new(titan.state.yaw.cos(), titan.state.yaw.sin());
        let victim = enemies
            .iter()
            .filter(|(_, e, _)| e.alive() && !e.infantry && e.doomed() && !e.held && !e.arriving())
            .filter_map(|(id, e, _)| {
                let to = (e.pos - bt).truncate();
                let dist = to.length() - e.radius;
                let ang = to.normalize_or_zero().dot(fwd).clamp(-1.0, 1.0).acos().to_degrees();
                (dist < TITAN_EXECUTION_RANGE && ang < TITAN_EXECUTION_ANGLE).then_some((id, dist))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let Some(victim) = victim else { return };
        // The melee press starts the execution instead of a punch.
        input.melee = false;
        let (_, mut e, _) = enemies.get_mut(victim).unwrap();
        let (_, a, v) = BT_EXECUTIONS[bt_execution_for_map(&args.map)];
        let to = (e.pos - bt).truncate();
        let yaw = to.y.atan2(to.x);
        e.held = true;
        let duration = actors.get(e.actor).ok().and_then(|ac| ac.clip(v).map(|c| ac.clips[c].duration)).unwrap_or(5.0);
        log::info!("titan execution: {a} / {v} ({duration:.2}s); BT at {bt:.0}, victim at {:.0}", e.pos);
        for (who, actor_e, clip) in [("attacker", titan.actor, a), ("victim", e.actor, v)] {
            if let Some((ac, c)) = actors.get(actor_e).ok().and_then(|ac| ac.clip(clip).map(|c| (ac, c))) {
                let cl = &ac.clips[c];
                let p0 = cl.sample(0.0);
                let p1 = cl.sample(0.999);
                log::info!("  {who} {clip}: {} frames, root bone0 {:.0} -> {:.0}, bone1 {:.0} -> {:.0}, movement {:?} -> {:?}", cl.frames.len(), p0[0].0, p1[0].0, p0[1].0, p1[1].0, cl.movement.first(), cl.movement.last());
            }
        }
        crate::audio::event_at(&mut commands, "Atlas_3p_Sync_Melee", e.pos);
        exec.0 = Some(Running { victim, t: 0.0, origin: e.pos, yaw, attacker: a, victim_seq: v, duration, start: bt, prev_mode: *mode, health: Some(health.v.clone()) });
        *mode = CameraMode::Third;
    }
    let Some(run) = exec.0.as_mut() else { return };
    run.t += dt;
    let t = run.t;
    input.melee = false;
    // BT rides the attacker sequence's root motion from the shared reference.
    if let Some((_, pos, yaw, _)) = actors.get(titan.actor).ok().and_then(|a| place(a, run.attacker, t, run.origin, run.yaw)) {
        let k = (t / BLEND_TIME).min(1.0);
        let pos = run.start.lerp(pos, k);
        titan.state.pos = SVec3::new(pos.x, pos.y, pos.z);
        titan.state.vel = SVec3::ZERO;
        titan.state.yaw = yaw;
        tf.translation = pos;
        tf.rotation = Quat::from_rotation_z(yaw);
    }
    titan.override_anim = Some((run.attacker.to_string(), t));
    if let Some(v) = &run.health {
        health.v = v.clone();
    }
    let done = t >= run.duration;
    match enemies.get_mut(run.victim) {
        Ok((_, mut e, mut etf)) => {
            drive_victim(&mut actors, &mut e, &mut etf, run.victim_seq, t, run.origin, run.yaw);
            if done {
                e.held = false;
                e.executed = true;
                e.damage_bypass(1e9);
            }
        }
        Err(_) => {}
    }
    if done {
        titan.override_anim = None;
        *mode = run.prev_mode;
        exec.0 = None;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn pilot_execution(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    def: Option<Res<PilotMeleeDef>>,
    body: Option<Res<crate::pilotbody::PilotBody>>,
    mut input: ResMut<PlayerInput>,
    mut exec: ResMut<PilotExecution>,
    mut pick: Local<usize>,
    mut pilots: Query<&mut PlayerPilot>,
    mut enemies: Query<(Entity, &mut Enemy, &mut Transform)>,
    mut anchors: Query<&mut Transform, Without<Enemy>>,
    mut actors: Query<&mut Actor>,
    (mut melee, mut punch, springs): (ResMut<PilotMelee>, ResMut<crate::vmmotion::ViewPunch>, Res<crate::vmmotion::WeaponSprings>),
) {
    let dt = time.delta_secs().min(0.1);
    let Ok(mut pilot) = pilots.single_mut() else { return };
    let Some(body) = body else { return };
    // The empty-handed melee under way: lunge, strike at the attack event, then recover.
    if let (Some(t0), Some(d)) = (melee.t, def.as_deref()) {
        let t = t0 + dt;
        melee.t = Some(t);
        if let Some((from, to)) = melee.lunge {
            let k = (t / d.lunge_time.max(0.01)).min(1.0);
            let p = from.lerp(to, k);
            pilot.state.pos = SVec3::new(p.x, p.y, pilot.state.pos.z);
            if k >= 1.0 {
                melee.lunge = None;
                pilot.state.vel.x = 0.0;
                pilot.state.vel.y = 0.0;
            }
        }
        if t >= MELEE_HIT_SECS && !melee.struck {
            melee.struck = true;
            let me = Vec3::from(pilot.state.pos.to_array());
            let look = Vec2::new(pilot.state.yaw.cos(), pilot.state.yaw.sin());
            // The lunge target if it is still in reach, else whoever is right in front.
            let reach = |e: &Enemy| (e.pos - me).truncate().length() - e.radius < d.range + 16.0;
            let target = melee.target.filter(|&id| enemies.get(id).is_ok_and(|(_, e, _)| e.alive() && reach(&e))).or_else(|| {
                enemies
                    .iter()
                    .filter(|(_, e, _)| e.alive() && e.infantry && reach(e) && (e.pos - me).truncate().normalize_or_zero().dot(look) > 0.7)
                    .map(|(id, ..)| id)
                    .next()
            });
            if let Some((_, mut e, _)) = target.and_then(|id| enemies.get_mut(id).ok()) {
                e.damage_unblockable(d.damage, false);
                melee.hit = true;
                crate::audio::event(&mut commands, "Pilot_Mvmt_Melee_Hit_1P");
                log::info!("pilot melee hit for {}", d.damage);
            }
            let r = [rand01(&mut *pick), rand01(&mut *pick), rand01(&mut *pick), rand01(&mut *pick)];
            punch.kick(&d.kick, &springs, 0.0, false, 1.0, r);
            melee.raise = if melee.hit { d.raise_quick } else { d.raise_normal };
        }
        if t >= MELEE_ANIM_SECS + melee.raise {
            melee.t = None;
        }
        input.melee = false;
    }
    // TF_EXEC_TEST: stand the nearest grunt in front of the Pilot with its back turned.
    if std::env::var_os("TF_EXEC_TEST").is_some() && exec.0.is_none() && *control == Control::Pilot {
        let me = Vec3::from(pilot.state.pos.to_array());
        let fwd = Vec3::new(pilot.state.yaw.cos(), pilot.state.yaw.sin(), 0.0);
        if let Some((_, mut e, mut etf)) = enemies.iter_mut().find(|(_, e, _)| e.alive() && e.infantry && !e.held) {
            let p = me + fwd * 70.0;
            e.pos = p;
            e.state.pos = SVec3::new(p.x, p.y, p.z);
            e.state.yaw = pilot.state.yaw;
            etf.translation = p;
            etf.rotation = Quat::from_rotation_z(pilot.state.yaw);
        }
    }
    if exec.0.is_none() {
        if *control != Control::Pilot || !input.melee {
            return;
        }
        input.melee = false;
        let me = Vec3::from(pilot.state.pos.to_array());
        let look = Vec2::new(pilot.state.yaw.cos(), pilot.state.yaw.sin());
        // Execution: a grunt in reach, in view, seen from behind.
        let target = enemies
            .iter()
            .filter(|(_, e, _)| e.alive() && e.infantry && !e.held)
            .filter_map(|(id, e, _)| {
                let to = (e.pos - me).truncate();
                let dist = to.length();
                let ang = to.normalize_or_zero().dot(look).clamp(-1.0, 1.0).acos().to_degrees();
                let their_fwd = Vec2::new(e.state.yaw.cos(), e.state.yaw.sin());
                // The attacker must be at the victim's local -X.
                let behind = (-to).normalize_or_zero().dot(-their_fwd);
                (dist < HUMAN_EXECUTION_RANGE + e.radius && ang < HUMAN_EXECUTION_ANGLE).then_some((id, dist, behind))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match target {
            Some((id, _, behind)) if behind >= HUMAN_EXECUTION_MIN_DOT => {
                let (_, mut e, _) = enemies.get_mut(id).unwrap();
                *pick = (*pick + 1) % PILOT_EXECUTIONS.len();
                let (a, v) = PILOT_EXECUTIONS[*pick];
                e.held = true;
                let duration = actors.get(e.actor).ok().and_then(|ac| ac.clip(v).map(|c| ac.clips[c].duration)).unwrap_or(3.0);
                log::info!("pilot execution: {a} / {v} ({duration:.2}s)");
                exec.0 = Some(Running { victim: id, t: 0.0, origin: e.pos, yaw: e.state.yaw, attacker: a, victim_seq: v, duration, start: me, prev_mode: CameraMode::Third, health: None });
            }
            _ if melee.busy() => {}
            _ => {
                // The empty-handed melee: a swing (melee_pilot_emptyhanded.txt) that lunges at
                // the nearest grunt within melee_lunge_target_range and _angle, striking at
                // the sequence's attack event.
                let Some(def) = def else { return };
                let cos = def.lunge_angle.to_radians().cos();
                let near = enemies
                    .iter()
                    .filter(|(_, e, _)| e.alive() && e.infantry)
                    .map(|(id, e, _)| (id, (e.pos - me).truncate().length() - e.radius, e.pos))
                    .filter(|(_, d, p)| *d < def.lunge_range.max(def.range) && (*p - me).truncate().normalize_or_zero().dot(look) > cos)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                *pick += 1;
                *melee = PilotMelee { t: Some(0.0), seq: *pick, target: near.map(|n| n.0), raise: def.raise_normal, ..Default::default() };
                // Lunge to arm's length (melee_range) over melee_lunge_time.
                if let Some((_, dist, p)) = near.filter(|n| n.1 > def.range * 0.7) {
                    let dir = (p - me).truncate().normalize_or_zero();
                    let to = me + (dir * (dist - def.range * 0.7)).extend(0.0);
                    melee.lunge = Some((me, to));
                }
            }
        }
        return;
    }
    let Some(run) = exec.0.as_mut() else { return };
    run.t += dt;
    let t = run.t;
    input.melee = false;
    // The Pilot's body plays the attacker sequence from the victim's reference.
    if let Ok(mut a) = actors.get_mut(body.actor) {
        if let Some((c, pos, yaw, d)) = place(&a, run.attacker, t, run.origin, run.yaw) {
            a.autoplay = false;
            a.aim = None;
            a.layers = vec![Layer { clip: c, cycle: (t / d.max(1e-3)).min(0.999), weight: 1.0 }];
            let k = (t / 0.4).min(1.0); // the attacker sequence's blendTime
            let pos = run.start.lerp(pos, k);
            pilot.state.pos = SVec3::new(pos.x, pos.y, pos.z);
            pilot.state.vel = SVec3::ZERO;
            if let Ok(mut atf) = anchors.get_mut(body.anchor) {
                atf.translation = pos;
                atf.rotation = Quat::from_rotation_z(yaw);
            }
        }
    }
    let done = t >= run.duration;
    if let Ok((_, mut e, mut etf)) = enemies.get_mut(run.victim) {
        drive_victim(&mut actors, &mut e, &mut etf, run.victim_seq, t, run.origin, run.yaw);
        if done {
            e.held = false;
            e.executed = true;
            e.damage_unblockable(1e9, false);
        }
    }
    if done {
        exec.0 = None;
    }
}

/// During an execution the camera rides the attacker's animated camera bone, looking at the
/// victim (the scripts' thirdPersonCameraAttachments).
#[allow(clippy::type_complexity)]
pub fn execution_camera(
    titan_exec: Res<TitanExecution>,
    pilot_exec: Res<PilotExecution>,
    body: Option<Res<crate::pilotbody::PilotBody>>,
    titans: Query<&PlayerTitan>,
    enemies: Query<&Enemy>,
    actors: Query<(&Actor, &GlobalTransform)>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
    mut vis: Query<&mut Visibility>,
    mut cams: Query<&mut Transform, With<MainCamera>>,
    mut hud_hidden: ResMut<crate::hud::HudHidden>,
) {
    let executing = titan_exec.0.is_some() || pilot_exec.0.is_some();
    if hud_hidden.0 != executing {
        hud_hidden.0 = executing;
    }
    let (actor_entity, run, height) = if let Some(r) = titan_exec.0.as_ref() {
        let Ok(t) = titans.single() else { return };
        (t.actor, r, 150.0)
    } else if let (Some(r), Some(b)) = (pilot_exec.0.as_ref(), body.as_ref()) {
        // Hide the first-person guns while the body plays the execution.
        for lo in &loadouts {
            for g in &lo.guns {
                if let Some((anchor, _)) = g.viewmodel {
                    if let Ok(mut v) = vis.get_mut(anchor) {
                        *v = Visibility::Hidden;
                    }
                }
            }
        }
        if let Ok(mut v) = vis.get_mut(b.anchor) {
            *v = Visibility::Inherited;
        }
        (b.actor, r, 50.0)
    } else {
        return;
    };
    let Ok((actor, gt)) = actors.get(actor_entity) else { return };
    let Ok(e) = enemies.get(run.victim) else { return };
    let Some(bone) = actor.bone_model_transform("jx_c_camera") else {
        log::warn!("execution camera: no jx_c_camera bone on the attacker");
        return;
    };
    let eye = gt.transform_point(bone.translation);
    // The camera attachment's frame inside its bone: forward = bone Z, up = bone Y (the
    // `CAMERA` attachment's local matrix on `jx_c_camera`, as the rodeo arms confirmed).
    let rot = gt.rotation() * bone.rotation;
    let (fwd, up) = (rot * Vec3::Z, rot * Vec3::Y);
    let target = to_bevy(e.pos + Vec3::Z * height);
    log::debug!("exec cam t={:.2}: bone {:.0} eye(bevy) {:.2} fwd {:.2} up {:.2} victim(bevy) {:.2}", run.t, bone.translation, eye, fwd, up, target);
    if let Ok(mut cam) = cams.single_mut() {
        // The view is the bone's own; TF_EXEC_LOOKAT=1 looks at the victim from it instead.
        if std::env::var_os("TF_EXEC_LOOKAT").is_some() || fwd.length_squared() < 0.5 {
            if (target - eye).length_squared() > 1e-4 {
                *cam = Transform::from_translation(eye).looking_at(target, Vec3::Y);
            }
        } else {
            *cam = Transform::from_translation(eye).looking_to(fwd, up);
        }
    }
}
