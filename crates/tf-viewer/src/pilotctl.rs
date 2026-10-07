//! Playing on foot: disembarking from BT, pilot movement (wall-running and all), embarking
//! back in, and calling BT down with a Titanfall.

use crate::actor::Actor;
use crate::player::{to_bevy, CameraMode, Collision, MainCamera, PlayerInput, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use crate::weapons::FxAssets;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::{self, PilotInput, PilotMove, PilotParams, PilotState};

const DISEMBARK_ANIM: &str = "at_dismount_stand";
/// GetDisembarkSequenceForTitan: with crouch held you leave low (no headroom needed).
const DISEMBARK_CROUCH_ANIM: &str = "at_dismount_crouch";
/// After the pilot is out, BT closes the hatch and returns to idle.
const DISEMBARK_RECOVER_ANIM: &str = "at_MP_disembark_back2idle";
/// BT's standing mount, by which side the pilot climbs on from (relative to his facing).
const EMBARK_FRONT: &str = "at_mount_stand_front";
const EMBARK_BEHIND: &str = "at_mount_stand_behind";
const EMBARK_LEFT: &str = "at_mount_stand_left";
const EMBARK_RIGHT: &str = "at_mount_stand_right";
/// The kneeling mounts (BT drops to a knee and lifts the pilot in): used when he isn't in
/// combat, as `ShouldDoRegularEmbark` does for BT in SP ("special for BT if he is in casual
/// mode"); the left/right ones only exist on the kneel set and the Titan's stand set.
const EMBARK_KNEEL_FRONT: &str = "at_mount_kneel_front";
const EMBARK_KNEEL_BEHIND: &str = "at_mount_kneel_behind";
const EMBARK_KNEEL_LEFT: &str = "at_mount_kneel_left";
const EMBARK_KNEEL_RIGHT: &str = "at_mount_kneel_right";
/// BT counts as in combat (regular, standing embark) with a live enemy this close: the
/// XO-16's npc_min_engage_titan, the range at which the NPC would engage it.
const EMBARK_COMBAT_RANGE: f32 = 1500.0;
/// Fallback lengths if BT's sequences are missing (their real lengths: 1.17 s and 2.77 s).
const DISEMBARK_SECS: f32 = 1.17;
const EMBARK_SECS: f32 = 2.77;
/// TitanEjectPlayer (sh_titan.gnut): BT plays the eject sequence for blendDelay (0.15 s) plus
/// TITAN_PLAYEREJECT_DURATION (0.8 s), then the pilot launches at 1500-1700 u/s pitched 5 degrees
/// back from straight up (times sqrt(gravityscale)), looking 80 degrees down at the Titan.
const EJECT_ANIM: &str = "at_MP_eject_stand_start";
const EJECT_SECS: f32 = 0.15 + 0.8;
const EJECT_SPEED: (f32, f32) = (1500.0, 1700.0);
const EMBARK_RANGE: f32 = 260.0;
/// The player's Titanfall (_titan_hotdrop.gnut): BT rides this sequence's root motion down
/// from the sky, then stands with the quickstand.
const DROP_ANIM: &str = "at_hotdrop_drop_2knee_turbo";
const STAND_ANIM: &str = "at_hotdrop_quickstand";
/// damagedef_titan_fall: crushes what is under BT (heavy armor 23000, bypasses shields and
/// the doomed state). damagedef_titan_hotdrop: the impact blast.
const TITANFALL_DAMAGE_HEAVY: f32 = 23000.0;
const TITANFALL_INNER_RADIUS: f32 = 90.0;
const TITANFALL_OUTER_RADIUS: f32 = 120.0;
const HOTDROP_DAMAGE: f32 = 150.0;
const HOTDROP_INNER_RADIUS: f32 = 80.0;
const HOTDROP_RADIUS: f32 = 250.0;

/// Who the player is controlling.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub enum Control {
    Titan,
    Pilot,
    /// Climbing out of BT (seconds elapsed).
    Disembark(f32),
    /// Climbing into BT (seconds elapsed).
    Embark(f32),
}

#[derive(Resource, Clone)]
pub struct PilotSettings(pub PilotParams);

#[derive(Component)]
pub struct PlayerPilot {
    pub state: PilotState,
    /// Camera position/rotation when an embark started (game space), for the blend in.
    embark_from: Option<(Vec3, Quat)>,
    /// BT's mount sequence for the side the pilot climbs on from.
    embark_anim: &'static str,
    /// BT's dismount sequence (standing, or crouched when crouch is held).
    disembark_anim: &'static str,
}

impl PlayerPilot {
    pub fn new() -> Self {
        Self { state: PilotState::new(SVec3::ZERO, 0.0), embark_from: None, embark_anim: EMBARK_FRONT, disembark_anim: DISEMBARK_ANIM }
    }
}

