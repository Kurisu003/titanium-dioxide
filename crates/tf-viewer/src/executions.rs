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

/// A Titan execution from `datatable/titan_executions.rpak` (TitanVsTitan_3p): the attacker's
/// sequence against a Titan with its Pilot inside and against an empty one, the victim's by its
/// chassis (light, medium, heavy: victimAnim_lt/_md/_hv), the sequence of the Pilot pulled out
/// of a player's Titan (victimAnim_pt_*, none for BT's), and the sounds (sound_1p for the
/// attacker, sound_3p for everyone else).
pub struct TitanExecutionDef {
    pub attacker: &'static str,
    pub attacker_vs_auto: &'static str,
    pub victim: [&'static str; 3],
    pub victim_pilot: &'static str,
    pub sound_1p: &'static str,
    pub sound_3p: &'static str,
}

/// Tone's execution (titan_atlas_tracker, the players' chassis), then BT's three (titan_buddy;
/// the campaign picks one per level, `_sp_loadouts.nut` titanExecution).
pub const TITAN_EXECUTIONS: [TitanExecutionDef; 4] = [
    TitanExecutionDef {
        attacker: "mt_execution_attacker_tone",
        attacker_vs_auto: "mt_execution_attacker_tone_auto",
        victim: [
            "lt_execution_victim_tone",
            "mt_execution_victim_tone",
            "ht_execution_victim_tone",
        ],
        victim_pilot: "pt_execution_victim_tone",
        sound_1p: "Atlas_1p_Sync_Melee",
        sound_3p: "Atlas_3p_Sync_Melee",
    },
    TitanExecutionDef {
        attacker: "bt_synced_titan_execute_flip_takedown_A",
        attacker_vs_auto: "bt_synced_titan_execute_flip_takedown_A",
        victim: ["titan_synced_bt_execute_flip_takedown_V"; 3],
        victim_pilot: "",
        sound_1p: "Atlas_1p_Sync_Melee",
        sound_3p: "Atlas_3p_Sync_Melee",
    },
    TitanExecutionDef {
        attacker: "bt_synced_titan_execute_kickshoot_A",
        attacker_vs_auto: "bt_synced_titan_execute_kickshoot_A",
        victim: ["titan_synced_bt_execute_kickshoot_V"; 3],
        victim_pilot: "",
        sound_1p: "Atlas_1p_Sync_Melee",
        sound_3p: "Atlas_3p_Sync_Melee",
    },
    TitanExecutionDef {
        attacker: "bt_synced_titan_execute_pilot_rip_A",
        attacker_vs_auto: "bt_synced_titan_execute_pilot_rip_A",
        victim: ["titan_synced_bt_execute_pilot_rip_V"; 3],
        victim_pilot: "",
        sound_1p: "Atlas_1p_Sync_Melee",
        sound_3p: "Atlas_3p_Sync_Melee",
    },
];

/// The victim Pilot sequences, for the Pilot body.
pub const TITAN_EXEC_PILOT_SEQUENCES: &[&str] = &["pt_execution_victim_tone"];

/// Where in `pt_execution_victim_tone` its "pink_mist" event fires (MeleePinkMistFakeBody):
/// the Pilot's body dissolves in a pink mist.
const PINK_MIST_CYCLE: f32 = 0.815;

/// The player's execution: Tone's on Tone's chassis, BT's campaign one on the buddy model.
pub fn player_execution(map: &str) -> usize {
    if crate::PLAYER_TITAN_MODEL.contains("buddy") {
        1 + bt_execution_for_map(map)
    } else {
        0
    }
}

/// The datatable's victim column (light, medium, heavy) for a Titan model.
fn chassis_index(model: &str) -> usize {
    match crate::net::chassis_name(model) {
        "stryder" => 0,
        "ogre" => 2,
        _ => 1,
    }
}

/// Pilot executions on humans that need no extra props: (attacker, victim).
pub const PILOT_EXECUTIONS: [(&str, &str); 2] = [
    ("pt_mp_execution_attacker_nb", "pt_mp_execution_victim_nb"),
    (
        "pt_mp_execution_attacker_kick",
        "pt_mp_execution_victim_kick",
    ),
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

/// The victim sequences a Titan model plays, for the enemy actors' sequence lists.
pub fn titan_victim_sequences(model: &str) -> Vec<&'static str> {
    let c = chassis_index(model);
    let mut v: Vec<&str> = TITAN_EXECUTIONS.iter().map(|e| e.victim[c]).collect();
    v.dedup();
    v
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
        let s = tf_assets::settings::PlayerSettings::load(
            "scripts/weapons/melee_pilot_emptyhanded.txt",
            crate::gamedata::sp(),
            read,
        )
        .unwrap_or_default();
        let f = |k: &str, d: f32| {
            s.get(&format!("{}{k}", crate::gamedata::base()))
                .or_else(|| s.get(&format!(".{k}")))
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
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
    /// The shared reference: the victim's position and the facing both sequences start from
    /// (for a Titan execution, the victim facing the attacker).
    origin: Vec3,
    yaw: f32,
    attacker: &'static str,
    victim_seq: &'static str,
    /// The sequence of the Pilot pulled out of the victim Titan (played on the Pilot body), and
    /// whether their body is gone in a pink mist.
    pilot_seq: Option<&'static str>,
    mist: bool,
    duration: f32,
    /// Where the attacker was, for the blend in.
    start: Vec3,
    start_yaw: f32,
    prev_mode: CameraMode,
    /// BT's health when it started (the attacker is invulnerable).
    health: Option<crate::vitals::Vitals>,
}

#[derive(Resource, Default)]
pub struct TitanExecution(pub Option<Running>);

#[derive(Resource, Default)]
pub struct PilotExecution(pub Option<Running>);

/// Another player is executing our Titan (net.rs EV_EXECUTION).
pub struct Executed {
    t: f32,
    /// The shared reference: our spot and the facing (toward the attacker).
    origin: Vec3,
    yaw: f32,
    seq: &'static str,
    pilot_seq: Option<&'static str>,
    mist: bool,
    sound: &'static str,
    /// The view to go back to.
    prev: CameraMode,
}

#[derive(Resource, Default)]
pub struct BeingExecuted(pub Option<Executed>);

/// While set, the Titan's death kills its Pilot instead of ejecting them (pilotctl.rs).
pub static BEING_EXECUTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// While set, our Titan is executing another: no control until it is over (game.rs gate_input).
pub static EXECUTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// While set, a Titan execution is playing the victim Pilot's sequence on the Pilot body
/// (pilotbody.rs leaves it alone).
pub static BODY_IN_EXECUTION: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl BeingExecuted {
    /// `idx` is the attacker's TITAN_EXECUTIONS entry; our half is its victim sequence for our
    /// chassis, and our Pilot is pulled out.
    pub fn start(&mut self, origin: Vec3, yaw: f32, idx: usize) {
        let def = &TITAN_EXECUTIONS[idx.min(TITAN_EXECUTIONS.len() - 1)];
        self.0 = Some(Executed {
            t: 0.0,
            origin,
            yaw,
            seq: def.victim[chassis_index(crate::PLAYER_TITAN_MODEL)],
            pilot_seq: (!def.victim_pilot.is_empty()).then_some(def.victim_pilot),
            mist: false,
            sound: def.sound_3p,
            prev: CameraMode::Cockpit,
        });
        BEING_EXECUTED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

/// A root-motion sequence's frame at `t` seconds, played from `start` (game space).
fn moved(actor: &Actor, clip: usize, t: f32, start: Transform) -> Transform {
    let m = actor.clips[clip].movement_at(t).unwrap_or([0.0; 4]);
    start
        * Transform::from_xyz(m[0], m[1], m[2])
            .with_rotation(Quat::from_rotation_z(m[3].to_radians()))
}

/// The cycle of `clip` at `t` seconds.
fn cycle_at(actor: &Actor, clip: usize, t: f32) -> f32 {
    (t / actor.clips[clip].duration.max(1e-3)).min(0.999)
}

/// The reference the other halves play from: the victim's `ref` attachment (on its jx_c_start
/// bone) as its sequence moves it (FirstPersonSequence's useAnimatedRefAttachment).
fn animated_ref(actor: &Actor, clip: usize, t: f32, start: Transform) -> Transform {
    let root = moved(actor, clip, t, start);
    match actor.clip_bone_model_transform(clip, cycle_at(actor, clip, t), "jx_c_start") {
        Some(b) => root * b,
        None => root,
    }
}

/// Where an actor playing `clip` stands at `t` so that its own jx_c_start meets `reference`.
fn root_on_ref(actor: &Actor, clip: usize, t: f32, reference: Transform) -> (Vec3, f32) {
    let root = match actor.clip_bone_model_transform(clip, cycle_at(actor, clip, t), "jx_c_start") {
        Some(b) => reference.compute_affine() * b.compute_affine().inverse(),
        None => reference.compute_affine(),
    };
    let (_, rot, pos) = root.to_scale_rotation_translation();
    let fwd = rot * Vec3::X;
    (pos, fwd.y.atan2(fwd.x))
}

/// Play one layer of `clip` at `t` on an actor, nothing else.
fn play(actor: &mut Actor, clip: usize, t: f32) {
    let cycle = cycle_at(actor, clip, t);
    actor.autoplay = false;
    actor.aim = None;
    actor.additive.clear();
    actor.overlays.clear();
    actor.layers = vec![Layer {
        clip,
        cycle,
        weight: 1.0,
    }];
}

/// The victim Pilot's half (TitanVsTitan_3p's targetViewBody): the Pilot body plays its
/// sequence on the victim Titan's animated reference, until the pink mist takes it.
fn drive_victim_pilot(
    commands: &mut Commands,
    actors: &mut Query<&mut Actor>,
    anchors: &mut Query<(&mut Transform, &mut Visibility), (Without<PlayerTitan>, Without<Enemy>)>,
    body: Option<&crate::pilotbody::PilotBody>,
    seq: Option<&'static str>,
    mist: &mut bool,
    t: f32,
    reference: Transform,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let (Some(body), Some(seq)) = (body, seq) else {
        return;
    };
    let Ok(mut a) = actors.get_mut(body.actor) else {
        return;
    };
    let Some(c) = a.clip(seq) else { return };
    BODY_IN_EXECUTION.store(true, Relaxed);
    let (pos, yaw) = root_on_ref(&a, c, t, reference);
    play(&mut a, c, t);
    let gone = *mist || cycle_at(&a, c, t) >= PINK_MIST_CYCLE;
    if let Ok((mut tf, mut vis)) = anchors.get_mut(body.anchor) {
        tf.translation = pos;
        tf.rotation = Quat::from_rotation_z(yaw);
        vis.set_if_neq(if gone {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    if gone && !*mist {
        *mist = true;
        // ENTITY_DISSOLVE_PINKMIST: the body bursts.
        let at = pos + Vec3::Z * 40.0;
        crate::pfx::emit_named(commands, "death_pinkmist", to_bevy(at), Vec3::Y);
        crate::audio::event_at(commands, "death_pinkmist", at);
    }
}

/// The Pilot body back to its own systems.
fn release_body(
    anchors: &mut Query<(&mut Transform, &mut Visibility), (Without<PlayerTitan>, Without<Enemy>)>,
    body: Option<&crate::pilotbody::PilotBody>,
) {
    if !BODY_IN_EXECUTION.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Some((_, mut vis)) = body.and_then(|b| anchors.get_mut(b.anchor).ok()) {
        vis.set_if_neq(Visibility::Hidden);
    }
}

/// Our Titan's half of another player's execution: it rides the victim sequence from the
/// shared reference with no control (no moving, firing, disembarking or ejecting: the
/// execution's victim can't get out), seen from its own animated camera, while our Pilot is
/// pulled out and crushed, until the attacker's last blow destroys it, which kills the Pilot
/// inside rather than ejecting them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn executed_victim(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<BeingExecuted>,
    mut mode: ResMut<CameraMode>,
    mut input: ResMut<PlayerInput>,
    body: Option<Res<crate::pilotbody::PilotBody>>,
    mut titans: Query<(&mut PlayerTitan, &mut Transform, &TitanHealth), Without<Enemy>>,
    mut anchors: Query<(&mut Transform, &mut Visibility), (Without<PlayerTitan>, Without<Enemy>)>,
    mut actors: Query<&mut Actor>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(r) = run.0.as_mut() else {
        return;
    };
    let Ok((mut titan, mut tf, health)) = titans.single_mut() else {
        return;
    };
    if r.t == 0.0 {
        r.prev = *mode;
        *mode = CameraMode::Third;
        // EmitDifferentSoundsOnEntityForPlayerAndWorld: the victim hears the world's.
        crate::audio::event_at(&mut commands, r.sound, r.origin);
    }
    r.t += time.delta_secs().min(0.1);
    let t = r.t;
    let (yaw_in, pitch_in) = (input.yaw, input.pitch);
    *input = PlayerInput {
        yaw: yaw_in,
        pitch: pitch_in,
        ..default()
    };
    let start = Transform::from_translation(r.origin).with_rotation(Quat::from_rotation_z(r.yaw));
    let mut duration = 5.0;
    let mut reference = None;
    if let Ok(mut a) = actors.get_mut(titan.actor) {
        if let Some(c) = a.clip(r.seq) {
            duration = a.clips[c].duration;
            let root = moved(&a, c, t, start);
            reference = Some(animated_ref(&a, c, t, start));
            play(&mut a, c, t);
            let fwd = root.rotation * Vec3::X;
            let yaw = fwd.y.atan2(fwd.x);
            titan.state.pos =
                SVec3::new(root.translation.x, root.translation.y, root.translation.z);
            titan.state.vel = SVec3::ZERO;
            titan.state.yaw = yaw;
            tf.translation = root.translation;
            tf.rotation = Quat::from_rotation_z(yaw);
        }
    }
    if let Some(reference) = reference {
        drive_victim_pilot(
            &mut commands,
            &mut actors,
            &mut anchors,
            body.as_deref(),
            r.pilot_seq,
            &mut r.mist,
            t,
            reference,
        );
    }
    titan.override_anim = Some((r.seq.to_string(), t.min(duration)));
    // Over once the Titan is gone (the attacker's blow), or a little after the sequence if
    // that never comes.
    if health.dead_for.is_some() || t > duration + 3.0 {
        titan.override_anim = None;
        *mode = r.prev;
        run.0 = None;
        BEING_EXECUTED.store(false, Relaxed);
        release_body(&mut anchors, body.as_deref());
    }
}

impl PilotExecution {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

/// Root motion of `clip` at `t`, placed at the shared reference: (position, yaw).
fn place(
    actor: &Actor,
    clip: &str,
    t: f32,
    origin: Vec3,
    yaw: f32,
) -> Option<(usize, Vec3, f32, f32)> {
    let c = actor.clip(clip)?;
    let cl = &actor.clips[c];
    let m = cl.movement_at(t).unwrap_or([0.0; 4]);
    let pos = origin + Quat::from_rotation_z(yaw) * Vec3::new(m[0], m[1], m[2]);
    Some((c, pos, yaw + m[3].to_radians(), cl.duration))
}

/// Drive one enemy through its victim sequence; returns its animated reference.
fn drive_victim(
    actors: &mut Query<&mut Actor>,
    e: &mut Enemy,
    etf: &mut Transform,
    seq: &str,
    t: f32,
    origin: Vec3,
    yaw: f32,
) -> Option<Transform> {
    let Ok(mut a) = actors.get_mut(e.actor) else {
        return None;
    };
    let c = a.clip(seq)?;
    let start = Transform::from_translation(origin).with_rotation(Quat::from_rotation_z(yaw));
    let root = moved(&a, c, t, start);
    let reference = animated_ref(&a, c, t, start);
    play(&mut a, c, t);
    let fwd = root.rotation * Vec3::X;
    let y = fwd.y.atan2(fwd.x);
    let pos = root.translation;
    e.pos = pos;
    e.state.pos = SVec3::new(pos.x, pos.y, pos.z);
    e.state.vel = SVec3::ZERO;
    e.state.yaw = y;
    etf.translation = pos;
    etf.rotation = Quat::from_rotation_z(y);
    Some(reference)
}

/// Shortest-way blend between two angles.
fn lerp_angle(a: f32, b: f32, k: f32) -> f32 {
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * k
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn titan_execution(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    args: Res<crate::Args>,
    kits: Res<crate::targets::EnemyKits>,
    body: Option<Res<crate::pilotbody::PilotBody>>,
    mut mode: ResMut<CameraMode>,
    mut input: ResMut<PlayerInput>,
    mut exec: ResMut<TitanExecution>,
    mut titans: Query<(&mut PlayerTitan, &mut Transform, &mut TitanHealth), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &mut Transform), Without<PlayerTitan>>,
    mut anchors: Query<(&mut Transform, &mut Visibility), (Without<PlayerTitan>, Without<Enemy>)>,
    mut actors: Query<&mut Actor>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let dt = time.delta_secs().min(0.1);
    let Ok((mut titan, mut tf, mut health)) = titans.single_mut() else {
        return;
    };
    // TF_EXEC_TEST: debug toggle that dooms the first landed enemy Titan and stands it in front
    // of BT, so a scripted melee executes it.
    if let Some(want) = std::env::var("TF_EXEC_TEST")
        .ok()
        .filter(|_| exec.0.is_none() && *control == Control::Titan)
    {
        let bt = Vec3::from(titan.state.pos.to_array());
        // Along the view, so the crosshair is on it in the cockpit.
        let fwd = Vec3::new(input.yaw.cos(), input.yaw.sin(), 0.0);
        // TF_EXEC_TEST=<class> (ion, scorch, ...) picks that class; anything else, the first.
        let want = want.to_ascii_lowercase();
        if let Some((_, mut e, mut etf)) = enemies.iter_mut().find(|(_, e, _)| {
            e.alive()
                && !e.infantry
                && !e.arriving()
                && !e.held
                && e.sound_type.is_none_or(|t| want == "1" || t == want)
        }) {
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
        let can = *control == Control::Titan
            && input.melee
            && health.dead_for.is_none()
            && titan.override_anim.is_none()
            && *mode != CameraMode::Free;
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
                let ang = to
                    .normalize_or_zero()
                    .dot(fwd)
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees();
                (dist < TITAN_EXECUTION_RANGE && ang < TITAN_EXECUTION_ANGLE).then_some((id, dist))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let Some(victim) = victim else { return };
        // The melee press starts the execution instead of a punch.
        input.melee = false;
        let (_, mut e, _) = enemies.get_mut(victim).unwrap();
        let idx = player_execution(&args.map);
        let def = &TITAN_EXECUTIONS[idx];
        let model = kits.0.get(e.kit).map(|k| k.model.as_str()).unwrap_or("");
        let v = def.victim[chassis_index(model)];
        // A player's Titan with its Pilot inside: they are pulled out (targetViewBody), and the
        // attacker plays the version with them; an empty Titan gets attackerAnimVsAutoTitan.
        let pilot_seq = (e.piloted && !def.victim_pilot.is_empty()).then_some(def.victim_pilot);
        let a = if pilot_seq.is_some() {
            def.attacker
        } else {
            def.attacker_vs_auto
        };
        // GetRefAnglesBetweenEnts: the victim turns to face the attacker.
        let to = (bt - e.pos).truncate();
        let yaw = to.y.atan2(to.x);
        e.held = true;
        let duration = actors
            .get(titan.actor)
            .ok()
            .and_then(|ac| ac.clip(a).map(|c| ac.clips[c].duration))
            .unwrap_or(5.0);
        log::info!(
            "titan execution: {a} / {v}{} ({duration:.2}s); BT at {bt:.0}, victim at {:.0}",
            if pilot_seq.is_some() {
                " with its Pilot"
            } else {
                ""
            },
            e.pos
        );
        // EmitDifferentSoundsOnEntityForPlayerAndWorld( sound_1p, sound_3p, attacker ).
        crate::audio::event(&mut commands, def.sound_1p);
        // Another player's Titan: they play their half (executed_victim).
        if let Some(pid) = e.remote {
            crate::net::event(
                crate::net::EV_EXECUTION,
                e.pos,
                Vec3::new(pid as f32, idx as f32, yaw),
            );
        }
        exec.0 = Some(Running {
            victim,
            t: 0.0,
            origin: e.pos,
            yaw,
            attacker: a,
            victim_seq: v,
            pilot_seq,
            mist: false,
            duration,
            start: bt,
            start_yaw: titan.state.yaw,
            prev_mode: *mode,
            health: Some(health.v.clone()),
        });
        *mode = CameraMode::Third;
        EXECUTING.store(true, Relaxed);
    }
    let Some(run) = exec.0.as_mut() else { return };
    run.t += dt;
    let t = run.t;
    input.melee = false;
    let done = t >= run.duration;
    // The victim plays its sequence from the shared reference; its animated reference carries
    // the attacker and the Pilot pulled out of it.
    let reference = match enemies.get_mut(run.victim) {
        Ok((_, mut e, mut etf)) => {
            let r = drive_victim(
                &mut actors,
                &mut e,
                &mut etf,
                run.victim_seq,
                t,
                run.origin,
                run.yaw,
            );
            if done {
                e.held = false;
                e.executed = true;
                e.damage_bypass(1e9);
            }
            r
        }
        Err(_) => None,
    }
    .unwrap_or_else(|| {
        Transform::from_translation(run.origin).with_rotation(Quat::from_rotation_z(run.yaw))
    });
    // BT stands where his own jx_c_start meets that reference, blending in from where he was.
    if let Ok(mut a) = actors.get_mut(titan.actor) {
        if let Some(c) = a.clip(run.attacker) {
            let (pos, yaw) = root_on_ref(&a, c, t, reference);
            play(&mut a, c, t);
            let k = (t / BLEND_TIME).min(1.0);
            let pos = run.start.lerp(pos, k);
            let yaw = lerp_angle(run.start_yaw, yaw, k);
            titan.state.pos = SVec3::new(pos.x, pos.y, pos.z);
            titan.state.vel = SVec3::ZERO;
            titan.state.yaw = yaw;
            input.yaw = yaw;
            input.pitch = 0.0;
            tf.translation = pos;
            tf.rotation = Quat::from_rotation_z(yaw);
        }
    }
    drive_victim_pilot(
        &mut commands,
        &mut actors,
        &mut anchors,
        body.as_deref(),
        run.pilot_seq,
        &mut run.mist,
        t,
        reference,
    );
    titan.override_anim = Some((run.attacker.to_string(), t));
    if let Some(v) = &run.health {
        health.v = v.clone();
    }
    if done {
        titan.override_anim = None;
        *mode = run.prev_mode;
        exec.0 = None;
        EXECUTING.store(false, Relaxed);
        release_body(&mut anchors, body.as_deref());
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
    (mut melee, mut punch, springs): (
        ResMut<PilotMelee>,
        ResMut<crate::vmmotion::ViewPunch>,
        Res<crate::vmmotion::WeaponSprings>,
    ),
) {
    let dt = time.delta_secs().min(0.1);
    let Ok(mut pilot) = pilots.single_mut() else {
        return;
    };
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
            let target = melee
                .target
                .filter(|&id| {
                    enemies
                        .get(id)
                        .is_ok_and(|(_, e, _)| e.alive() && reach(&e))
                })
                .or_else(|| {
                    enemies
                        .iter()
                        .filter(|(_, e, _)| {
                            e.alive()
                                && e.infantry
                                && reach(e)
                                && (e.pos - me).truncate().normalize_or_zero().dot(look) > 0.7
                        })
                        .map(|(id, ..)| id)
                        .next()
                });
            if let Some((_, mut e, _)) = target.and_then(|id| enemies.get_mut(id).ok()) {
                e.damage_unblockable(d.damage, false);
                melee.hit = true;
                crate::audio::event(&mut commands, "Pilot_Mvmt_Melee_Hit_1P");
                log::info!("pilot melee hit for {}", d.damage);
            }
            let r = [
                rand01(&mut *pick),
                rand01(&mut *pick),
                rand01(&mut *pick),
                rand01(&mut *pick),
            ];
            punch.kick(&d.kick, &springs, 0.0, false, 1.0, r);
            melee.raise = if melee.hit {
                d.raise_quick
            } else {
                d.raise_normal
            };
        }
        if t >= MELEE_ANIM_SECS + melee.raise {
            melee.t = None;
        }
        input.melee = false;
    }
    // TF_EXEC_TEST: stand the nearest grunt in front of the Pilot with its back turned.
    if std::env::var_os("TF_EXEC_TEST").is_some() && exec.0.is_none() && *control == Control::Pilot
    {
        let me = Vec3::from(pilot.state.pos.to_array());
        let fwd = Vec3::new(pilot.state.yaw.cos(), pilot.state.yaw.sin(), 0.0);
        if let Some((_, mut e, mut etf)) = enemies
            .iter_mut()
            .find(|(_, e, _)| e.alive() && e.infantry && !e.held)
        {
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
                let ang = to
                    .normalize_or_zero()
                    .dot(look)
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees();
                let their_fwd = Vec2::new(e.state.yaw.cos(), e.state.yaw.sin());
                // The attacker must be at the victim's local -X.
                let behind = (-to).normalize_or_zero().dot(-their_fwd);
                (dist < HUMAN_EXECUTION_RANGE + e.radius && ang < HUMAN_EXECUTION_ANGLE)
                    .then_some((id, dist, behind))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match target {
            Some((id, _, behind)) if behind >= HUMAN_EXECUTION_MIN_DOT => {
                let (_, mut e, _) = enemies.get_mut(id).unwrap();
                *pick = (*pick + 1) % PILOT_EXECUTIONS.len();
                let (a, v) = PILOT_EXECUTIONS[*pick];
                e.held = true;
                let duration = actors
                    .get(e.actor)
                    .ok()
                    .and_then(|ac| ac.clip(v).map(|c| ac.clips[c].duration))
                    .unwrap_or(3.0);
                log::info!("pilot execution: {a} / {v} ({duration:.2}s)");
                exec.0 = Some(Running {
                    victim: id,
                    t: 0.0,
                    origin: e.pos,
                    yaw: e.state.yaw,
                    attacker: a,
                    victim_seq: v,
                    pilot_seq: None,
                    mist: false,
                    duration,
                    start: me,
                    start_yaw: pilot.state.yaw,
                    prev_mode: CameraMode::Third,
                    health: None,
                });
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
                    .filter(|(_, d, p)| {
                        *d < def.lunge_range.max(def.range)
                            && (*p - me).truncate().normalize_or_zero().dot(look) > cos
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                *pick += 1;
                *melee = PilotMelee {
                    t: Some(0.0),
                    seq: *pick,
                    target: near.map(|n| n.0),
                    raise: def.raise_normal,
                    ..Default::default()
                };
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
            a.layers = vec![Layer {
                clip: c,
                cycle: (t / d.max(1e-3)).min(0.999),
                weight: 1.0,
            }];
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
        drive_victim(
            &mut actors,
            &mut e,
            &mut etf,
            run.victim_seq,
            t,
            run.origin,
            run.yaw,
        );
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

/// During an execution the camera rides the animated camera bone (jx_c_camera, the
/// `vehicle_driver_eyes` attachment first in the datatable's camAttach) of the attacker, or
/// of the victim's own Titan for the victim (thirdPersonCameraEntity = target).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn execution_camera(
    titan_exec: Res<TitanExecution>,
    pilot_exec: Res<PilotExecution>,
    executed: Res<BeingExecuted>,
    body: Option<Res<crate::pilotbody::PilotBody>>,
    titans: Query<&PlayerTitan>,
    enemies: Query<&Enemy>,
    actors: Query<(&Actor, &GlobalTransform)>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
    mut vis: Query<&mut Visibility>,
    mut cams: Query<&mut Transform, With<MainCamera>>,
    mut hud_hidden: ResMut<crate::hud::HudHidden>,
) {
    let executing = titan_exec.0.is_some() || pilot_exec.0.is_some() || executed.active();
    if hud_hidden.0 != executing {
        hud_hidden.0 = executing;
    }
    let (actor_entity, target, t) = if let Some(r) = titan_exec.0.as_ref() {
        let Ok(t) = titans.single() else { return };
        let Ok(e) = enemies.get(r.victim) else { return };
        (t.actor, e.pos + Vec3::Z * 150.0, r.t)
    } else if let Some(r) = executed.0.as_ref() {
        let Ok(t) = titans.single() else { return };
        (t.actor, r.origin + Vec3::Z * 150.0, r.t)
    } else if let (Some(r), Some(b)) = (pilot_exec.0.as_ref(), body.as_ref()) {
        // Hide the first-person guns while the body plays the execution.
        for lo in &loadouts {
            for g in &lo.guns {
                if let Some((anchor, _)) = g.viewmodel {
                    if let Ok(mut v) = vis.get_mut(anchor) {
                        v.set_if_neq(Visibility::Hidden);
                    }
                }
            }
        }
        if let Ok(mut v) = vis.get_mut(b.anchor) {
            v.set_if_neq(Visibility::Inherited);
        }
        let Ok(e) = enemies.get(r.victim) else { return };
        (b.actor, e.pos + Vec3::Z * 50.0, r.t)
    } else {
        return;
    };
    let Ok((actor, gt)) = actors.get(actor_entity) else {
        return;
    };
    let Some(bone) = actor.bone_model_transform("jx_c_camera") else {
        log::warn!("execution camera: no jx_c_camera bone on the attacker");
        return;
    };
    let eye = gt.transform_point(bone.translation);
    // The camera attachment's frame inside its bone: forward = bone Z, up = bone Y (the
    // `CAMERA` attachment's local matrix on `jx_c_camera`, as the rodeo arms confirmed).
    let rot = gt.rotation() * bone.rotation;
    let (fwd, up) = (rot * Vec3::Z, rot * Vec3::Y);
    let target = to_bevy(target);
    log::debug!(
        "exec cam t={:.2}: bone {:.0} eye(bevy) {:.2} fwd {:.2} up {:.2} victim(bevy) {:.2}",
        t,
        bone.translation,
        eye,
        fwd,
        up,
        target
    );
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
