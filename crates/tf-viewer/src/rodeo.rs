//! Rodeo: the Pilot lands on an enemy Titan, climbs to its back and rips out its battery
//! (`_rodeo_titan.gnut`, `sh_rodeo_titan*.nut`):
//! - Landing on a Titan while airborne attaches you (RODEO_APPROACH_JUMP_ON / FALLING_FROM_ABOVE);
//!   the entrance animation is picked by the side you came from and how high you were
//!   (`pt_rodeo_move_<chassis>_<front|back|left|right>[_lower|_mid]_entrance`, and the
//!   first-person `ptpov_` twin on the arms). Chassis: atlas (Ion, Tone), ogre (Scorch, Legion),
//!   stryder (Northstar, Ronin).
//! - On the back you ride along; a Titan that still has its battery gets it ripped out
//!   (`ptpov_rodeo_*_hijack_battery`): at the `rodeo_battery_rip` event the Titan takes one
//!   health segment (`GetSegmentHealthForTitan`: `healthPerSegment` 1500 in the SP player
//!   settings; a doomed Titan dies) and you are pushed off at 450 u/s with 80 + 100 up
//!   (RODEO_BATTERY_RIP_PILOT_PUSHED_OFF_*, ThrowRiderOff), carrying the battery.
//! - Without a battery you drop a grenade in the open hatch (`ptpov_rodeo_*_grenade_1st`) for
//!   the same damage, then jump clear.
//! - Jump leaves the Titan at 350 u/s along the input, 390 up (after a 0.6 s debounce).
//! - Embarking BT with a battery applies it: half a segment of health (`battery_health_frac`
//!   0.5; BT's `healthPerSegment` is 1800), full shield, 20% core (`battery_core_frac`).
//!
//! The rider is parented to the Titan's `HIJACK` attachment (on `def_c_spineC`, like
//! `GetRodeoSpotOrigin`: attachment + 40 up is the ride spot, 180 units is the attach range,
//! `RodeoDistanceIsTooFar`). The first-person arms play the rodeo clips in that frame and the
//! view follows their `jx_c_camera` bone (attachment frames: forward = bone Z, left = X, up = Y),
//! so the climb, the battery rip and the lean over the hatch look like the game's.
//! The entrance is picked like `GetRodeoDirection`/`GetRodeoDirectionFromAbove`: falling onto
//! the Titan (40 above the spot, falling faster than 120, looking at it) uses the upper
//! entrances; otherwise `fromBelow` (Titan less than 70 below you) and the forward/right dots
//! choose front/back lower, back mid, left or right.
//!
//! Not modelled: the enemy's anti-rodeo electric smoke, rodeoing friendly BT to apply the
//! battery from outside, the third-person rider on the Titan's back.

use crate::actor::{Actor, Layer};
use crate::fparms::{attach_frame, FpArms};
use crate::particles::{emit, Effect};
use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::player::{to_bevy, CameraMode, PlayerInput};
use crate::targets::{Enemy, EnemyKits};
use crate::ui::UiAssets;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::PilotMove;

/// `healthPerSegment` [$sp] of the enemy Titans' player settings.
pub const SP_TITAN_SEGMENT_HEALTH: f32 = 1500.0;
/// titan_buddy.set healthPerSegment.
pub const BT_SEGMENT_HEALTH: f32 = 1800.0;
pub const BATTERY_HEALTH_FRAC: f32 = 0.5;
pub const BATTERY_CORE_FRAC: f32 = 0.2;
const RIP_PUSH_HORIZONTAL: f32 = 450.0;
const RIP_PUSH_VERTICAL: f32 = 80.0 + 100.0;
const JUMP_OFF_SPEED: f32 = 350.0;
const JUMP_OFF_UP: f32 = 390.0;
const JUMP_DEBOUNCE: f32 = 0.6;
/// How long a rider has been on an NPC Titan's back before it pops its Electric Smoke (the
/// NPC's anti-rodeo timing is server-side script that isn't shipped; 2 s is an approximation).
const ANTI_RODEO_SMOKE_DELAY: f32 = 2.0;
/// Where in the hijack / grenade clips the damage lands (the `rodeo_battery_rip` event sits
/// at 0.81-0.99 of the pilot's hijack animations; `signal:RodeoPointOfNoReturn` at 0.585).
const RIP_CYCLE: f32 = 0.85;
const NO_RETURN_CYCLE: f32 = 0.585;
/// RodeoDistanceIsTooFar: the rider must be within this of the ride spot.
const ATTACH_RANGE: f32 = 180.0;
/// The view blends from where you were onto the entrance clip's camera over this long.
const VIEW_BLEND: f32 = 0.25;
const GRENADE_CYCLE: f32 = 0.7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chassis {
    Atlas,
    Ogre,
    Stryder,
}

