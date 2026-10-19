//! Rodeo: the Pilot lands on an enemy Titan, climbs to its back and rips out its battery
//! (`_rodeo_titan.gnut`, `sh_rodeo_titan*.nut`):
//! - Landing on a Titan while airborne attaches you (RODEO_APPROACH_JUMP_ON / FALLING_FROM_ABOVE);
//!   the entrance animation is picked by the side you came from and how high you were
//!   (`pt_rodeo_move_<chassis>_<front|back|left|right>[_lower|_mid]_entrance`, and the
//!   first-person `ptpov_` twin on the arms). Chassis: atlas (Ion, Tone), ogre (Scorch, Legion),
//!   stryder (Northstar, Ronin).
//! - On the back you rip the battery out (`ptpov_rodeo_*_hijack_battery`,
//!   PlayerRemovesBatteryPack): at the `rodeo_battery_rip` event the Titan takes one health
//!   segment (RodeoBatteryPackRemovalDamage, `healthPerSegment` 1500 in the SP player settings;
//!   a doomed Titan dies) and you are thrown off (ThrowRiderOff: 100 units up, then 450 u/s
//!   backwards (0.75 of it, or your stick: forward 0.75..1, sideways up to 0.4) with 80 up plus
//!   SP_RODEO_BOOST 300), carrying the battery.
//! - Already carrying a battery (ShouldThrowGrenadeInHatch) you drop a grenade in the open
//!   hatch instead (`ptpov_rodeo_*_grenade_1st`) for half the damage, and are thrown off the
//!   same way. A Titan that lost its battery can be ripped again (the game counts them).
//! - Jump leaves the Titan at 350 u/s along the input, 390 up (after a 0.6 s debounce).
//! - Embarking BT with a battery applies it (Rodeo_ApplyAllBatteriesOnEmbark,
//!   Rodeo_ApplyBatteryToTitan's SP branch): 2000 health, a doomed BT is undoomed to one
//!   segment, what the health can't take goes to the shield, and 98% or more rounds up to
//!   full (BATTERY_PICKUP_IGNORE_FRAC). BT himself can't be rodeoed (titan_buddy.set
//!   `rodeo_allow 0`), so this is the only way to give him one.
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
//! Not modelled: the third-person rider on the Titan's back, the halved air control for
//! 0.75 s after being thrown off (PostRodeoAirControl), friendly NPC Titans to hand a battery
//! to (none exist here).

use crate::actor::{Actor, ActorSpec, Layer};
use crate::fparms::{attach_frame, FpArms};
use crate::particles::{emit, Effect};
use crate::pilotctl::{Control, PilotSettings, PlayerPilot};
use crate::player::{to_bevy, CameraMode, PlayerInput};
use crate::targets::{Enemy, EnemyKits};
use crate::ui::UiAssets;
use bevy::prelude::*;
use std::collections::HashMap;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::PilotMove;

/// `healthPerSegment` [$sp] of the enemy Titans' player settings.
pub const SP_TITAN_SEGMENT_HEALTH: f32 = 1500.0;
/// titan_buddy.set healthPerSegment.
pub const BT_SEGMENT_HEALTH: f32 = 1800.0;
/// Rodeo_ApplyBatteryToTitan, single player: the healing a battery gives.
pub const BATTERY_HEAL_SP: f32 = 2000.0;
/// BATTERY_PICKUP_IGNORE_FRAC: this full or more rounds up to full.
pub const BATTERY_FULL_FRAC: f32 = 0.98;
const RIP_PUSH_HORIZONTAL: f32 = 450.0;
const RIP_PUSH_VERTICAL: f32 = 80.0;
/// SP_RODEO_BOOST: extra height thrown off with in SP, against rodeo spam.
const SP_RODEO_BOOST: f32 = 300.0;
/// ThrowRiderOff lifts the rider this much before the throw.
const RIP_PUSH_LIFT: f32 = 100.0;
const JUMP_OFF_SPEED: f32 = 350.0;
const JUMP_OFF_UP: f32 = 390.0;
const JUMP_DEBOUNCE: f32 = 0.6;
const GRAPPLE_OFF_DEBOUNCE: f32 = 1.5;
/// How long a rider has been on an NPC Titan's back before it pops its Electric Smoke (the
/// NPC's anti-rodeo timing is server-side script that isn't shipped; 2 s is an approximation).
const ANTI_RODEO_SMOKE_DELAY: f32 = 2.0;
/// RodeoDistanceIsTooFar: the rider must be within this of the ride spot.
const ATTACH_RANGE: f32 = 180.0;
/// The view blends from where you were onto the entrance clip's camera over this long.
const VIEW_BLEND: f32 = 0.25;
/// RODEO_BATTERY_MODEL_FOR_RODEO_ANIMS: the battery shown in the hand during the rip (on the
/// `R_HAND` attachment: `ja_r_propHand`, identity axes).
const BATTERY_MODEL: &str = "models/props/titan_battery_static/titan_battery_static.mdl";
/// GRENADE_MODEL: the frag shown in the hand for the hatch grenade (on `PROPGUN`:
/// `ja_c_propGun`, which is also the model's root bone).
const GRENADE_MODEL: &str = "models/weapons/grenades/m20_f_grenade.mdl";