/// The mount sequence for a pilot at `to_pilot` from a Titan facing `yaw` (game space), and
/// its first-person twin on the arms.
fn embark_anim_for(yaw: f32, to_pilot: SVec3, kneel: bool) -> (&'static str, &'static str) {
    let rel = (to_pilot.y.atan2(to_pilot.x) - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    let q = std::f32::consts::FRAC_PI_4;
    let side = if rel.abs() <= q {
        0
    } else if rel.abs() >= 3.0 * q {
        1
    } else if rel > 0.0 {
        2
    } else {
        3
    };
    if kneel {
        match side {
            0 => (EMBARK_KNEEL_FRONT, "ptpov_mount_buddy_kneel_front"),
            1 => (EMBARK_KNEEL_BEHIND, "ptpov_mount_buddy_kneel_behind"),
            2 => (EMBARK_KNEEL_LEFT, "ptpov_mount_buddy_kneel_left"),
            _ => (EMBARK_KNEEL_RIGHT, "ptpov_mount_buddy_kneel_right"),
        }
    } else {
        // The standing left/right climbs use the front first-person clip, like the embark
        // actions' firstPersonStandingAlias.
        match side {
            0 => (EMBARK_FRONT, "ptpov_mount_buddy_stand_front"),
            1 => (EMBARK_BEHIND, "ptpov_mount_buddy_stand_behind"),
            2 => (EMBARK_LEFT, "ptpov_mount_buddy_stand_front"),
            _ => (EMBARK_RIGHT, "ptpov_mount_buddy_stand_front"),
        }
    }
}

/// Put the pilot above BT's cockpit and launch them (TitanEjectPlayer).
fn eject_pilot(commands: &mut Commands, pilot: &mut PlayerPilot, input: &mut PlayerInput, titan: &PlayerTitan, gravity_scale: f32) {
    let yaw = titan.state.yaw;
    let mut s = PilotState::new(titan.state.pos + SVec3::Z * 260.0, yaw);
    // RandomFloatRange(1500, 1700): a cheap hash of where BT stands.
    let roll = (titan.state.pos.x * 12.9898 + titan.state.pos.y * 78.233).sin().abs().fract();
    s.vel = eject_launch(yaw, gravity_scale, roll);
    // player_look_angles.x = 80: looking down at the Titan.
    s.pitch = 80f32.to_radians();
    pilot.state = s;
    input.yaw = yaw;
    input.pitch = 80f32.to_radians();
    crate::audio::event(commands, "player_eject_windrush");
}

/// Launch velocity for an ejecting pilot (TitanEjectPlayer).
fn eject_launch(yaw: f32, gravity_scale: f32, roll: f32) -> SVec3 {
    let speed = (EJECT_SPEED.0 + (EJECT_SPEED.1 - EJECT_SPEED.0) * roll) * gravity_scale.sqrt();
    // Pitch 265 in Source angles: 95 degrees up, i.e. 5 degrees back past vertical.
    let up = 95f32.to_radians();
    SVec3::new(up.cos() * yaw.cos(), up.cos() * yaw.sin(), up.sin()) * speed
}

/// BT's Titanfall: seconds into the drop sequence, then into the stand-up.
#[derive(Component, Default)]
pub struct Titanfall {
    pub drop: Option<f32>,
    /// While dropping: BT's offset from where he lands (game units).
    pub offset: Option<Vec3>,
    impacted: bool,
    pub landing: Option<f32>,
    /// Seconds into a manual eject (BT's eject sequence before the pilot launches).
    pub eject: Option<f32>,
    /// Seconds into BT's post-disembark recovery (hatch closing, back to idle).
    pub recover: Option<f32>,
    /// BT was ejected from and destroys itself.
    self_destruct: bool,
    /// Eject presses so far while doomed (three within TITAN_EJECT_MAX_PRESS_DELAY of each
    /// other eject), and seconds since the last one.
    pub eject_presses: u32,
    pub since_eject_press: f32,
    pub cooldown: f32,
    /// BT was destroyed and a replacement is being prepared.
    pub rebuilding: bool,
}

impl Titanfall {
    pub fn busy(&self) -> bool {
        self.drop.is_some() || self.landing.is_some() || self.eject.is_some()
    }
}

fn playing_now(game: &crate::game::Game) -> bool {
    matches!(game.state, crate::game::GameState::Playing | crate::game::GameState::Intermission)
}

/// Pilot health (pilot_solo.set: 100) with the campaign's rules: PilotHealthRegenThinkSP
/// (_health_regen.gnut) and the per-hit cap from SpPlayer_OnDamaged (_sp_difficulty.gnut).
#[derive(Component)]
pub struct PilotHealth {
    pub health: f32,
    pub since_hit: f32,
    pub hurt: f32,
    /// Recent damage (age in seconds, amount), for the 0.25 s per-hit cap.
    recent: Vec<(f32, f32)>,
}

impl Default for PilotHealth {
    fn default() -> Self {
        Self { health: PILOT_HEALTH, since_hit: 99.0, hurt: 0.0, recent: Vec::new() }
    }
}

pub const PILOT_HEALTH: f32 = 100.0;
/// 4 health per HEALTH_REGEN_TICK_TIME (0.1 s).
const PILOT_REGEN_RATE: f32 = 40.0;
/// TITAN_EJECT_MAX_PRESS_DELAY (_settings.nut): the eject presses must come this close together.
pub const EJECT_MAX_PRESS_DELAY: f32 = 1.0;
/// Presses needed to eject (PlayerPressed_Eject in cl_titan_cockpit.nut).
pub const EJECT_PRESSES: u32 = 3;
/// Seconds after BT is destroyed before a new Titanfall can be called.
pub const TITAN_REBUILD_SECS: f32 = 25.0;

impl PilotHealth {
    /// Damage from an enemy, already scaled for difficulty. `max_per_hit` is the difficulty's
    /// maxDamagePerHit: at most that much (less what was taken in the last 0.25 s, at least 5)
    /// gets through, so a Pilot can't be deleted by a Titan in one burst.
    pub fn damage_capped(&mut self, amount: f32, max_per_hit: f32) {
        let recent: f32 = self.recent.iter().filter(|r| r.0 <= 0.25).map(|r| r.1).sum();
        let allowed = (max_per_hit - recent).max(5.0);
        self.damage(amount.min(allowed));
    }
    pub fn damage(&mut self, amount: f32) {
        if self.health > 0.0 {
            self.health = (self.health - amount).max(0.0);
            self.since_hit = 0.0;
            self.hurt = 0.15;
            self.recent.push((0.0, amount));
            if self.health <= 0.0 {
                log::info!("pilot killed");
            }
        }
    }
    /// Regen starts sooner the healthier the Pilot is: GraphCapped(health, 0, max, 3.0, 0.8).
    pub fn tick(&mut self, dt: f32) {
        self.since_hit += dt;
        self.hurt = (self.hurt - dt).max(0.0);
        for r in &mut self.recent {
            r.0 += dt;
        }
        self.recent.retain(|r| r.0 <= 0.25);
        let delay = crate::vitals::graph_capped(self.health, 0.0, PILOT_HEALTH, 3.0, 0.8);
        if !self.dead() && self.since_hit >= delay {
            self.health = (self.health + PILOT_REGEN_RATE * dt).min(PILOT_HEALTH);
        }
    }
    pub fn dead(&self) -> bool {
        self.health <= 0.0
    }
}

/// Camera shake amount (decays).
#[derive(Resource, Default)]
pub struct Shake(pub f32);

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn view_quat(yaw: f32, pitch: f32) -> Quat {
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch)
}