impl Chassis {
    pub fn of_model(model: &str) -> Self {
        let m = model.to_ascii_lowercase();
        if m.contains("titan_heavy") {
            Chassis::Ogre
        } else if m.contains("titan_light") {
            Chassis::Stryder
        } else {
            Chassis::Atlas
        }
    }
    fn name(self) -> &'static str {
        match self {
            Chassis::Atlas => "atlas",
            Chassis::Ogre => "ogre",
            Chassis::Stryder => "stryder",
        }
    }
    /// The ride idle; the arms model has no ogre idle, so Scorch/Legion use the atlas one.
    fn idle_1p(self) -> &'static str {
        match self {
            Chassis::Stryder => "ptpov_rodeo_move_stryder_back_idle",
            _ => "ptpov_rodeo_move_atlas_back_idle",
        }
    }
    fn hijack_1p(self) -> &'static str {
        match self {
            Chassis::Atlas => "ptpov_rodeo_ride_R_hijack_battery",
            Chassis::Ogre => "ptpov_rodeo_ogre_R_hijack_battery",
            Chassis::Stryder => "ptpov_rodeo_stryder_R_hijack_battery",
        }
    }
    /// The Titan's side of the rip (only the medium chassis has one: `at_rodeo_panel_opening`
    /// aliases); it stands still for it. The others keep walking.
    fn hijack_titan(self) -> Option<&'static str> {
        match self {
            Chassis::Atlas => Some("at_rodeo_ride_R_hijack_battery"),
            _ => None,
        }
    }
    fn grenade_1p(self) -> &'static str {
        match self {
            Chassis::Atlas => "ptpov_rodeo_medium_grenade_1st",
            Chassis::Ogre => "ptpov_rodeo_heavy_grenade_1st",
            Chassis::Stryder => "ptpov_rodeo_light_grenade_1st",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Entrance,
    Idle,
    Hijack,
    Grenade,
}

pub struct Ride {
    pub target: Entity,
    chassis: Chassis,
    phase: Phase,
    /// Seconds into the phase.
    t: f32,
    /// Seconds since attaching.
    since: f32,
    entrance: String,
    /// The eye and view when attaching (the entrance blends from there).
    from: (Vec3, Quat),
    done_hit: bool,
    /// The Titan is held still (and animated here) for the rip.
    holds: bool,
}

#[derive(Resource)]
pub struct Rodeo {
    pub ride: Option<Ride>,
    /// No re-attach for this long after leaving.
    pub debounce: f32,
    /// A stolen battery on the Pilot's back.
    pub battery: bool,
    /// The view while riding (game space: eye, rotation with forward X / up Z).
    pub cam: Option<(Vec3, Quat)>,
}

impl Default for Rodeo {
    fn default() -> Self {
        Self { ride: None, debounce: 0.0, battery: std::env::var_os("TF_BATTERY").is_some(), cam: None }
    }
}

/// The Titan's `HIJACK` attachment (on `def_c_spineC`) in game space: the bone's position,
/// facing the Titan's way (the bone's own tilt is dropped, like `GetFrontRightDots` does; the
/// rodeo clips sit a little lower on a leaning Titan than in the game).
fn hijack_attachment(actors: &Query<&mut Actor>, titan_tf: &Transform, e: &Enemy) -> Transform {
    let spine = actors.get(e.actor).ok().and_then(|a| a.bone_model_transform("def_c_spineC")).map(|t| t.translation).unwrap_or(Vec3::Z * e.height * 0.75);
    Transform::from_translation(titan_tf.transform_point(spine)).with_rotation(Quat::from_rotation_z(e.state.yaw))
}

impl Rodeo {
    pub fn riding(&self) -> bool {
        self.ride.is_some()
    }
}