/// Script event cycles of a rodeo clip (they sit on the third-person Pilot clips of
/// pete_scripted_r1.mdl; the first-person arms clips carry none): the hand prop appears
/// (`rodeo_battery_show` / `rodeo_battery_grenade_show`), `signal:RodeoPointOfNoReturn`, and
/// the hit (`rodeo_battery_rip` / `rodeo_battery_grenade_damage`).
#[derive(Clone, Copy)]
struct Events {
    show: f32,
    no_return: f32,
    hit: f32,
}

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
    pub fn name(self) -> &'static str {
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
    /// pt_rodeo_ride_R_hijack_battery (2.667 s). The ogre and stryder Pilot clips aren't
    /// shipped in the campaign files; they use the same marks.
    fn rip_events(self) -> Events {
        Events {
            show: 0.213,
            no_return: 0.625,
            hit: 0.913,
        }
    }
    /// pt_rodeo_<medium|heavy|light>_grenade_1st.
    fn grenade_events(self) -> Events {
        match self {
            Chassis::Atlas => Events {
                show: 0.15,
                no_return: 0.5,
                hit: 0.837,
            },
            Chassis::Ogre => Events {
                show: 0.096,
                no_return: 0.532,
                hit: 0.936,
            },
            Chassis::Stryder => Events {
                show: 0.15,
                no_return: 0.5,
                hit: 0.999,
            },
        }
    }
    /// The `hatchmodel` player setting: the battery seated in the Titan's back socket (the
    /// Titan models have no battery of their own; AddPanelToTitan parents this prop to the
    /// `RODEO_BATTERY` attachment and plays `hatch_rodeo_up_idle`).
    fn hatch_model(self) -> &'static str {
        match self {
            Chassis::Atlas => "models/titans/medium/titan_medium_rodeo_battery.mdl",
            Chassis::Ogre => "models/titans/heavy/titan_heavy_rodeo_battery.mdl",
            Chassis::Stryder => "models/titans/light/titan_light_rodeo_battery.mdl",
        }
    }
    /// The `hatch_rodeo_<state>` alias of the hatch model: `up_idle` (seated), `down_idle`
    /// (sunk 18 units into the socket, out of sight), `up` (rising back, 0.667 s).
    fn hatch_clip(self, state: &str) -> String {
        let t = match self {
            Chassis::Atlas => "medium",
            Chassis::Ogre => "heavy",
            Chassis::Stryder => "light",
        };
        format!("hatch_rodeo_{t}_{state}")
    }
    /// The `RODEO_BATTERY` attachment: on `def_c_spineC`, identity axes (the Ogre's is also
    /// tilted 4.6 degrees about X, dropped here).
    fn socket(self) -> Vec3 {
        match self {
            Chassis::Atlas => Vec3::new(0.1, 58.3, -40.3),
            Chassis::Ogre => Vec3::new(0.0, 74.5, -33.7),
            Chassis::Stryder => Vec3::new(0.0, 49.2, -15.1),
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
    /// Where the phase's clip is (0..1), for the props.
    cycle: f32,
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
    /// The ridden Titan's hatch battery has sunk out of sight (the show event passed).
    pub hatch_down: Option<HatchDown>,
}

/// The hatch battery of the Titan being rodeoed, sunk: for a rip, snapped down at the show
/// event (`hatch_rodeo_down_idle` while the hand holds the battery over the socket); for a
/// grenade, lowered (`hatch_rodeo_down`, 0.667 s) as soon as the rider lands
/// (Rodeo_MoveBatteryDown when ShouldThrowGrenadeInHatch). When the ride ends it rises again
/// (`hatch_rodeo_up`) if the rip got past the point of no return or a grenade went in, else it
/// snaps straight back (PlayerRemovesBatteryPack).
#[derive(Clone, Copy)]
pub struct HatchDown {
    pub target: Entity,
    pub rises: bool,
    /// Play the `down` clip instead of snapping to `down_idle`.
    pub lower: bool,
}

impl Default for Rodeo {
    fn default() -> Self {
        Self {
            ride: None,
            debounce: 0.0,
            battery: std::env::var_os("TF_BATTERY").is_some(),
            cam: None,
            hatch_down: None,
        }
    }
}

/// The Titan's `HIJACK` attachment in game space: on `def_c_spineC` at the bone's origin with
/// the axes forward = bone Z, left = bone X, up = bone Y (attachdump: ajax, ogre, raptor all
/// the same). The rider is parented to it with `useAnimatedRefAttachment`, so it follows the
/// bone's animated pose: the torso's aim pitch and lean carry the rider, and the hands land on
/// the `RODEO_BATTERY` socket (40 behind and 58 above it on the ajax) in every pose.
fn hijack_attachment(actors: &Query<&mut Actor>, titan_tf: &Transform, e: &Enemy) -> Transform {
    match actors
        .get(e.actor)
        .ok()
        .and_then(|a| a.bone_model_transform("def_c_spineC"))
    {
        Some(b) => Transform::from_translation(titan_tf.transform_point(b.translation))
            .with_rotation(titan_tf.rotation * b.rotation * attach_frame()),
        None => Transform::from_translation(titan_tf.transform_point(Vec3::Z * e.height * 0.75))
            .with_rotation(Quat::from_rotation_z(e.state.yaw)),
    }
}

impl Rodeo {
    pub fn riding(&self) -> bool {
        self.ride.is_some()
    }
}

impl Ride {
    /// For the network: the Titan ridden, the phase (1 entrance, 2 idle, 3 rip, 4 grenade),
    /// the entrance side (0 back, 1 front, 2 left, 3 right) and height (0, 1 lower, 2 mid),
    /// seconds into the phase.
    pub fn net_state(&self) -> (Entity, u8, u8, u8, f32) {
        let phase = match self.phase {
            Phase::Entrance => 1,
            Phase::Idle => 2,
            Phase::Hijack => 3,
            Phase::Grenade => 4,
        };
        let rest = self
            .entrance
            .trim_start_matches("ptpov_rodeo_move_")
            .trim_end_matches("_entrance");
        let rest = rest.split_once('_').map(|(_, r)| r).unwrap_or(rest);
        let side = match rest.split('_').next().unwrap_or("back") {
            "front" => 1,
            "left" => 2,
            "right" => 3,
            _ => 0,
        };
        let height = if rest.ends_with("_lower") {
            1
        } else if rest.ends_with("_mid") {
            2
        } else {
            0
        };
        (self.target, phase, side, height, self.t)
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
    world: Res<crate::player::Collision>,
) {
    let dt = time.delta_secs().min(0.1);
    rodeo.debounce = (rodeo.debounce - dt).max(0.0);
    let Ok(mut pilot) = pilots.single_mut() else {
        return;
    };
    let on_foot = *control == Control::Pilot && *mode != CameraMode::Free;

    // --- Attach: airborne and against a living enemy Titan's hull ---
    if rodeo.ride.is_none() {
        // TF_RODEO_TEST: stand the first landed enemy Titan 110 units ahead of the Pilot, held
        // still, so a scripted double jump lands on it. TF_BATTERY=1 starts with a battery.
        if on_foot
            && pilot.state.mode == PilotMove::Ground
            && std::env::var_os("TF_RODEO_TEST").is_some()
        {
            let me = Vec3::from(pilot.state.pos.to_array());
            let fwd = Vec3::new(pilot.state.yaw.cos(), pilot.state.yaw.sin(), 0.0);
            if let Some((id, mut e)) = enemies
                .iter_mut()
                .find(|(_, e)| e.alive() && !e.infantry && !e.arriving())
            {
                // TF_RODEO_DIST=units: park it further out (grapple tests).
                let dist = std::env::var("TF_RODEO_DIST")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(110.0);
                let p = me + fwd * dist;
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
                // TF_RODEO_PITCH=deg: hold the torso's aim pitch (a tilted spine moves the
                // hatch, like a Titan aiming up at its rider), to check the rider follows.
                if let Some(pitch) = std::env::var("TF_RODEO_PITCH")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                {
                    if let Ok(mut a) = actors.get_mut(e.actor) {
                        if a.aim.as_ref().is_none_or(|x| x.pitch != pitch) {
                            log::debug!(
                                "rodeo test: aim grid {} ({}), pitch {pitch}",
                                e.aim_grids[0],
                                if a.grids.contains_key(e.aim_grids[0]) {
                                    "found"
                                } else {
                                    "missing"
                                }
                            );
                        }
                        a.aim = Some(crate::actor::Aim {
                            grid: e.aim_grids[0].into(),
                            yaw: 0.0,
                            pitch,
                            weight: 1.0,
                        });
                    }
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
            if (d < e.radius + 30.0 || hold && d < e.radius + 120.0)
                && dz > e.height * 0.35
                && dz < e.height + 70.0
                && best.is_none_or(|b| d < b.1)
            {
                best = Some((id, d));
            }
        }
        let Some((id, _)) = best else { return };
        let Ok((_, e)) = enemies.get(id) else { return };
        let Ok((titan_tf, _)) = tfs.get(id) else {
            return;
        };
        let attach = hijack_attachment(&actors, titan_tf, e);
        let spot = attach.translation + Vec3::Z * 40.0;
        if (spot - p).length() > ATTACH_RANGE {
            log::debug!("rodeo: {:.0} from the ride spot, too far (pilot {:.0}, titan {:.0}, anchor {:.0}, attachment {:.0})", (spot - p).length(), p, e.pos, titan_tf.translation, attach.translation);
            return;
        }
        let chassis = Chassis::of_model(&kits.0[e.kit].model);
        // GetFrontRightDots: where the rider is, in the Titan's frame (x forward, y left).
        let rel = (Quat::from_rotation_z(-e.state.yaw) * (p - e.pos))
            .truncate()
            .normalize_or_zero();
        let (forward_dot, right_dot) = (rel.x, -rel.y);
        let vel = Vec3::from(pilot.state.vel.to_array());
        let eye = Vec3::from(pilot.state.eye(&settings.0).to_array());
        let view = view_quat(pilot.state.yaw, pilot.state.pitch) * Vec3::X;
        let to_spot = (spot - eye).normalize_or_zero();
        let falling = p.z - spot.z >= 40.0
            && vel.z <= -120.0
            && view.dot(to_spot) >= 0.8
            && vel.normalize_or_zero().dot(to_spot) >= 0.8;
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
        let entrance = format!(
            "ptpov_rodeo_move_{}_{side}{height}_entrance",
            chassis.name()
        );
        log::info!(
            "rodeo: {} from the {side}{height} ({entrance}; {})",
            kits.0[e.kit].name,
            if falling {
                "falling onto it"
            } else {
                "jumped on"
            }
        );
        // The rodeo package's worldSound for everyone but the ridden Titan's player (Low
        // Profile, PAS_STEALTH_MOVEMENT, climbs on silently).
        if !crate::kits::has("pas_stealth_movement") {
            crate::audio::event(
                &mut commands,
                &climb_sound(chassis.name(), side, height, false),
            );
        }
        let from = (eye, view_quat(pilot.state.yaw, pilot.state.pitch));
        rodeo.ride = Some(Ride {
            target: id,
            chassis,
            phase: Phase::Entrance,
            cycle: 0.0,
            t: 0.0,
            since: 0.0,
            entrance,
            from,
            done_hit: false,
            holds: false,
        });
        // Carrying a battery: the container sinks right away for the grenade.
        if rodeo.battery {
            rodeo.hatch_down = Some(HatchDown {
                target: id,
                rises: true,
                lower: true,
            });
        }
        if std::env::var_os("TF_RODEO_TEST").is_some() {
            if let Ok((_, mut e)) = enemies.get_mut(id) {
                e.held = false;
            }
        }
        pilot.state.jumps = 0;
        // A grapple that pulled us here ends with the mount.
        if pilot.state.grapple.is_some() {
            pilot.state.release_grapple(&settings.0);
        }
        crate::audio::event(&mut commands, "Pilot_Rodeo_Titan_Attach");
        return;
    }

    // --- Riding ---
    let arms = fp_arms.map(|a| (a.anchor, a.actor));
    let mut got_battery = false;
    let mut ride = rodeo.ride.take().unwrap();
    ride.t += dt;
    ride.since += dt;
    // The hook still flies while riding (the hook update runs whatever the move does,
    // client.dll 0x1804f5db0), though not into the Titan under us (0x180566280 retracts it).
    pilot.state.hook_targets = crate::pilotctl::hook_targets(
        enemies
            .iter()
            .filter(|(id, _)| *id != ride.target)
            .map(|(_, e)| e),
        None,
        None,
    );
    tf_sim::pilot::step_hook_only(&mut pilot.state, &settings.0, &world.0, dt);
    let Ok((_, mut e)) = enemies.get_mut(ride.target) else {
        return;
    };
    let titan_rot = Quat::from_rotation_z(e.state.yaw);
    let Ok(titan_tf) = tfs.get(ride.target).map(|(t, _)| *t) else {
        return;
    };
    let attach = hijack_attachment(&actors, &titan_tf, &e);
    let anchor_rot = attach.rotation;
    let spot = attach.translation + Vec3::Z * 40.0;
    let titan_vel = Vec3::from(e.state.vel.to_array());
    let mut detach: Option<Vec3> = None;
    let mut lift = false;
    if !e.alive() || !on_foot {
        detach = Some(titan_vel);
    }
    // Clip timing from the arms actor (the body's twins have the same lengths).
    let clip_len = |actors: &Query<&mut Actor>, name: &str| {
        arms.and_then(|(_, a)| {
            actors
                .get(a)
                .ok()
                .and_then(|ac| ac.clip(name).map(|c| ac.clips[c].duration))
        })
        .unwrap_or(1.2)
    };
    let (clip_name, looped): (String, bool) = match ride.phase {
        Phase::Entrance => (ride.entrance.clone(), false),
        Phase::Idle => (ride.chassis.idle_1p().to_string(), true),
        Phase::Hijack => (ride.chassis.hijack_1p().to_string(), false),
        Phase::Grenade => (ride.chassis.grenade_1p().to_string(), false),
    };
    let len = clip_len(&actors, &clip_name);
    let cycle = if looped {
        (ride.t / len.max(1e-3)).rem_euclid(1.0)
    } else {
        (ride.t / len.max(1e-3)).min(0.999)
    };
    ride.cycle = cycle;

    // Jump off (before the point of no return of a hijack).
    let can_jump = ride.since > JUMP_DEBOUNCE
        && matches!(ride.phase, Phase::Entrance | Phase::Idle)
        || (ride.phase == Phase::Hijack && cycle < ride.chassis.rip_events().no_return);
    if input.jump && can_jump && detach.is_none() {
        let yaw = input.yaw;
        let fwd = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
        let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
        let wish = fwd * input.forward + right * input.right;
        let dir = if wish.length() > 0.1 {
            wish.normalize()
        } else {
            -(titan_rot * Vec3::X)
        };
        detach = Some(titan_vel + dir * JUMP_OFF_SPEED + Vec3::Z * JUMP_OFF_UP);
        log::info!("rodeo: jumped off");
    }
    input.jump = false;
    // The hook landed somewhere: the rider lets go, thrown up by grapple_rodeoVerticalImpulse
    // (client.dll 0x1804f5750), and the pull takes over. Past the rip's point of no return it
    // just comes back.
    let mut grappled_off = false;
    if pilot.state.grapple.is_some() && detach.is_none() {
        if can_jump {
            detach = Some(titan_vel + Vec3::Z * settings.0.grapple.rodeo_impulse);
            grappled_off = true;
            log::info!("rodeo: grappled off");
        } else {
            pilot.state.release_grapple(&settings.0);
        }
    }

    match ride.phase {
        Phase::Entrance => {
            if ride.t >= len {
                ride.phase = Phase::Idle;
                ride.t = 0.0;
            }
        }
        Phase::Idle => {
            // Straight to work: the battery, or, already carrying one, a grenade down the
            // open hatch (ShouldThrowGrenadeInHatch).
            if ride.t >= 0.3 {
                let rip = !rodeo.battery;
                ride.phase = if rip { Phase::Hijack } else { Phase::Grenade };
                ride.t = 0.0;
                ride.done_hit = false;
                log::info!(
                    "rodeo: {}",
                    if rip {
                        "ripping the battery"
                    } else {
                        "grenade in the hatch"
                    }
                );
                // Another player's Titan keeps following their snapshots; its copy plays the
                // panel clip over them (targets.rs mirror_enemy) instead of being held.
                if rip
                    && !e.held
                    && e.remote.is_none()
                    && ride
                        .chassis
                        .hijack_titan()
                        .is_some_and(|c| actors.get(e.actor).is_ok_and(|a| a.clip(c).is_some()))
                {
                    e.held = true;
                    ride.holds = true;
                }
            }
        }
        Phase::Hijack | Phase::Grenade => {
            if e.remote.is_some() {
                e.override_anim = ride
                    .chassis
                    .hijack_titan()
                    .filter(|_| ride.phase == Phase::Hijack)
                    .map(|c| (c.to_string(), ride.t));
            }
            if ride.holds {
                if let (Some(c), Ok(mut a)) = (ride.chassis.hijack_titan(), actors.get_mut(e.actor))
                {
                    if let Some(clip) = a.clip(c) {
                        a.autoplay = false;
                        a.layers = vec![Layer {
                            clip,
                            cycle,
                            weight: 1.0,
                        }];
                    }
                }
            }
            let ev = if ride.phase == Phase::Hijack {
                ride.chassis.rip_events()
            } else {
                ride.chassis.grenade_events()
            };
            if cycle >= ev.show && ride.phase == Phase::Hijack {
                rodeo.hatch_down = Some(HatchDown {
                    target: ride.target,
                    rises: cycle >= ev.no_return,
                    lower: false,
                });
            }
            if cycle >= ev.hit && !ride.done_hit {
                ride.done_hit = true;
                // RodeoBatteryPackRemovalDamage: a segment, half of it from a grenade
                // (thrown only while carrying a battery); a doomed Titan dies.
                let dmg = if e.doomed() {
                    e.v.health + e.v.shield + 1.0
                } else if rodeo.battery {
                    SP_TITAN_SEGMENT_HEALTH / 2.0
                } else {
                    SP_TITAN_SEGMENT_HEALTH
                };
                e.damage_bypass(dmg);
                let hatch = e.pos + titan_rot * Vec3::new(0.0, 0.0, e.height * 0.85);
                if ride.phase == Phase::Hijack {
                    e.battery = false;
                    got_battery = true;
                    emit(
                        &mut commands,
                        Effect::Impact {
                            at: to_bevy(hatch),
                            normal: Vec3::Y,
                            scale: 3.0,
                            energy: true,
                        },
                    );
                    crate::audio::event(&mut commands, "UI_TitanBattery_Pilot_PickUp");
                    log::info!("rodeo: battery ripped, {dmg:.0} damage");
                } else {
                    emit(
                        &mut commands,
                        Effect::Explosion {
                            at: to_bevy(hatch),
                            scale: 1.0,
                        },
                    );
                    crate::audio::event_at(&mut commands, "Explo_FragGrenade_Impact_3P", hatch);
                    log::info!("rodeo: grenade went off in the hatch, {dmg:.0} damage");
                }
            }
            if ride.t >= len {
                // Thrown off (CalculateDirectionToThrowOffBatteryThief): backwards off the
                // Titan, 0.75 of the speed, or steered by the stick (forward 0.75..1 of it,
                // sideways up to 0.4), then ThrowRiderOff lifts you and adds the SP boost.
                let back = -(titan_rot * Vec3::X).truncate().normalize_or(Vec2::NEG_X);
                let right = Vec2::new(back.y, -back.x);
                let x = input.right.clamp(-1.0, 1.0) * 0.4;
                let y = 0.875 - input.forward.clamp(-1.0, 1.0) * 0.125;
                let dir = if x.abs() < 0.2 && y.abs() < 0.2 {
                    back * 0.75
                } else {
                    back * y + right * x
                };
                detach = Some(
                    titan_vel
                        + Vec3::new(dir.x, dir.y, 0.0) * RIP_PUSH_HORIZONTAL
                        + Vec3::Z * (RIP_PUSH_VERTICAL + SP_RODEO_BOOST),
                );
                lift = true;
            }
        }
    }
    // Anti-rodeo: the Titan pops its Electric Smoke on itself once you've been on for a while.
    if e.smoke_charges > 0 && ride.since >= ANTI_RODEO_SMOKE_DELAY && e.alive() {
        e.smoke_charges -= 1;
        crate::titankit::spawn_smoke(&mut commands, e.pos, Vec3::X, true);
        crate::audio::event_at(
            &mut commands,
            "Titan_Offhand_ElectricSmoke_Deploy_3P",
            e.pos,
        );
        log::info!("rodeo: the Titan released its Electric Smoke");
    }
    pilot.state.vel = SVec3::new(titan_vel.x, titan_vel.y, titan_vel.z);
    pilot.state.mode = PilotMove::Air;
    if got_battery {
        rodeo.battery = true;
    }

    // The first-person arms ride the attachment and play the rodeo clip; the view is their
    // camera bone (the entrance starts from wherever you were).
    let eye_off = Vec3::from(pilot.state.eye(&settings.0).to_array())
        - Vec3::from(pilot.state.pos.to_array());
    let mut view = (spot + Vec3::Z * 20.0, view_quat(e.state.yaw, 0.3));
    if let Some((anchor, actor_e)) = arms {
        if let Ok(mut a) = actors.get_mut(actor_e) {
            a.autoplay = false;
            a.aim = None;
            if let Some(c) = a.clip(&clip_name) {
                a.layers = vec![Layer {
                    clip: c,
                    cycle,
                    weight: 1.0,
                }];
            }
            // The clip's `jx_c_start` bone is its reference point (the hijack attachment,
            // useAnimatedRefAttachment): it sits up to 60 units from the clip's root and
            // walks with the root motion of the entrances. Place the arms so it stays on the
            // attachment, as fparms.rs does for the mounts: root = attach * s0 * start(t)^-1.
            let s0 = Quat::from_xyzw(0.5, 0.5, 0.5, 0.5);
            let (root_pos, root_rot) = match a.bone_model_transform("jx_c_start") {
                Some(st) => {
                    let frame =
                        Transform::from_translation(attach.translation).with_rotation(anchor_rot);
                    let root = frame.compute_affine()
                        * Transform::from_rotation(s0).compute_affine()
                        * st.compute_affine().inverse();
                    let (_, r, t) = root.to_scale_rotation_translation();
                    (t, r)
                }
                None => (attach.translation, anchor_rot),
            };
            if let Some(c) = a.bone_model_transform("jx_c_camera") {
                let rot = root_rot * c.rotation * attach_frame();
                view = (root_pos + root_rot * c.translation, rot);
                if std::env::var_os("TF_RODEO_TEST").is_some() {
                    log::debug!("rodeo cam {clip_name} cycle {cycle:.2}: bone {:.1} eye {:.0} fwd {:.2}; titan {:.0} yaw {:.0} attach {:.0} (fwd {:.2} up {:.2}) root {:.0}", c.translation, view.0, rot * Vec3::X, e.pos, e.state.yaw.to_degrees(), attach.translation, attach.rotation * Vec3::X, attach.rotation * Vec3::Z, root_pos);
                }
            }
            if let Ok((mut tf, mut vis)) = tfs.get_mut(anchor) {
                tf.rotation = root_rot;
                tf.translation = root_pos;
                vis.set_if_neq(if detach.is_some() {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                });
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
        if e.remote.is_some() {
            e.override_anim = None;
        }
        if lift {
            pilot.state.pos.z += RIP_PUSH_LIFT;
        }
        pilot.state.vel = SVec3::new(v.x, v.y, v.z);
        pilot.state.mode = PilotMove::Air;
        pilot.state.jumps = 1;
        // Thrown up off the hull by a grapple, the pull swings the rider back past the Titan:
        // not onto it again.
        rodeo.debounce = if grappled_off {
            GRAPPLE_OFF_DEBOUNCE
        } else {
            JUMP_DEBOUNCE
        };
        rodeo.cam = None;
        if let Some((anchor, _)) = arms {
            if let Ok((_, mut vis)) = tfs.get_mut(anchor) {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    } else {
        rodeo.ride = Some(ride);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HatchState {
    Up,
    Lowering,
    Down,
    Rising,
}

struct Hatch {
    actor: Option<Entity>,
    socket: Entity,
    chassis: Chassis,
    state: HatchState,
    /// Seconds into `hatch_rodeo_up`.
    t: f32,
}

/// The rodeo props: every enemy Titan's hatch battery, and the battery / grenade in the hand.
#[derive(Resource, Default)]
pub struct RodeoProps {
    /// By enemy entity.
    hatches: HashMap<Entity, Hatch>,
    /// (battery, grenade) actors under the arms' hand bones, once the arms exist.
    hand: Option<(Option<Entity>, Option<Entity>)>,
}

/// Spawn and drive the rodeo props (after `rodeo_update`): the hatch battery sits in each
/// enemy Titan's `RODEO_BATTERY` socket (`hatch_rodeo_up_idle`), sinks at the show event and
/// rises or snaps back when the ride ends; the hand battery shows from the hijack's show event
/// and the grenade from the grenade's, both until the rider is thrown off
/// (ClearRodeoAnimTempProps).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn rodeo_props(
    mut commands: Commands,
    time: Res<Time>,
    gd: Res<crate::gamedata::GameData>,
    mut assets: (
        ResMut<crate::convert::Cache>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    ),
    mut rodeo: ResMut<Rodeo>,
    mut props: ResMut<RodeoProps>,
    fp_arms: Option<Res<FpArms>>,
    kits: Res<EnemyKits>,
    enemies: Query<(Entity, &Enemy)>,
    mut actors: Query<&mut Actor>,
    mut vis: Query<&mut Visibility>,
    globals: Query<&GlobalTransform>,
    skins: Query<&bevy::mesh::skinning::SkinnedMesh>,
) {
    let (cache, meshes, images, materials, bindposes) = (
        &mut *assets.0,
        &mut *assets.1,
        &mut *assets.2,
        &mut *assets.3,
        &mut *assets.4,
    );
    let mut spawn = |commands: &mut Commands, parent: Entity, path: &str, seqs: &[&str]| {
        let spec = ActorSpec {
            path,
            sequences: seqs,
            grids: &[],
            body: &[],
        };
        match crate::actor::spawn_actor(
            commands, parent, &gd, &spec, cache, meshes, images, materials, bindposes,
        ) {
            Ok(s) => Some(s.entity),
            Err(e) => {
                log::warn!("rodeo prop {path}: {e:#}");
                None
            }
        }
    };

    // A hatch battery for every enemy Titan, at its socket.
    for (ent, e) in enemies.iter() {
        if props.hatches.contains_key(&ent) {
            continue;
        }
        let Some(kit) = kits.0.get(e.kit) else {
            continue;
        };
        if kit.infantry {
            continue;
        }
        let Ok(a) = actors.get(e.actor) else { continue };
        let Some(spine) = a.joint("def_c_spineC") else {
            continue;
        };
        let chassis = Chassis::of_model(&kit.model);
        // The hatch model's battery hangs down its -Z from the cap at the origin and rides up
        // and down Z (`up_idle` +1.4, `down_idle` -16.5), so the socket's Z goes up the spine
        // (the bone's Y): at the show event the hand battery (hanging down the hand's -Y)
        // then sits exactly where the socket one was.
        let socket = commands
            .spawn((
                Transform::from_translation(chassis.socket())
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                Visibility::default(),
            ))
            .id();
        commands.entity(spine).add_child(socket);
        let clips = [
            chassis.hatch_clip("up_idle"),
            chassis.hatch_clip("down_idle"),
            chassis.hatch_clip("down"),
            chassis.hatch_clip("up"),
        ];
        let refs: Vec<&str> = clips.iter().map(String::as_str).collect();
        let actor = spawn(&mut commands, socket, chassis.hatch_model(), &refs);
        props.hatches.insert(
            ent,
            Hatch {
                actor,
                socket,
                chassis,
                state: HatchState::Up,
                t: 0.0,
            },
        );
    }

    // The battery and the grenade in the arms' hands, hidden until their show events.
    if props.hand.is_none() {
        if let Some(arms) = fp_arms.as_deref() {
            if let Ok(a) = actors.get(arms.actor) {
                let hand = a.joint("ja_r_propHand");
                let gun = a.joint("ja_c_propGun");
                let battery = hand.and_then(|j| spawn(&mut commands, j, BATTERY_MODEL, &[]));
                let grenade = gun.and_then(|j| spawn(&mut commands, j, GRENADE_MODEL, &[]));
                for p in [battery, grenade].into_iter().flatten() {
                    commands.entity(p).insert(Visibility::Hidden);
                }
                // `R_HAND` has the bone's axes, but `PROPGUN` is turned like the camera and
                // ref attachments (forward = bone Z, left = bone X, up = bone Y), and a prop
                // parented to it takes the attachment's axes.
                if let Some(g) = grenade {
                    commands
                        .entity(g)
                        .insert(Transform::from_rotation(attach_frame()));
                }
                if hand.is_none() || gun.is_none() {
                    log::warn!("rodeo props: the arms have no ja_r_propHand / ja_c_propGun");
                }
                props.hand = Some((battery, grenade));
            }
        }
    }

    let riding = rodeo.ride.as_ref().map(|r| r.target);
    let dt = time.delta_secs();
    for (ent, h) in props.hatches.iter_mut() {
        let down = rodeo
            .hatch_down
            .filter(|d| d.target == *ent && riding == Some(*ent));
        match (h.state, down) {
            (HatchState::Down, None) => {
                let rises = rodeo
                    .hatch_down
                    .is_some_and(|d| d.target == *ent && d.rises);
                h.state = if rises {
                    HatchState::Rising
                } else {
                    HatchState::Up
                };
                h.t = 0.0;
            }
            (HatchState::Up | HatchState::Rising, Some(d)) => {
                h.state = if d.lower {
                    HatchState::Lowering
                } else {
                    HatchState::Down
                };
                h.t = 0.0;
            }
            (HatchState::Lowering, None) => {
                h.state = HatchState::Rising;
                h.t = 0.0;
            }
            _ => {}
        }
        // TF_HATCH_TEST=up|down: hold every hatch in a state (testing).
        static TEST: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        match TEST
            .get_or_init(|| std::env::var("TF_HATCH_TEST").ok())
            .as_deref()
        {
            Some("down") => h.state = HatchState::Down,
            Some("up") => h.state = HatchState::Up,
            _ => {}
        }
        let Some(actor) = h.actor else { continue };
        if log::log_enabled!(target: "tf_viewer::rodeo", log::Level::Debug)
            && riding == Some(*ent)
            && time.elapsed_secs() % 0.25 < dt
        {
            let hand = fp_arms
                .as_deref()
                .and_then(|f| actors.get(f.actor).ok())
                .and_then(|a| a.joint("ja_r_propHand"))
                .and_then(|j| globals.get(j).ok());
            let gun = fp_arms
                .as_deref()
                .and_then(|f| actors.get(f.actor).ok())
                .and_then(|a| a.joint("ja_c_propGun"))
                .and_then(|j| globals.get(j).ok());
            let sock = globals.get(h.socket).ok();
            // The hull's socket hole as the GPU skins it: the ajax hole centre sits at bind
            // (0.1, 223.0, -40.3) (vertsnear), 100% on def_c_spineC.
            let hole = enemies
                .get(*ent)
                .ok()
                .and_then(|(_, e)| actors.get(e.actor).ok())
                .and_then(|a| {
                    let bi = a
                        .bone_names
                        .iter()
                        .position(|b| b.eq_ignore_ascii_case("def_c_spineC"))?;
                    let skin = skins
                        .iter()
                        .find(|s| s.joints.first() == a.joints.first())?;
                    let inv = bindposes.get(&skin.inverse_bindposes)?;
                    let jg = globals.get(a.joints[bi]).ok()?;
                    let v = inv.get(bi)?.transform_point3(Vec3::new(0.1, 223.0, -40.3));
                    Some(jg.affine().transform_point3(v))
                });
            if let (Some(hand), Some(gun), Some(sock)) = (hand, gun, sock) {
                if let Some(hole) = hole {
                    log::debug!(
                        "hatch {:?}: skinned hole in the socket frame {:.1}",
                        ent,
                        sock.affine().inverse().transform_point3(hole)
                    );
                }
                // The hand battery hangs down the hand's -Y, the socket one down the socket's
                // -Z: at the hand-off they should coincide (distance 0, dot 1). The grenade
                // sits on the propGun bone, over the socket at its show.
                // The socket sits under the Titan's unit-scaled root, so its local frame is
                // already in game units.
                let in_socket = |p: Vec3| sock.affine().inverse().transform_point3(p);
                log::debug!(
                    "hatch {:?}: hand-socket {:.1} units, axes dot {:.2}, gun-socket {:.1} (socket frame hand {:.1} gun {:.1}), state {}",
                    ent,
                    (hand.translation() - sock.translation()).length() / crate::player::UNIT,
                    (hand.rotation() * Vec3::NEG_Y).dot(sock.rotation() * Vec3::NEG_Z),
                    (gun.translation() - sock.translation()).length() / crate::player::UNIT,
                    in_socket(hand.translation()),
                    in_socket(gun.translation()),
                    match h.state {
                        HatchState::Up => "up",
                        HatchState::Lowering => "lowering",
                        HatchState::Down => "down",
                        HatchState::Rising => "rising",
                    }
                );
            }
        }
        let Ok(mut a) = actors.get_mut(actor) else {
            continue;
        };
        a.autoplay = false;
        let (name, cycle) = match h.state {
            HatchState::Up => (h.chassis.hatch_clip("up_idle"), 0.0),
            HatchState::Down => (h.chassis.hatch_clip("down_idle"), 0.0),
            HatchState::Lowering => {
                let name = h.chassis.hatch_clip("down");
                let len = a
                    .clip(&name)
                    .map(|c| a.clips[c].duration)
                    .unwrap_or(0.667)
                    .max(1e-3);
                h.t += dt;
                if h.t >= len {
                    h.state = HatchState::Down;
                    (h.chassis.hatch_clip("down_idle"), 0.0)
                } else {
                    (name, h.t / len)
                }
            }
            HatchState::Rising => {
                let name = h.chassis.hatch_clip("up");
                let len = a
                    .clip(&name)
                    .map(|c| a.clips[c].duration)
                    .unwrap_or(0.667)
                    .max(1e-3);
                h.t += dt;
                if h.t >= len {
                    h.state = HatchState::Up;
                    (h.chassis.hatch_clip("up_idle"), 0.0)
                } else {
                    (name, h.t / len)
                }
            }
        };
        if let Some(c) = a.clip(&name) {
            a.layers = vec![Layer {
                clip: c,
                cycle,
                weight: 1.0,
            }];
        }
    }
    if riding.is_none() {
        rodeo.hatch_down = None;
    }

    if let Some((battery, grenade)) = props.hand {
        let (show_battery, show_grenade) = match rodeo.ride.as_ref() {
            Some(r) => (
                r.phase == Phase::Hijack && r.cycle >= r.chassis.rip_events().show,
                r.phase == Phase::Grenade && r.cycle >= r.chassis.grenade_events().show,
            ),
            None => (false, false),
        };
        for (p, show) in [(battery, show_battery), (grenade, show_grenade)] {
            if let Some(mut v) = p.and_then(|p| vis.get_mut(p).ok()) {
                v.set_if_neq(if show {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}

/// While riding, the camera is the arms' camera bone (after `pilot_camera`).
pub fn rodeo_camera(
    rodeo: Res<Rodeo>,
    mut cams: Query<&mut Transform, With<crate::player::MainCamera>>,
) {
    let Some((eye, rot)) = rodeo.cam else { return };
    if let Ok(mut cam) = cams.single_mut() {
        *cam = Transform::from_translation(to_bevy(eye)).looking_to(
            to_bevy(rot * Vec3::X).normalize(),
            to_bevy(rot * Vec3::Z).normalize(),
        );
    }
}

/// HUD: the battery on the Pilot's back.
#[derive(Component)]
pub struct BatteryHud;

pub fn spawn_battery_hud(mut commands: Commands, ui: Res<UiAssets>) {
    let vh = Val::Vh;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(21.0),
                bottom: vh(4.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: vh(0.4),
                ..default()
            },
            Visibility::Hidden,
            BatteryHud,
        ))
        .id();
    let frame = commands
        .spawn((
            Node {
                width: vh(7.0),
                height: vh(7.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
        ))
        .id();
    let icon = commands
        .spawn((
            Node::default(),
            ui.image("rui/menu/boosts/boost_icon_battery")
                .unwrap_or_default(),
            crate::hud::FitIcon,
        ))
        .id();
    commands.entity(frame).add_child(icon);
    let label = commands
        .spawn((
            Text::new("BATTERY"),
            TextFont {
                font: ui.font.clone(),
                ..default()
            },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            crate::ui::VhText(1.0),
        ))
        .id();
    commands.entity(root).add_children(&[frame, label]);
}

pub fn battery_hud(
    rodeo: Res<Rodeo>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    game: Res<crate::game::Game>,
    hidden: Res<crate::hud::HudHidden>,
    mut huds: Query<&mut Visibility, With<BatteryHud>>,
) {
    let show = rodeo.battery
        && *control == Control::Pilot
        && *mode != CameraMode::Free
        && game.in_play()
        && !hidden.0;
    for mut v in &mut huds {
        v.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// GetRodeoPackage's climb-on sounds by entrance (sh_rodeo_titan.gnut): `interior` is the
/// cockpitSound the ridden Titan's player hears, else the worldSound everyone else hears.
pub fn climb_sound(chassis: &str, side: &str, height: &str, interior: bool) -> String {
    let how = match (side, height) {
        ("front", "_lower") => "ClimbOn_Front",
        ("front", _) => "LandOn_Above",
        ("back", "_lower") => "ClimbOn_Back",
        ("back", "_mid") => "ClimbOn_Back_Mid",
        ("back", _) => "LandOn_Back",
        ("left", _) => "ClimbOn_Left",
        _ => "ClimbOn_Right",
    };
    let place = if interior { "Interior" } else { "Exterior" };
    format!("Rodeo_{chassis}_Rodeo_{how}_{place}")
}