/// Switch between Titan and pilot, advance transitions, trigger Titanfall.
#[allow(clippy::too_many_arguments)]
pub fn control_transitions(
    mut commands: Commands,
    time: Res<Time>,
    mut control: ResMut<Control>,
    mut input: ResMut<PlayerInput>,
    mut shake: ResMut<Shake>,
    world: Res<Collision>,
    pilot_settings: Res<PilotSettings>,
    _fx: Res<FxAssets>,
    mut titans: Query<(&mut PlayerTitan, &mut Titanfall, &mut crate::combat::TitanHealth)>,
    mut pilots: Query<(&mut PlayerPilot, &mut PilotHealth)>,
    mut dummies: Query<&mut Enemy>,
    actors: Query<&crate::actor::Actor>,
    mut doom_test: Local<(bool, f32)>,
    game: Res<crate::game::Game>,
    (mut rodeo, mut cores): (ResMut<crate::rodeo::Rodeo>, Query<&mut crate::abilities::TitanCore>),
    (mut fpseq, globals, titan_settings, mut auto): (ResMut<crate::fparms::FpSeq>, Query<&GlobalTransform>, Res<TitanSettings>, ResMut<crate::autotitan::AutoTitan>),
) {
    let dt = time.delta_secs();
    let (Ok((mut titan, mut tf, mut bt_health)), Ok((mut pilot, mut pilot_health))) = (titans.single_mut(), pilots.single_mut()) else { return };
    let actor = titan.actor;
    // BT's HIJACK attachment (def_c_spineC, facing his way): the frame the first-person
    // mount/dismount clips play in (FirstPersonSequence attachment "hijack").
    // BT's HIJACK attachment (def_c_spineC; forward = bone Z, left = bone X, up = bone Y)
    // with its full orientation, so the mount/dismount follow his chest as he kneels and leans.
    let hijack_frame = |titan: &PlayerTitan| -> (Vec3, Quat) {
        let spine = actors.get(titan.actor).ok().and_then(|a| a.joint("def_c_spineC")).and_then(|j| globals.get(j).ok());
        let Some(g) = spine else { return (Vec3::from(titan.state.pos.to_array()) + Vec3::Z * 150.0, Quat::from_rotation_z(titan.state.yaw)) };
        let conv = |v: Vec3| Vec3::new(v.x, -v.z, v.y).normalize();
        let r = g.rotation();
        let rot = Quat::from_mat3(&Mat3::from_cols(conv(r * Vec3::Z), conv(r * Vec3::X), conv(r * Vec3::Y))).normalize();
        (from_bevy(g.translation()), rot)
    };
    let clip_of = |name: &str| actors.get(actor).ok().and_then(|a| a.clip(name).map(|c| (a.clips.clone(), c)));
    let secs = |name: &str, fallback: f32| clip_of(name).map(|(clips, c)| clips[c].duration).unwrap_or(fallback);
    let (disembark_secs, embark_secs) = (secs(pilot.disembark_anim, DISEMBARK_SECS), secs(pilot.embark_anim, EMBARK_SECS));
    let gravity_scale = pilot_settings.0.gravity / 750.0;
    // Pilot health regenerates after a short delay.
    pilot_health.tick(dt);
    // TF_HURT_PILOT: 15 damage every 1.5 s on foot, for testing the damage screen effects.
    if playing_now(&game) && *control == Control::Pilot && std::env::var_os("TF_HURT_PILOT").is_some() && pilot_health.health > 30.0 {
        let t = time.elapsed_secs();
        if (t / 1.5).floor() != ((t - dt) / 1.5).floor() {
            pilot_health.damage(15.0);
        }
    }
    // TF_HURT_BT: 1500 damage every 2 s in BT (shield first), for testing the hull damage
    // feedback; stops short of doomed.
    if playing_now(&game) && *control == Control::Titan && std::env::var_os("TF_HURT_BT").is_some() && bt_health.v.health > 3000.0 && bt_health.dead_for.is_none() {
        let t = time.elapsed_secs();
        if (t / 2.0).floor() != ((t - dt) / 2.0).floor() {
            bt_health.damage(1500.0, false);
        }
    }
    // TF_DOOM_BT: doom BT once, for testing the eject; a number delays it to that many seconds
    // of play.
    let playing = matches!(game.state, crate::game::GameState::Playing | crate::game::GameState::Intermission);
    if playing {
        doom_test.1 += dt;
    }
    let doom_at = std::env::var("TF_DOOM_BT").ok().map(|v| v.parse::<f32>().unwrap_or(0.0));
    if !doom_test.0 && playing && *control == Control::Titan && doom_at.is_some_and(|d| doom_test.1 >= d) && bt_health.dead_for.is_none() {
        doom_test.0 = true;
        let (h, sh) = (bt_health.v.health, bt_health.v.shield);
        bt_health.damage(h + sh, false);
    }
    // BT destroyed: eject if we were inside, and start the rebuild timer.
    if bt_health.dead_for.is_some() && !tf.rebuilding {
        tf.rebuilding = true;
        tf.cooldown = TITAN_REBUILD_SECS;
        tf.eject = None;
        tf.self_destruct = false;
        if matches!(*control, Control::Titan | Control::Embark(_) | Control::Disembark(_)) {
            // Auto-eject: no time for the sequence.
            fpseq.stop();
            eject_pilot(&mut commands, &mut pilot, &mut input, &titan, gravity_scale);
            *pilot_health = PilotHealth::default();
            titan.override_anim = None;
            *control = Control::Pilot;
            shake.0 = 0.8;
            log::info!("EJECT");
        }
    }
    // An ejected Titan destroys itself (titanEjectExplosion) once the doomed protection allows.
    if tf.self_destruct && bt_health.dead_for.is_none() {
        bt_health.damage(1.0e9, false);
    }
    let interact = std::mem::take(&mut input.interact);
    let call = std::mem::take(&mut input.titanfall);
    tf.cooldown = (tf.cooldown - dt).max(0.0);
    tf.since_eject_press += dt;
    if tf.since_eject_press > EJECT_MAX_PRESS_DELAY || bt_health.v.doomed.is_none() {
        tf.eject_presses = 0;
    }

    match *control {
        Control::Titan => {
            if tf.eject.is_none() {
                titan.override_anim = None;
            }
            if interact && tf.eject.is_none() {
                if bt_health.v.doomed.is_some() {
                    // Doomed: the use button counts eject presses instead (PlayerPressed_Eject);
                    // the third within TITAN_EJECT_MAX_PRESS_DELAY of the last ejects.
                    tf.eject_presses += 1;
                    tf.since_eject_press = 0.0;
                    crate::audio::event(&mut commands, "titan_eject_xbutton");
                    crate::audio::event(&mut commands, "hud_boost_card_radar_jammer_redtextbeep_1p");
                    log::info!("eject press {}/{EJECT_PRESSES}", tf.eject_presses);
                    if tf.eject_presses >= EJECT_PRESSES {
                        tf.eject = Some(0.0);
                        tf.eject_presses = 0;
                        crate::audio::event_at(&mut commands, "Titan_Eject_Servos_3P", Vec3::from(titan.state.pos.to_array()));
                        log::info!("ejecting");
                        // The cockpit's eject clip, from the eye.
                        let eye = Vec3::from(titan.state.eye(&titan_settings.0).to_array());
                        fpseq.start("atpov_cockpit_eject", (eye, view_quat(titan.state.yaw, 0.0)), None, true);
                    }
                } else {
                    *control = Control::Disembark(0.0);
                    // GetDisembarkSequenceForPlayer/Titan: crouch held = the crouch dismount
                    // (the _fast set needs PAS_FAST_EMBARK, an MP passive).
                    let (anim, clip) = if input.crouch { (DISEMBARK_CROUCH_ANIM, "ptpov_dismount_buddy_crouch") } else { (DISEMBARK_ANIM, "ptpov_dismount_buddy_stand") };
                    pilot.disembark_anim = anim;
                    log::info!("disembarking ({anim}, {clip})");
                    let eye = Vec3::from(titan.state.eye(&titan_settings.0).to_array());
                    fpseq.start(clip, hijack_frame(&titan), Some((eye, view_quat(titan.state.yaw, titan.state.pitch))), false);
                }
            }
        }
        Control::Disembark(t) => {
            let t = t + dt;
            titan.override_anim = Some((pilot.disembark_anim.into(), t));
            fpseq.set_frame(hijack_frame(&titan));
            // The Pilot is let go when the first-person dismount ends (held on its last frame
            // it would ride BT's hijack point back into the closing hatch), or when BT's
            // disembark does if there is no first-person clip.
            let fp_done = fpseq.active.as_ref().is_some_and(|a| a.t >= a.len);
            if t >= disembark_secs || fp_done {
                let last_view = fpseq.cam;
                fpseq.stop();
                // Pop out of the hatch, forward and up, facing the way BT faces.
                let yaw = titan.state.yaw;
                let fwd = SVec3::new(yaw.cos(), yaw.sin(), 0.0);
                let mut s = PilotState::new(titan.state.pos + fwd * 150.0 + SVec3::Z * 180.0, yaw);
                s.vel = fwd * 180.0 + SVec3::Z * 260.0;
                s.pitch = 0.2;
                // From where the dismount left the view, looking the way it looked. The game just
                // unparents the Pilot from BT there (DelayedSafePlayerLocationForDisembark sets no
                // velocity), so they drop from the hatch with BT's own velocity, landing in front
                // of him within embark range.
                if let Some((eye, rot)) = last_view {
                    let f = rot * Vec3::X;
                    let view_yaw = f.y.atan2(f.x);
                    let feet = eye - Vec3::Z * pilot_settings.0.eye_height;
                    s = PilotState::new(SVec3::new(feet.x, feet.y, feet.z), view_yaw);
                    s.vel = titan.state.vel;
                    s.pitch = (-f.z).clamp(-1.0, 1.0).asin();
                }
                // TF_PILOT_TP="x y z yaw_degrees": land at a test spot instead (testing).
                if let Some(v) = std::env::var("TF_PILOT_TP").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 4) {
                    s = PilotState::new(SVec3::new(v[0], v[1], v[2]), v[3].to_radians());
                    s.pitch = 0.0;
                }
                let (yaw, pitch) = (s.yaw, s.pitch);
                pilot.state = s;
                input.yaw = yaw;
                input.pitch = pitch;
                titan.override_anim = None;
                // BT finishes his disembark (negative: time left in it), then closes up.
                tf.recover = Some((t - disembark_secs).min(0.0));
                *control = Control::Pilot;
                log::info!("pilot on foot");
            } else {
                *control = Control::Disembark(t);
            }
        }
        Control::Pilot => {
            let to_bt = titan.state.pos - pilot.state.pos;
            if std::env::var_os("TF_TRACE_POS").is_some() {
                log::info!("trace pilot {:?} vel {:?} yaw {:.2} bt {:?}", pilot.state.pos, pilot.state.vel, pilot.state.yaw, titan.state.pos);
            }
            let near = to_bt.truncate().length() < EMBARK_RANGE && to_bt.z.abs() < 300.0;
            if interact && !(near && !tf.busy() && bt_health.dead_for.is_none()) {
                log::info!("embark refused: BT {:.0} away ({:.0} up), busy {}", to_bt.truncate().length(), to_bt.z, tf.busy());
            }
            if interact && near && !tf.busy() && bt_health.dead_for.is_none() {
                let eye = Vec3::from(pilot.state.eye(&pilot_settings.0).to_array());
                pilot.embark_from = Some((eye, view_quat(pilot.state.yaw, pilot.state.pitch)));
                // BT reaches for the pilot on whichever side they are; out of combat he
                // kneels for them (ShouldDoRegularEmbark: BT's NPC state combat/alert).
                let bt_pos = Vec3::from(titan.state.pos.to_array());
                let nearest = dummies.iter().filter(|e| e.active && e.alive() && !e.arriving()).map(|e| e.pos.distance(bt_pos)).fold(f32::INFINITY, f32::min);
                let combat = nearest < EMBARK_COMBAT_RANGE;
                let (anim, clip) = embark_anim_for(titan.state.yaw, -to_bt, !combat);
                pilot.embark_anim = anim;
                tf.recover = None;
                *control = Control::Embark(0.0);
                log::info!("embarking ({}, {clip}; nearest enemy {nearest:.0})", pilot.embark_anim);
                fpseq.start(clip, hijack_frame(&titan), Some((eye, view_quat(pilot.state.yaw, pilot.state.pitch))), false);
            }
            if call && bt_health.dead_for.is_none() && !tf.busy() {
                // The Titan command: BT guards the spot you're looking at, or follows again.
                let p = &pilot.state;
                let eye = p.eye(&pilot_settings.0);
                let dir = SVec3::new(p.pitch.cos() * p.yaw.cos(), p.pitch.cos() * p.yaw.sin(), -p.pitch.sin());
                let spot = world.0.raycast(eye, dir, 9999.0).map(|h| h.point).unwrap_or(eye + dir * 9999.0);
                auto.toggle(&mut commands, Vec3::from(spot.to_array()));
            } else if call && !tf.busy() && tf.cooldown <= 0.0 {
                // Drop BT at the crosshair (or a little ahead), onto the ground.
                let p = &pilot.state;
                let eye = p.eye(&pilot_settings.0);
                let dir = SVec3::new(p.pitch.cos() * p.yaw.cos(), p.pitch.cos() * p.yaw.sin(), -p.pitch.sin());
                let mut aim = world.0.raycast(eye, dir, 3000.0).map(|h| h.point - dir * 150.0).unwrap_or(eye + dir * 1200.0);
                // Never drop BT on the pilot's head.
                let flat = SVec3::new(aim.x - eye.x, aim.y - eye.y, 0.0);
                if flat.length() < 600.0 {
                    let d = if flat.length() > 1.0 { flat.normalize() } else { SVec3::new(p.yaw.cos(), p.yaw.sin(), 0.0) };
                    aim = SVec3::new(eye.x, eye.y, aim.z) + d * 600.0;
                }
                let ground = world.0.raycast(aim + SVec3::Z * 600.0, -SVec3::Z, 4000.0).map(|h| h.point).unwrap_or(aim);
                titan.state = tf_sim::titan::TitanState::new(ground, p.yaw);
                tf.drop = Some(0.0);
                tf.impacted = false;
                tf.cooldown = 0.0;
                tf.rebuilding = false;
                // A fresh Titan.
                bt_health.reset();
                log::info!("titanfall at {ground:?}");
            }
        }
        Control::Embark(t) => {
            let t = t + dt;
            titan.override_anim = Some((pilot.embark_anim.into(), t));
            fpseq.set_frame(hijack_frame(&titan));
            if t >= embark_secs {
                titan.override_anim = None;
                fpseq.stop();
                input.yaw = titan.state.yaw;
                input.pitch = 0.0;
                pilot.embark_from = None;
                *control = Control::Titan;
                log::info!("in BT");
                if rodeo.battery {
                    // Rodeo_YouEmbarkedWithABattery: ApplyBatteryToTitan.
                    rodeo.battery = false;
                    let v = &mut bt_health.v;
                    v.health = (v.health + crate::rodeo::BT_SEGMENT_HEALTH * crate::rodeo::BATTERY_HEALTH_FRAC).min(v.max_health);
                    v.shield = v.max_shield;
                    if let Ok(mut core) = cores.single_mut() {
                        if core.state == crate::abilities::CoreState::Building {
                            core.meter = (core.meter + crate::rodeo::BATTERY_CORE_FRAC).min(1.0);
                        }
                    }
                    crate::audio::event(&mut commands, "UI_TitanBattery_Titan_PickUp");
                    log::info!("battery applied to BT: health {:.0}, shield {:.0}", v.health, v.shield);
                }
            } else {
                *control = Control::Embark(t);
            }
        }
    }

    // Manual eject: BT's eject sequence, then the pilot launches and BT self-destructs.
    if let Some(t) = tf.eject {
        let t = t + dt;
        titan.override_anim = Some((EJECT_ANIM.into(), t));
        if t >= EJECT_SECS {
            fpseq.stop();
            eject_pilot(&mut commands, &mut pilot, &mut input, &titan, gravity_scale);
            crate::audio::event_at(&mut commands, "Titan_Eject_PilotLaunch_3P", Vec3::from(titan.state.pos.to_array()));
            tf.eject = None;
            tf.self_destruct = true;
            titan.override_anim = None;
            *control = Control::Pilot;
            shake.0 = 0.8;
            log::info!("EJECT");
        } else {
            tf.eject = Some(t);
        }
    }
    // After a disembark BT closes up and settles back to idle (cosmetic: it doesn't block
    // embarking or a new Titanfall).
    if let Some(t) = tf.recover {
        let t = t + dt;
        let duration = clip_of(DISEMBARK_RECOVER_ANIM).map(|(clips, c)| clips[c].duration).unwrap_or(0.0);
        if *control != Control::Pilot || tf.drop.is_some() || t >= duration {
            tf.recover = None;
            if *control == Control::Pilot && tf.drop.is_none() {
                titan.override_anim = None;
            }
        } else {
            titan.override_anim = Some(if t < 0.0 { (pilot.disembark_anim.into(), disembark_secs + t) } else { (DISEMBARK_RECOVER_ANIM.into(), t) });
            tf.recover = Some(t);
        }
    }

    // Titanfall: ride the drop sequence's root motion down, impact, then stand up.
    if let Some(t) = tf.drop {
        let t = t + dt;
        titan.override_anim = Some((DROP_ANIM.into(), t));
        let (offset, duration, impact) = match clip_of(DROP_ANIM) {
            Some((clips, c)) => {
                let clip = &clips[c];
                let total = clip.movement.last().copied().unwrap_or_default();
                let now = clip.movement_at(t).unwrap_or(total);
                // Impact: the first frame that reaches the bottom of the fall.
                let n = clip.movement.len().max(2);
                let hit = clip.movement.iter().position(|m| m[2] <= total[2] + 2.0).unwrap_or(n - 1);
                let rot = Quat::from_rotation_z(titan.state.yaw);
                (rot * Vec3::new(now[0] - total[0], now[1] - total[1], now[2] - total[2]), clip.duration, hit as f32 / (n - 1) as f32 * clip.duration)
            }
            None => (Vec3::ZERO, 0.0, 0.0),
        };
        tf.offset = Some(offset);
        if !tf.impacted && t >= impact {
            tf.impacted = true;
            shake.0 = 1.0;
            crate::audio::cue(&mut commands, crate::audio::Cue::TitanLand, Some(Vec3::from(titan.state.pos.to_array())));
            let at = Vec3::from(titan.state.pos.to_array());
            for mut d in &mut dummies {
                if !d.alive() {
                    continue;
                }
                let dist = ((d.pos - at).truncate().length() - d.radius).max(0.0);
                let fall = |inner: f32, outer: f32| 1.0 - ((dist - inner) / (outer - inner)).clamp(0.0, 1.0);
                if dist < TITANFALL_OUTER_RADIUS {
                    d.damage_bypass(TITANFALL_DAMAGE_HEAVY * fall(TITANFALL_INNER_RADIUS, TITANFALL_OUTER_RADIUS));
                    log::info!("titanfall crushed an enemy titan");
                }
                if dist < HOTDROP_RADIUS {
                    d.damage(HOTDROP_DAMAGE * fall(HOTDROP_INNER_RADIUS, HOTDROP_RADIUS), false);
                }
            }
            let b = to_bevy(at);
            crate::particles::emit(&mut commands, crate::particles::Effect::Explosion { at: b, scale: 0.9 });
            crate::particles::emit(&mut commands, crate::particles::Effect::DustRing { at: b, radius: 220.0 * crate::player::UNIT });
            log::info!("titanfall impact at {t:.2}s");
        }
        if t >= duration {
            tf.drop = None;
            tf.offset = None;
            tf.landing = Some(0.0);
        } else {
            tf.drop = Some(t);
        }
    }
    if let Some(t) = tf.landing {
        let t = t + dt;
        titan.override_anim = Some((STAND_ANIM.into(), t));
        let duration = clip_of(STAND_ANIM).map(|(clips, c)| clips[c].duration).unwrap_or(0.0);
        if t >= duration {
            tf.landing = None;
            titan.override_anim = None;
        } else {
            tf.landing = Some(t);
        }
    }
}