fn view_quat(yaw: f32, pitch: f32) -> Quat {
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn rodeo_update(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    settings: Res<PilotSettings>,
    mut input: ResMut<PlayerInput>,
    mut rodeo: ResMut<Rodeo>,
    mut pilots: Query<&mut PlayerPilot>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    kits: Res<EnemyKits>,
    mut actors: Query<&mut Actor>,
    mut tfs: Query<(&mut Transform, &mut Visibility)>,
    fp_arms: Option<Res<FpArms>>,
) {
    let dt = time.delta_secs().min(0.1);
    rodeo.debounce = (rodeo.debounce - dt).max(0.0);
    let Ok(mut pilot) = pilots.single_mut() else { return };
    let on_foot = *control == Control::Pilot && *mode != CameraMode::Free;

    // --- Attach: airborne and against a living enemy Titan's hull ---
    if rodeo.ride.is_none() {
        // TF_RODEO_TEST: stand the first landed enemy Titan 110 units ahead of the Pilot, held
        // still, so a scripted double jump lands on it. TF_BATTERY=1 starts with a battery.
        if on_foot && pilot.state.mode == PilotMove::Ground && std::env::var_os("TF_RODEO_TEST").is_some() {
            let me = Vec3::from(pilot.state.pos.to_array());
            let fwd = Vec3::new(pilot.state.yaw.cos(), pilot.state.yaw.sin(), 0.0);
            if let Some((id, mut e)) = enemies.iter_mut().find(|(_, e)| e.alive() && !e.infantry && !e.arriving()) {
                let p = me + fwd * 110.0;
                e.pos = p;
                e.state.pos = SVec3::new(p.x, p.y, p.z);
                e.state.vel = SVec3::ZERO;
                e.state.yaw = pilot.state.yaw;
                e.held = true;
                // Held Titans skip the enemy update, so place the anchor here too.
                if let Ok((mut tf, _)) = tfs.get_mut(id) {
                    tf.translation = p;
                    tf.rotation = Quat::from_rotation_z(e.state.yaw);
                }
            }
        }
        if !on_foot || rodeo.debounce > 0.0 || pilot.state.mode != PilotMove::Air {
            return;
        }
        let p = Vec3::from(pilot.state.pos.to_array());
        if std::env::var_os("TF_RODEO_TEST").is_some() {
            for (_, e) in enemies.iter().filter(|(_, e)| e.alive() && !e.infantry) {
                log::debug!("rodeo test: pilot {:.0} vel {:.0}; titan {:.0} d {:.0} dz {:.0} arriving {} held {}", p, Vec3::from(pilot.state.vel.to_array()), e.pos, (p - e.pos).truncate().length(), p.z - e.pos.z, e.arriving(), e.held);
            }
        }
        let mut best: Option<(Entity, f32)> = None;
        for (id, e) in enemies.iter() {
            let hold = e.held && std::env::var_os("TF_RODEO_TEST").is_some();
            if !e.alive() || e.infantry || e.arriving() || e.held && !hold {
                continue;
            }
            let d = (p - e.pos).truncate().length();
            let dz = p.z - e.pos.z;
            if (d < e.radius + 30.0 || hold && d < e.radius + 120.0) && dz > e.height * 0.35 && dz < e.height + 70.0 && best.is_none_or(|b| d < b.1) {
                best = Some((id, d));
            }
        }
        let Some((id, _)) = best else { return };
        let Ok((_, e)) = enemies.get(id) else { return };
        let Ok((titan_tf, _)) = tfs.get(id) else { return };
        let attach = hijack_attachment(&actors, titan_tf, e);
        let spot = attach.translation + Vec3::Z * 40.0;
        if (spot - p).length() > ATTACH_RANGE {
            log::debug!("rodeo: {:.0} from the ride spot, too far (pilot {:.0}, titan {:.0}, anchor {:.0}, attachment {:.0})", (spot - p).length(), p, e.pos, titan_tf.translation, attach.translation);
            return;
        }
        let chassis = Chassis::of_model(&kits.0[e.kit].model);
        // GetFrontRightDots: where the rider is, in the Titan's frame (x forward, y left).
        let rel = (Quat::from_rotation_z(-e.state.yaw) * (p - e.pos)).truncate().normalize_or_zero();
        let (forward_dot, right_dot) = (rel.x, -rel.y);
        let vel = Vec3::from(pilot.state.vel.to_array());
        let eye = Vec3::from(pilot.state.eye(&settings.0).to_array());
        let view = view_quat(pilot.state.yaw, pilot.state.pitch) * Vec3::X;
        let to_spot = (spot - eye).normalize_or_zero();
        let falling = p.z - spot.z >= 40.0 && vel.z <= -120.0 && view.dot(to_spot) >= 0.8 && vel.normalize_or_zero().dot(to_spot) >= 0.8;
        let (side, height) = if falling {
            // GetRodeoDirectionFromAbove
            if forward_dot > 0.1 {
                ("front", "")
            } else if forward_dot < -0.88 {
                ("back", "")
            } else if right_dot < 0.0 {
                ("left", "")
            } else {
                ("right", "")
            }
        } else {
            // GetRodeoDirection
            let from_below = e.pos.z - p.z > -70.0;
            if from_below {
                if forward_dot > 0.0 {
                    ("front", "_lower")
                } else {
                    ("back", "_lower")
                }
            } else if forward_dot > 0.45 {
                ("front", "_lower")
            } else if forward_dot < -0.75 {
                ("back", "_mid")
            } else if right_dot > 0.0 {
                ("right", "")
            } else {
                ("left", "")
            }
        };
        let entrance = format!("ptpov_rodeo_move_{}_{side}{height}_entrance", chassis.name());
        log::info!("rodeo: {} from the {side}{height} ({entrance}; {})", kits.0[e.kit].name, if falling { "falling onto it" } else { "jumped on" });
        let from = (eye, view_quat(pilot.state.yaw, pilot.state.pitch));
        rodeo.ride = Some(Ride { target: id, chassis, phase: Phase::Entrance, t: 0.0, since: 0.0, entrance, from, done_hit: false, holds: false });
        if std::env::var_os("TF_RODEO_TEST").is_some() {
            if let Ok((_, mut e)) = enemies.get_mut(id) {
                e.held = false;
            }
        }
        pilot.state.jumps = 0;
        crate::audio::event(&mut commands, "Pilot_Rodeo_Titan_Attach");
        return;
    }

    // --- Riding ---
    let arms = fp_arms.map(|a| (a.anchor, a.actor));
    let mut got_battery = false;
    let mut ride = rodeo.ride.take().unwrap();
    ride.t += dt;
    ride.since += dt;
    let Ok((_, mut e)) = enemies.get_mut(ride.target) else {
        return;
    };
    let titan_rot = Quat::from_rotation_z(e.state.yaw);
    let Ok(titan_tf) = tfs.get(ride.target).map(|(t, _)| *t) else { return };
    let attach = hijack_attachment(&actors, &titan_tf, &e);
    let anchor_rot = attach.rotation;
    let spot = attach.translation + Vec3::Z * 40.0;
    let titan_vel = Vec3::from(e.state.vel.to_array());
    let mut detach: Option<Vec3> = None;
    if !e.alive() || !on_foot {
        detach = Some(titan_vel);
    }
    // Clip timing from the arms actor (the body's twins have the same lengths).
    let clip_len = |actors: &Query<&mut Actor>, name: &str| arms.and_then(|(_, a)| actors.get(a).ok().and_then(|ac| ac.clip(name).map(|c| ac.clips[c].duration))).unwrap_or(1.2);
    let (clip_name, looped): (String, bool) = match ride.phase {
        Phase::Entrance => (ride.entrance.clone(), false),
        Phase::Idle => (ride.chassis.idle_1p().to_string(), true),
        Phase::Hijack => (ride.chassis.hijack_1p().to_string(), false),
        Phase::Grenade => (ride.chassis.grenade_1p().to_string(), false),
    };
    let len = clip_len(&actors, &clip_name);
    let cycle = if looped { (ride.t / len.max(1e-3)).rem_euclid(1.0) } else { (ride.t / len.max(1e-3)).min(0.999) };

    // Jump off (before the point of no return of a hijack).
    let can_jump = ride.since > JUMP_DEBOUNCE && matches!(ride.phase, Phase::Entrance | Phase::Idle) || (ride.phase == Phase::Hijack && cycle < NO_RETURN_CYCLE);
    if input.jump && can_jump && detach.is_none() {
        let yaw = input.yaw;
        let fwd = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
        let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
        let wish = fwd * input.forward + right * input.right;
        let dir = if wish.length() > 0.1 { wish.normalize() } else { -(titan_rot * Vec3::X) };
        detach = Some(titan_vel + dir * JUMP_OFF_SPEED + Vec3::Z * JUMP_OFF_UP);
        log::info!("rodeo: jumped off");
    }
    input.jump = false;

    match ride.phase {
        Phase::Entrance => {
            if ride.t >= len {
                ride.phase = Phase::Idle;
                ride.t = 0.0;
            }
        }
        Phase::Idle => {
            // Straight to work: the battery, or a grenade down the open hatch.
            if ride.t >= 0.3 {
                ride.phase = if e.battery { Phase::Hijack } else { Phase::Grenade };
                ride.t = 0.0;
                ride.done_hit = false;
                log::info!("rodeo: {}", if e.battery { "ripping the battery" } else { "grenade in the hatch" });
                if e.battery && !e.held && ride.chassis.hijack_titan().is_some_and(|c| actors.get(e.actor).is_ok_and(|a| a.clip(c).is_some())) {
                    e.held = true;
                    ride.holds = true;
                }
            }
        }
        Phase::Hijack | Phase::Grenade => {
            if ride.holds {
                if let (Some(c), Ok(mut a)) = (ride.chassis.hijack_titan(), actors.get_mut(e.actor)) {
                    if let Some(clip) = a.clip(c) {
                        a.autoplay = false;
                        a.layers = vec![Layer { clip, cycle, weight: 1.0 }];
                    }
                }
            }
            let hit_at = if ride.phase == Phase::Hijack { RIP_CYCLE } else { GRENADE_CYCLE };
            if cycle >= hit_at && !ride.done_hit {
                ride.done_hit = true;
                let dmg = if e.doomed() { e.v.health + e.v.shield + 1.0 } else { SP_TITAN_SEGMENT_HEALTH };
                e.damage_bypass(dmg);
                let hatch = e.pos + titan_rot * Vec3::new(0.0, 0.0, e.height * 0.85);
                if ride.phase == Phase::Hijack {
                    e.battery = false;
                    got_battery = true;
                    emit(&mut commands, Effect::Impact { at: to_bevy(hatch), normal: Vec3::Y, scale: 3.0, energy: true });
                    crate::audio::event(&mut commands, "UI_TitanBattery_Pilot_PickUp");
                    log::info!("rodeo: battery ripped, {dmg:.0} damage");
                } else {
                    emit(&mut commands, Effect::Explosion { at: to_bevy(hatch), scale: 1.0 });
                    crate::audio::event_at(&mut commands, "Explo_FragGrenade_Impact_3P", hatch);
                    log::info!("rodeo: grenade went off in the hatch, {dmg:.0} damage");
                }
            }
            if ride.t >= len {
                // Pushed off (CalculateDirectionToThrowOffBatteryThief: away from the Titan).
                let away = (Vec3::from(pilot.state.pos.to_array()) - e.pos).truncate().normalize_or(Vec2::NEG_X);
                detach = Some(titan_vel + Vec3::new(away.x, away.y, 0.0) * RIP_PUSH_HORIZONTAL + Vec3::Z * RIP_PUSH_VERTICAL);
            }
        }
    }
    // Anti-rodeo: the Titan pops its Electric Smoke on itself once you've been on for a while.
    if e.smoke_charges > 0 && ride.since >= ANTI_RODEO_SMOKE_DELAY && e.alive() {
        e.smoke_charges -= 1;
        crate::titankit::spawn_smoke(&mut commands, e.pos, true);
        crate::audio::event_at(&mut commands, "Titan_Offhand_ElectricSmoke_Deploy_3P", e.pos);
        log::info!("rodeo: the Titan released its Electric Smoke");
    }
    pilot.state.vel = SVec3::new(titan_vel.x, titan_vel.y, titan_vel.z);
    pilot.state.mode = PilotMove::Air;
    if got_battery {
        rodeo.battery = true;
    }

    // The first-person arms ride the attachment and play the rodeo clip; the view is their
    // camera bone (the entrance starts from wherever you were).
    let eye_off = Vec3::from(pilot.state.eye(&settings.0).to_array()) - Vec3::from(pilot.state.pos.to_array());
    let mut view = (spot + Vec3::Z * 20.0, view_quat(e.state.yaw, 0.3));
    if let Some((anchor, actor_e)) = arms {
        if let Ok(mut a) = actors.get_mut(actor_e) {
            a.autoplay = false;
            a.aim = None;
            if let Some(c) = a.clip(&clip_name) {
                a.layers = vec![Layer { clip: c, cycle, weight: 1.0 }];
            }
            if let Some(c) = a.bone_model_transform("jx_c_camera") {
                let rot = anchor_rot * c.rotation * attach_frame();
                view = (attach.translation + anchor_rot * c.translation, rot);
                if std::env::var_os("TF_RODEO_TEST").is_some() {
                    log::debug!("rodeo cam {clip_name} cycle {cycle:.2}: bone {:.1} eye {:.0} fwd {:.2}; titan {:.0} yaw {:.0} attach {:.0}", c.translation, view.0, rot * Vec3::X, e.pos, e.state.yaw.to_degrees(), attach.translation);
                }
            }
            if let Ok((mut tf, mut vis)) = tfs.get_mut(anchor) {
                tf.rotation = anchor_rot;
                tf.translation = attach.translation;
                *vis = if detach.is_some() { Visibility::Hidden } else { Visibility::Inherited };
            }
        }
    }
    let k = (ride.since / VIEW_BLEND).clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    let eye = ride.from.0.lerp(view.0, k);
    let rot = ride.from.1.slerp(view.1, k);
    let fwd = rot * Vec3::X;
    let (yaw, pitch) = (fwd.y.atan2(fwd.x), (-fwd.z).clamp(-1.0, 1.0).asin());
    pilot.state.yaw = yaw;
    pilot.state.pitch = pitch;
    input.yaw = yaw;
    input.pitch = pitch;
    let pos = eye - eye_off;
    pilot.state.pos = SVec3::new(pos.x, pos.y, pos.z);
    rodeo.cam = Some((eye, rot));

    if let Some(v) = detach {
        if ride.holds {
            e.held = false;
        }
        pilot.state.vel = SVec3::new(v.x, v.y, v.z);
        pilot.state.mode = PilotMove::Air;
        pilot.state.jumps = 1;
        rodeo.debounce = JUMP_DEBOUNCE;
        rodeo.cam = None;
        if let Some((anchor, _)) = arms {
            if let Ok((_, mut vis)) = tfs.get_mut(anchor) {
                *vis = Visibility::Hidden;
            }
        }
    } else {
        rodeo.ride = Some(ride);
    }
}

/// While riding, the camera is the arms' camera bone (after `pilot_camera`).
pub fn rodeo_camera(rodeo: Res<Rodeo>, mut cams: Query<&mut Transform, With<crate::player::MainCamera>>) {
    let Some((eye, rot)) = rodeo.cam else { return };
    if let Ok(mut cam) = cams.single_mut() {
        *cam = Transform::from_translation(to_bevy(eye)).looking_to(to_bevy(rot * Vec3::X).normalize(), to_bevy(rot * Vec3::Z).normalize());
    }
}

/// HUD: the battery on the Pilot's back.
#[derive(Component)]
pub struct BatteryHud;

pub fn spawn_battery_hud(mut commands: Commands, ui: Res<UiAssets>) {
    let vh = Val::Vh;
    let root = commands
        .spawn((Node { position_type: PositionType::Absolute, left: vh(21.0), bottom: vh(4.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.4), ..default() }, Visibility::Hidden, BatteryHud))
        .id();
    let frame = commands
        .spawn((Node { width: vh(7.0), height: vh(7.0), border: UiRect::all(Val::Px(1.0)), ..default() }, BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4))))
        .id();
    let icon = commands.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ui.image("rui/menu/boosts/boost_icon_battery").unwrap_or_default())).id();
    commands.entity(frame).add_child(icon);
    let label = commands.spawn((Text::new("BATTERY"), TextFont { font: ui.font.clone(), ..default() }, TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)), crate::ui::VhText(1.0))).id();
    commands.entity(root).add_children(&[frame, label]);
}

pub fn battery_hud(rodeo: Res<Rodeo>, control: Res<Control>, mode: Res<CameraMode>, game: Res<crate::game::Game>, hidden: Res<crate::hud::HudHidden>, mut huds: Query<&mut Visibility, With<BatteryHud>>) {
    let show = rodeo.battery && *control == Control::Pilot && *mode != CameraMode::Free && game.in_play() && !hidden.0;
    for mut v in &mut huds {
        *v = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
}