/// Play the sound events (AE_CL_PLAYSOUND) of BT's scripted sequences (embark, disembark,
/// eject, Titanfall) as they pass, at BT's position.
pub fn bt_sequence_sounds(mut commands: Commands, titans: Query<&PlayerTitan>, actors: Query<&Actor>, mut last: Local<Option<(String, f32)>>) {
    let Ok(titan) = titans.single() else { return };
    let Some((name, t)) = titan.override_anim.clone() else {
        *last = None;
        return;
    };
    let Ok(actor) = actors.get(titan.actor) else { return };
    let Some(c) = actor.clip(&name) else { return };
    let clip = &actor.clips[c];
    let cycle = t / clip.duration.max(1e-3);
    let from = match last.as_ref() {
        Some((n, prev)) if *n == name && cycle >= *prev => *prev,
        _ => -1.0,
    };
    let at = Vec3::from(titan.state.pos.to_array()) + Vec3::Z * 150.0;
    for (ev, sound) in &clip.sounds {
        if *ev > from && *ev <= cycle {
            crate::audio::event_at(&mut commands, sound, at);
        }
    }
    *last = Some((name, cycle));
}

pub fn simulate_pilot(
    mut commands: Commands,
    mut stride: Local<f32>,
    time: Res<Time>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    world: Res<Collision>,
    settings: Res<PilotSettings>,
    mut input: ResMut<PlayerInput>,
    mut pilots: Query<&mut PlayerPilot>,
    titans: Query<&PlayerTitan>,
    status: Res<crate::pilotability::PilotStatus>,
    rodeo: Res<crate::rodeo::Rodeo>,
    mut shake: ResMut<Shake>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
) {
    let Ok(mut pilot) = pilots.single_mut() else { return };
    // Stim speeds the Pilot up.
    let mut params = settings.0.clone();
    let k = status.speed_scale();
    params.walk_speed *= k;
    params.sprint_speed *= k;
    params.crouch_speed *= k;
    // Aiming down sights slows the Pilot (the gun's ads_move_speed_scale).
    if let Some(g) = loadouts.single().ok().and_then(|l| l.active()) {
        let a = 1.0 + (g.def.ads_move_scale - 1.0) * g.ads;
        params.walk_speed *= a;
        params.crouch_speed *= a;
    }
    if *control != Control::Pilot {
        input.jump = false;
        return;
    }
    if rodeo.riding() {
        // rodeo.rs places the Pilot on the Titan and handles the jump off.
        return;
    }
    let frozen = *mode == CameraMode::Free;
    let dt = time.delta_secs().min(0.1);
    let pi = PilotInput {
        forward: if frozen { 0.0 } else { input.forward },
        right: if frozen { 0.0 } else { input.right },
        yaw: input.yaw,
        pitch: input.pitch,
        sprint: input.sprint && !frozen,
        crouch: input.crouch && !frozen,
        jump: false,
        ads: input.pilot_ads && !frozen,
    };
    let jump = std::mem::take(&mut input.jump) && !frozen;
    let n = (dt * 120.0).ceil().max(1.0) as usize;
    for i in 0..n {
        let mut step_input = pi;
        step_input.jump = jump && i == 0;
        let before = pilot.state.mode;
        let jumps = pilot.state.jumps;
        let fall = pilot.state.vel.z;
        pilot::step(&mut pilot.state, &step_input, &params, &world.0, dt / n as f32);
        // Movement sounds.
        use crate::audio::{cue, Cue};
        let now = pilot.state.mode;
        if pilot.state.jumps != jumps {
            let double = matches!(before, PilotMove::Air);
            cue(&mut commands, if double { Cue::PilotDoubleJump } else { Cue::PilotJump }, None);
            // The jets' body (sound_jumpjet_jump/jet_body_1p, pilot_base.set) runs on until the
            // jump ends.
            crate::audio::keyed(&mut commands, "jumpjet", if double { "Jumpjet_Jet_Body_1P" } else { "Jumpjet_Jump_Body_1P" });
        }
        if matches!(now, PilotMove::WallRun { .. }) && !matches!(before, PilotMove::WallRun { .. }) {
            crate::audio::keyed(&mut commands, "jumpjet", "Jumpjet_Wallrun_Body_1P");
        }
        if (matches!(before, PilotMove::Air | PilotMove::WallRun { .. }) && matches!(now, PilotMove::Ground | PilotMove::Slide)) || (matches!(before, PilotMove::WallRun { .. }) && !matches!(now, PilotMove::WallRun { .. }) && pilot.state.jumps == jumps) {
            crate::audio::loop_stop(&mut commands, "jumpjet", None);
        }
        match (before, now) {
            (PilotMove::Air, PilotMove::Ground | PilotMove::Slide) if fall < -150.0 => cue(&mut commands, Cue::PilotLand, None),
            (b, PilotMove::WallRun { .. }) if !matches!(b, PilotMove::WallRun { .. }) => cue(&mut commands, Cue::PilotWallrunStart, None),
            (PilotMove::Ground, PilotMove::Slide) => cue(&mut commands, Cue::PilotSlide, None),
            (b, PilotMove::Mantle { .. }) if !matches!(b, PilotMove::Mantle { .. }) => cue(&mut commands, Cue::PilotMantle, None),
            // sound_wallHangStart / sound_wallHangFall (pilot_base.set).
            (b, PilotMove::WallHang { .. }) if !matches!(b, PilotMove::WallHang { .. }) => crate::audio::event(&mut commands, "Default.WallCling_Attach"),
            (PilotMove::WallHang { .. }, n) if !matches!(n, PilotMove::WallHang { .. }) => crate::audio::event(&mut commands, "Default.WallCling_Detach"),
            _ => {}
        }
        // Hard landings (faster than impactSpeed) shake the view.
        if let Some(v) = pilot.state.take_hard_landing(&params) {
            shake.0 = shake.0.max(((v - params.impact_speed) / 600.0 + 0.25).min(0.8));
        }
        if std::mem::discriminant(&pilot.state.mode) != std::mem::discriminant(&before) {
            log::info!(
                "pilot {:?} -> {:?} at {:?} vel {:?}",
                before,
                pilot.state.mode,
                pilot.state.pos,
                pilot.state.vel
            );
        }
    }
    // Footsteps every stride on the ground or the wall.
    let speed = pilot.state.horizontal_speed();
    let stepping = matches!(pilot.state.mode, PilotMove::Ground | PilotMove::WallRun { .. }) && speed > 60.0;
    if stepping {
        *stride += speed * dt;
        let interval = if pilot.state.sprinting || matches!(pilot.state.mode, PilotMove::WallRun { .. }) { 96.0 } else { 72.0 };
        if *stride > interval {
            *stride = 0.0;
            let c = if matches!(pilot.state.mode, PilotMove::WallRun { .. }) { crate::audio::Cue::PilotWallrunStep } else { crate::audio::Cue::PilotStep };
            crate::audio::cue(&mut commands, c, None);
        }
    }
    // Pilots can't walk through BT.
    if let Ok(t) = titans.single() {
        let to = pilot.state.pos - t.state.pos;
        let flat = tf_sim::glam::Vec2::new(to.x, to.y);
        let min = 60.0 + settings.0.radius;
        if flat.length() < min && to.z > -50.0 && to.z < 235.0 {
            let push = flat.normalize_or(tf_sim::glam::Vec2::X) * (min - flat.length());
            pilot.state.pos.x += push.x;
            pilot.state.pos.y += push.y;
        }
    }
}

/// Camera for the pilot and for the embark/disembark transitions.
#[allow(clippy::too_many_arguments)]
pub fn pilot_camera(
    time: Res<Time>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    mut shake: ResMut<Shake>,
    titan_settings: Res<TitanSettings>,
    pilot_settings: Res<PilotSettings>,
    titans: Query<(&PlayerTitan, &crate::combat::TitanHealth)>,
    pilots: Query<&PlayerPilot>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    mut vis: Query<&mut Visibility>,
    punch: Res<crate::weapons::ViewPunch>,
    weapons: Query<&crate::pilotweapon::PilotLoadout>,
    (mut feel, mut vm_eye, world): (Local<CamFeel>, ResMut<VmEye>, Res<Collision>),
) {
    let dt = time.delta_secs();
    shake.0 = (shake.0 - dt * 1.5).max(0.0);
    *vm_eye = VmEye::default();
    if *control == Control::Titan || *mode == CameraMode::Free {
        return;
    }
    let (Ok((titan, bt_health)), Ok(pilot), Ok((mut cam, mut projection))) = (titans.single(), pilots.single(), cameras.single_mut()) else { return };
    // Pilot ADS zoom (R-201 zoom_fov against the pilot's 70 degree hip FOV).
    if let (Some(w), Projection::Perspective(pp)) = (weapons.single().ok().and_then(|l| l.active()), projection.as_mut()) {
        let base = 60f32.to_radians();
        let ratio = (w.def.zoom_fov.to_radians() * 0.5).tan() / (70f32.to_radians() * 0.5).tan();
        let zoomed = 2.0 * ((base * 0.5).tan() * ratio).atan();
        let a = w.ads * w.ads * (3.0 - 2.0 * w.ads);
        pp.fov = base + (zoomed - base) * if *control == Control::Pilot { a } else { 0.0 };
    }
    // BT is visible from outside, unless destroyed.
    if let Ok(mut v) = vis.get_mut(titan.actor) {
        *v = if bt_health.dead_for.is_some_and(|t| t > 1.0) { Visibility::Hidden } else { Visibility::Inherited };
    }
    if let Some((c, _)) = titan.cockpit {
        if let Ok(mut v) = vis.get_mut(c) {
            *v = Visibility::Hidden;
        }
    }
    let s = &pilot.state;
    let pilot_eye = Vec3::from(s.eye(&pilot_settings.0).to_array());
    let bt_eye = Vec3::from(titan.state.eye(&titan_settings.0).to_array());
    let bt_rot = view_quat(titan.state.yaw, 0.0);
    // BT's chest bone, in game space, so the camera rides along as he kneels.
    let chest = actors
        .get(titan.actor)
        .ok()
        .and_then(|a| a.joint("def_c_spineC"))
        .and_then(|j| globals.get(j).ok())
        .map(|g| from_bevy(g.translation()));

    let clip_secs = |name: &str, fallback: f32| actors.get(titan.actor).ok().and_then(|a| a.clip(name).map(|c| a.clips[c].duration)).unwrap_or(fallback);
    let (pos, rot) = match *control {
        Control::Pilot => {
            let dt = time.delta_secs().min(0.05);
            let f = &mut *feel;
            let grounded = s.on_ground();
            // Landing: a dip scaled by impact speed between the falls that viewkickFallDistMin
            // and viewkickFallDistMax (10..70 units) produce.
            if grounded && !f.was_grounded {
                let g = pilot_settings.0.gravity;
                let (lo, hi) = ((2.0 * g * 10.0f32).sqrt(), (2.0 * g * 70.0f32).sqrt());
                let k = ((-f.prev_vz - lo) / (hi - lo)).clamp(0.0, 1.0);
                f.dip_vel -= 60.0 + 260.0 * k;
            }
            f.was_grounded = grounded;
            f.prev_vz = s.vel.z;
            // Spring the dip back.
            f.dip_vel += (-f.dip * 180.0 - f.dip_vel * 22.0) * dt;
            f.dip += f.dip_vel * dt;
            // Smooth step-ups/downs on the ground; follow exactly in the air.
            let target_z = pilot_eye.z;
            let eye_z = match f.eye_z {
                Some(z) if grounded && (target_z - z).abs() < 40.0 => z + (target_z - z) * (1.0 - (-dt * 18.0).exp()),
                _ => target_z,
            };
            f.eye_z = Some(eye_z);
            // Sprinting lowers the view (sprintViewOffset) and leans into turns (sprinttiltMaxRoll).
            let sprinting = s.sprinting && grounded;
            let blend = |cur: f32, want: f32, rate: f32| cur + (want - cur) * (1.0 - (-dt * rate).exp());
            f.sprint = blend(f.sprint, if sprinting { 1.0 } else { 0.0 }, 6.0);
            let turn = ((s.yaw - f.prev_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI) / dt.max(1e-3);
            f.prev_yaw = s.yaw;
            // Sprint lean (client.dll m_sprintTiltFrac/Vel): the turn rate over sprinttilt_turnrange
            // (120 deg/s) is the target fraction, reached at up to sprinttilt_maxvel (2/s) with
            // sprinttilt_accel (35/s^2); the roll is that times sprinttiltMaxRoll (pilot_base.set 2).
            // How the engine steers the velocity is a guess.
            let target = (-turn.to_degrees() / 120.0).clamp(-1.0, 1.0) * if sprinting { 1.0 } else { 0.0 };
            let want_vel = ((target - f.tilt_frac) * 12.0).clamp(-2.0, 2.0);
            f.tilt_vel += (want_vel - f.tilt_vel).clamp(-35.0 * dt, 35.0 * dt);
            f.tilt_frac = (f.tilt_frac + f.tilt_vel * dt).clamp(-1.0, 1.0);
            let tilt = f.tilt_frac * 2f32.to_radians() * f.sprint;
            // Head bob with the stride.
            let speed = s.horizontal_speed();
            if grounded && speed > 20.0 {
                f.bob_phase += speed * dt / 64.0 * std::f32::consts::TAU;
            }
            let bob_amp = if grounded { (speed / 243.0).min(1.0) * (0.5 + 0.7 * f.sprint) } else { 0.0 };
            f.bob = blend(f.bob, bob_amp, 10.0);
            let bob = f.bob_phase.sin().abs() * f.bob;
            // Wall-running tilts the head away from the wall (feet on it): a wall on the left rolls
            // the view clockwise. client.dll convars: wallrun_maxViewTilt 15 degrees,
            // wallrun_viewTiltSpeed 6, and wallrun_viewTiltPredictTime 0.25 s - the tilt starts
            // that long before reaching a wall (a ray along the horizontal velocity here).
            let wall_normal = match s.mode {
                PilotMove::WallRun { normal, .. } | PilotMove::WallHang { normal, .. } => Some(normal),
                PilotMove::Air if s.horizontal_speed() > 100.0 => {
                    let h = SVec3::new(s.vel.x, s.vel.y, 0.0);
                    let mid = s.pos + SVec3::Z * 36.0;
                    world.0.raycast(mid, h.normalize(), h.length() * WALL_TILT_PREDICT + 22.0).map(|hit| hit.normal).filter(|n| {
                        n.z.abs() < 0.35 && 1.0 - h.normalize().dot(SVec3::new(n.x, n.y, 0.0).normalize_or_zero()).abs() > 0.3
                    })
                }
                _ => None,
            };
            let wall_roll = wall_normal.map_or(0.0, |n| {
                let right = Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0);
                right.dot(Vec3::new(n.x, n.y, 0.0)) * WALL_MAX_VIEW_TILT.to_radians()
            });
            f.wall_roll = blend(f.wall_roll, wall_roll, WALL_VIEW_TILT_SPEED);
            // Sliding sideways tilts the view (client.dll slide_viewTiltSide 15 degrees at
            // slide_viewTiltPlayerSpeed 400 of sideways speed, easing in at
            // slide_viewTiltIncreaseSpeed 5 and out at slide_viewTiltDecreaseSpeed 2.5; tilting
            // toward the slide is a guess at the sign).
            let slide_roll = if s.mode == PilotMove::Slide {
                let right = Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0);
                (right.dot(Vec3::new(s.vel.x, s.vel.y, 0.0)) / 400.0).clamp(-1.0, 1.0) * 15f32.to_radians()
            } else {
                0.0
            };
            let rate = if slide_roll.abs() > f.slide_roll.abs() { 5.0 } else { 2.5 };
            f.slide_roll = blend(f.slide_roll, slide_roll, rate);
            f.roll = f.wall_roll + tilt + f.slide_roll;
            // Slides widen the view (slideFOVScale 1.1 over slideFOVLerpIn/OutTime 0.25 s).
            f.slide_fov = blend(f.slide_fov, if s.mode == PilotMove::Slide { 1.0 } else { 0.0 }, 4.0);
            if let Projection::Perspective(pp) = projection.as_mut() {
                pp.fov *= 1.0 + 0.1 * f.slide_fov;
            }
            let eye = Vec3::new(pilot_eye.x, pilot_eye.y, eye_z - 6.0 * f.sprint + f.dip - bob);
            *vm_eye = VmEye { offset: eye - pilot_eye, roll: f.roll };
            let (py, pp) = (punch.yaw.to_radians(), punch.pitch.to_radians() - f.dip * 0.004);
            (eye, view_quat(s.yaw + py, s.pitch + pp) * Quat::from_rotation_x(f.roll + punch.roll.to_radians()))
        }
        Control::Disembark(t) => {
            // Rise out of the cockpit and over the hatch.
            let k = smooth(t / clip_secs(pilot.disembark_anim, DISEMBARK_SECS));
            let fwd = bt_rot * Vec3::X;
            let out = bt_eye + fwd * 120.0 + Vec3::Z * 90.0;
            (bt_eye.lerp(out, k), view_quat(titan.state.yaw, 0.25 * k))
        }
        Control::Embark(t) => {
            // Walk the camera up to BT's chest as he kneels, then settle into the cockpit.
            let (from_p, from_r) = pilot.embark_from.unwrap_or((pilot_eye, view_quat(s.yaw, s.pitch)));
            let into = chest.map(|c| c + bt_rot * Vec3::new(30.0, 0.0, 40.0)).unwrap_or(bt_eye);
            let secs = clip_secs(pilot.embark_anim, EMBARK_SECS);
            let a = smooth(t / (secs * 0.6));
            let b = smooth((t - secs * 0.6) / (secs * 0.4));
            let p = from_p.lerp(into, a).lerp(bt_eye, b);
            // Turn to face BT's chest as he reaches for you, then settle into his view.
            let target = chest.unwrap_or(bt_eye);
            let d = target - p;
            let look = view_quat(d.y.atan2(d.x), -d.z.atan2(d.truncate().length().max(1.0)));
            let turn = smooth(t / (secs * 0.3));
            (p, from_r.slerp(look, turn).slerp(bt_rot, b))
        }
        Control::Titan => unreachable!(),
    };
    let jitter = shake.0 * shake.0;
    let t = time.elapsed_secs();
    let shake_off = Vec3::new((t * 53.0).sin(), (t * 61.0).cos(), (t * 47.0).sin()) * 6.0 * jitter;
    vm_eye.offset += shake_off;
    let dir = rot * Vec3::X;
    let up = rot * Vec3::Z;
    *cam = Transform::from_translation(to_bevy(pos + shake_off)).looking_to(to_bevy(dir).normalize(), to_bevy(up).normalize());
}

fn from_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y) / crate::player::UNIT
}


/// How far the Pilot's view sits from the simulated eye (sprint lowering, head bob, landing
/// dip, step smoothing, shake) and its roll, so first-person models ride with the view as the
/// game draws them from the view origin. Set by `pilot_camera` (read by the viewmodels on the
/// next frame; the offset changes smoothly). Game space.
#[derive(Resource, Default, Clone, Copy)]
pub struct VmEye {
    pub offset: Vec3,
    pub roll: f32,
}

impl VmEye {
    /// The view origin and rotation (no recoil punch) for first-person models.
    pub fn frame(&self, s: &tf_sim::pilot::PilotState, settings: &PilotParams) -> (Vec3, Quat) {
        let eye = Vec3::from(s.eye(settings).to_array()) + self.offset;
        (eye, view_quat(s.yaw, s.pitch) * Quat::from_rotation_x(self.roll))
    }
}

const WALL_MAX_VIEW_TILT: f32 = 15.0;
const WALL_VIEW_TILT_SPEED: f32 = 6.0;
const WALL_TILT_PREDICT: f32 = 0.25;

/// Pilot camera smoothing and feel.
#[derive(Default)]
pub struct CamFeel {
    eye_z: Option<f32>,
    was_grounded: bool,
    prev_vz: f32,
    prev_yaw: f32,
    dip: f32,
    dip_vel: f32,
    sprint: f32,
    roll: f32,
    wall_roll: f32,
    tilt_frac: f32,
    tilt_vel: f32,
    bob: f32,
    bob_phase: f32,
    slide_fov: f32,
    slide_roll: f32,
}

/// The cockpit boot after embarking (ServerCallback_TitanCockpitBoot, cl_titan_cockpit.nut):
/// exposure compensation snaps to -6 stops, and 0.1 s later goes back to 0 while auto-exposure
/// brings the view up. Bevy has no auto-exposure here, so the recovery is an ease-out over about
/// a second (TitanCockpit_IsBooting counts 1.3 s); every camera dims together.
pub fn cockpit_boot(
    time: Res<Time>,
    control: Res<Control>,
    mut cams: Query<(Entity, &mut bevy::camera::Exposure)>,
    mut state: Local<(Option<f32>, bool, std::collections::HashMap<Entity, f32>)>,
) {
    let (boot, was_embarking, base) = &mut *state;
    let embarking = matches!(*control, Control::Embark(_));
    if *was_embarking && *control == Control::Titan {
        *boot = Some(0.0);
    }
    *was_embarking = embarking;
    let Some(t) = boot.as_mut() else { return };
    *t += time.delta_secs();
    let stops = if *t < 0.1 { 6.0 } else { 6.0 * (-(*t - 0.1) / 0.3).exp() };
    let done = *t > 1.6 || *control != Control::Titan;
    for (e, mut ex) in &mut cams {
        let b = *base.entry(e).or_insert(ex.ev100);
        // A higher EV100 is a darker picture.
        ex.ev100 = if done { b } else { b + stops };
    }
    if done {
        *boot = None;
        base.clear();
    }
}
