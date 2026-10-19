//! The Pilot's tactical ability (Q) and ordnance (G), from the game's ability and grenade
//! scripts (MP_BASE values; cooldowns are `ammo_per_shot / regen_ammo_refill_rate`, charges
//! `ammo_clip_size / ammo_per_shot`).
//!
//! Tacticals:
//! - Stim (`mp_ability_heal`): a speed boost (`STIM_EFFECT_SEVERITY` 0.4) and faster health
//!   regeneration (`ABILITY_STIM_REGEN_MOD` 2) for its `fire_duration`.
//! - Cloak (`mp_ability_cloak`): enemies lose sight of you beyond close range.
//! - Phase Shift (`mp_ability_shifter`): untouchable and unseen for its `fire_duration`.
//! - Grapple (`mp_ability_grapple`): hooks a surface within `grapple_maxLength` and reels you in
//!   (tf-sim's pilot movement); `grapple_power_required` of the power that regenerates at
//!   `grapple_power_regen_rate` after `grapple_power_regen_delay` (pilot_base.set).
//! - Pulse Blade (`mp_weapon_grenade_sonar`): thrown at `projectile_launch_speed`, sticks, and
//!   reveals enemies within `SONAR_GRENADE_RADIUS` for `SONAR_GRENADE_PULSE_DURATION`.
//! - Holo Pilot (`mp_ability_holopilot`): a decoy that runs ahead for `DECOY_DURATION`; enemies
//!   shoot at it when it's closer than you, or when they can't see you.
//! - A-Wall (`mp_weapon_deployable_cover`): an arc shield (`DEPLOYABLE_SHIELD_RADIUS`, `_HEIGHT`,
//!   `_FOV`, `_HEALTH`) that stops enemy rounds for `DEPLOYABLE_SHIELD_DURATION`.
//!
//! Ordnance:
//! - Frag Grenade: bounces with the script's bounce fractions, explodes after
//!   `grenade_fuse_time`.
//! - Arc Grenade (`mp_weapon_grenade_emp`): explodes on impact and stuns enemies for the EMP
//!   screen-effect durations (`EMP_GRENADE_PILOT_SCREEN_EFFECTS_DURATION_MIN/MAX`).
//! - Firestar (`mp_weapon_thermite_grenade`): sticks and burns for `THERMITE_GRENADE_BURN_TIME`,
//!   dealing its explosion damage every 0.2 s (the first burst x1.2, x2.5 against Titans).
//! - Gravity Star (`mp_weapon_grenade_gravity`): sticks, pops up `POP_HEIGHT` after `POP_DELAY`,
//!   pulls nearby grunts (radius `PULL_RANGE * 2`, accel 2000, 400 u/s) for `PULL_DELAY`, then
//!   explodes.
//! - Electric Smoke (`mp_weapon_grenade_electric_smoke`): a cloud for the smokescreen's 5 s that
//!   deals its explosion damage per second (0.1 s ticks) after a 1 s delay.
//! - Satchel (`mp_weapon_satchel`): thrown at `SATCHEL_THROW_POWER`, sticks, and goes off when
//!   you press G again.

use crate::convert::Cache;
use crate::gamedata::GameData;
use crate::particles::{emit, Effect};
use crate::pilotctl::{Control, PilotHealth, PlayerPilot, PILOT_HEALTH};
use crate::player::{to_bevy, Collision, MainCamera, PlayerInput};
use crate::targets::Enemy;
use crate::ui::UiAssets;
use bevy::prelude::*;
use tf_assets::settings::PlayerSettings;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::{GrappleEvent, PilotMove};

/// Stim's speed_boost status effect: STIM_EFFECT_SEVERITY, held for the stim's duration plus
/// 0.5 s and eased out over the last 0.25 s (StatusEffect_AddTimed in sh_stim.gnut).
const STIM_SEVERITY: f32 = 0.4;
const STIM_BOOST_EXTRA: f32 = 0.5;
const STIM_BOOST_EASE_OUT: f32 = 0.25;
/// grapple_gracePeriod: seconds of pull off the world before any power is drained.
const GRAPPLE_GRACE: f32 = 0.25;

/// The hook's direction when fired (client.dll 0x1802cd1e0): where the straight shot would hit
/// nothing within reach, aim at a target in the cone instead, at its feet beside its hull
/// (radius + 4 to the side the aim is on, then the other side), if that is within acos(0.95)
/// of the aim and the shot that way reaches it.
fn grapple_snap(
    eye: SVec3,
    dir: SVec3,
    reach: f32,
    world: &tf_sim::collision::CollisionWorld,
    targets: &[tf_sim::pilot::HookTarget],
) -> SVec3 {
    if world.raycast(eye, dir, reach).is_some() {
        return dir;
    }
    let best = targets
        .iter()
        .filter_map(|t| {
            let to = t.pos + SVec3::Z * (t.height * 0.5) - eye;
            let d = to.length();
            (d > 1.0 && d <= reach).then(|| (t, to.dot(dir) / d))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let Some((t, _)) = best else { return dir };
    let right = SVec3::new(t.yaw.sin(), -t.yaw.cos(), 0.0);
    let mut sign = if dir.dot(right) >= 0.0 { -1.0 } else { 1.0 };
    for _ in 0..2 {
        let point = t.pos + right * ((t.radius + 4.0) * sign);
        let to = point - eye;
        let d = to.length().max(1e-3);
        let nd = to / d;
        if nd.dot(dir) >= 0.95 {
            let hit = world
                .raycast(eye, nd, reach)
                .map(|h| h.t)
                .into_iter()
                .chain(t.hit(eye, nd, reach))
                .fold(None, |b: Option<f32>, x| Some(b.map_or(x, |b| b.min(x))));
            if hit.is_some_and(|h| d - 15.0 < h) {
                log::info!("grapple aimed at a target {d:.0} away");
                return nd;
            }
        }
        sign = -sign;
    }
    dir
}
/// Health regenerated per second while stimmed, on top of normal regen (ABILITY_STIM_REGEN_MOD).
const STIM_REGEN: f32 = 40.0;
/// Cloaked Pilots are only seen this close (game units).
pub const CLOAK_SEEN_WITHIN: f32 = 400.0;
const GRENADE_GRAVITY: f32 = 750.0;
/// DEPLOYABLE_THROW_POWER (sh_deployable.gnut) and the A-Wall's `projectile_gravity_scale`.
const DEPLOYABLE_THROW_POWER: f32 = 500.0;
const AWALL_GRAVITY_SCALE: f32 = 3.0;
const GRENADE_RADIUS: f32 = 4.0;
/// SONAR_GRENADE_RADIUS / SONAR_GRENADE_PULSE_DURATION (sh_consts.gnut).
const SONAR_RADIUS: f32 = 1250.0;
const SONAR_DURATION: f32 = 6.0;
/// DECOY_DURATION (mp_ability_holopilot.nut).
const DECOY_DURATION: f32 = 10.0;
/// The decoy's end: server.dll's Decoy_Dissolve (0x1801c4aa0) dissolves the decoy and the gun
/// models it carries and removes it 3 s later; client.dll's C_EntityDissolve (0x180116b90)
/// plays the `dissolve` system and "Death.Dissolve" on it. The model's own burn-out (the
/// dissolve render effect) isn't drawn: it fades out over the second second instead, as
/// Source's dissolve hides the model between 1.9 and 2 s.
const DECOY_DISSOLVE_TOTAL: f32 = 3.0;
const DECOY_DISSOLVE_FADE: (f32, f32) = (1.0, 2.0);
/// DEPLOYABLE_SHIELD_* (mp_weapon_deployable_cover.nut).
const SHIELD_DURATION: f32 = 15.0;
const SHIELD_HEALTH: f32 = 850.0;
const SHIELD_RADIUS: f32 = 84.0;
const SHIELD_HEIGHT: f32 = 89.0;
const SHIELD_FOV: f32 = 150.0;
/// THERMITE_GRENADE_BURN_TIME and the burn's tick (mp_weapon_thermite_grenade.nut).
const THERMITE_BURN_TIME: f32 = 6.0;
const THERMITE_TICK: f32 = 0.2;
/// Gravity Star (mp_weapon_grenade_gravity.nut).
pub const GRAV_POP_DELAY: f32 = 0.8;
pub const GRAV_PULL_DELAY: f32 = 2.0;
const GRAV_POP_HEIGHT: f32 = 60.0;
pub const GRAV_PULL_RADIUS: f32 = 300.0;
pub const GRAV_PULL_ACCEL: f32 = 2000.0;
pub const GRAV_PULL_SPEED: f32 = 400.0;
/// Electric smokescreen lifetime, damage delay and tick (smokescreen.nut and the grenade).
const SMOKE_LIFETIME: f32 = 5.0;
const SMOKE_DELAY: f32 = 1.0;
const SMOKE_TICK: f32 = 0.1;
/// SATCHEL_THROW_POWER (mp_weapon_satchel.nut).
const SATCHEL_THROW: f32 = 620.0;
/// EMP_GRENADE_PILOT_SCREEN_EFFECTS_DURATION_MIN/MAX, used as the stun on AI.
const ARC_STUN: (f32, f32) = (1.5, 2.5);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tactical {
    Stim,
    Cloak,
    PhaseShift,
    Grapple,
    PulseBlade,
    HoloPilot,
    AWall,
}

/// (kind, weapon script, HUD icon, label, activation sound event)
pub const TACTICALS: &[(Tactical, &str, &str, &str, &str)] = &[
    (
        Tactical::Stim,
        "mp_ability_heal",
        "rui/pilot_loadout/tactical/pilot_tactical_stim",
        "STIM",
        "pilot_stimpack_activate_1P",
    ),
    (
        Tactical::Cloak,
        "mp_ability_cloak",
        "rui/pilot_loadout/tactical/pilot_tactical_cloak",
        "CLOAK",
        "cloak_on_1P",
    ),
    (
        Tactical::PhaseShift,
        "mp_ability_shifter",
        "rui/pilot_loadout/tactical/pilot_tactical_phase_shift",
        "PHASE SHIFT",
        "Pilot_PhaseShift_Activate_1P",
    ),
    (
        Tactical::Grapple,
        "mp_ability_grapple",
        "rui/pilot_loadout/tactical/pilot_tactical_grapple",
        "GRAPPLE",
        "pilot_grapple_fire",
    ),
    (
        Tactical::PulseBlade,
        "mp_weapon_grenade_sonar",
        "rui/pilot_loadout/tactical/pilot_tactical_pulse_blade",
        "PULSE BLADE",
        "Pilot_PulseBlade_Throw_1P",
    ),
    (
        Tactical::HoloPilot,
        "mp_ability_holopilot",
        "rui/pilot_loadout/tactical/pilot_tactical_holo_pilot",
        "HOLO PILOT",
        "holopilot_deploy_1p",
    ),
    (
        Tactical::AWall,
        "mp_weapon_deployable_cover",
        "rui/pilot_loadout/tactical/pilot_tactical_hardcover",
        "A-WALL",
        "Pilot_Hardcover_Toss_1P",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ordnance {
    Frag,
    Arc,
    Firestar,
    GravityStar,
    ElectricSmoke,
    Satchel,
}

/// (kind, weapon script, HUD icon, label, throw sound, explosion sound)
pub const ORDNANCE: &[(Ordnance, &str, &str, &str, &str, &str)] = &[
    (
        Ordnance::Frag,
        "mp_weapon_frag_grenade",
        "rui/pilot_loadout/ordnance/frag",
        "FRAG GRENADE",
        "Weapon_FragGrenade_Throw",
        "Explo_FragGrenade_Impact_3P",
    ),
    (
        Ordnance::Arc,
        "mp_weapon_grenade_emp",
        "rui/pilot_loadout/ordnance/arc_grenade",
        "ARC GRENADE",
        "weapon_empgrenade_throw",
        "explo_proximityemp_impact_3p",
    ),
    (
        Ordnance::Firestar,
        "mp_weapon_thermite_grenade",
        "rui/pilot_loadout/ordnance/firestar",
        "FIRESTAR",
        "weapon_firestar_throw_1p",
        "explo_firestar_impact",
    ),
    (
        Ordnance::GravityStar,
        "mp_weapon_grenade_gravity",
        "rui/pilot_loadout/ordnance/gravity_grenade",
        "GRAVITY STAR",
        "weapon_gravitystar_throw_1p",
        "gravitystar_explo_3p",
    ),
    (
        Ordnance::ElectricSmoke,
        "mp_weapon_grenade_electric_smoke",
        "rui/pilot_loadout/ordnance/electric_smoke",
        "ELECTRIC SMOKE",
        "weapon_electric_smoke_throw_1p",
        "explo_electric_smoke_impact",
    ),
    (
        Ordnance::Satchel,
        "mp_weapon_satchel",
        "rui/pilot_loadout/ordnance/satchel",
        "SATCHEL",
        "weapon_r1_satchel_throw",
        "explo_satchel_impact_3p",
    ),
];

pub struct TacticalDef {
    pub duration: f32,
    /// Seconds per charge.
    pub cooldown: f32,
    pub charges: u32,
    /// Throw speed (Pulse Blade, A-Wall) or grapple reach.
    pub speed: f32,
    /// Grapple: seconds from the pull starting to the power regenerating
    /// (grapple_power_regen_delay).
    pub regen_delay: f32,
    /// Grapple: power drained per second of pull off the world after grapple_gracePeriod
    /// (grapple_power_use_rate), and the vertical reach (grapple_maxLengthVert).
    pub use_rate: f32,
    pub max_len_vert: f32,
    /// Thrown tacticals (Pulse Blade, A-Wall): `viewmodel`, `toss_pullout_time`, `toss_time`.
    pub viewmodel: String,
    pub pullout: f32,
    pub toss: f32,
    /// Thrown tacticals: `projectilemodel` and `projectile_trail_effect_0`.
    pub model: String,
    pub trail: String,
}

pub struct OrdnanceDef {
    pub damage: f32,
    pub damage_heavy: f32,
    /// Direct-hit damage (damage_near_value / _titanarmor).
    pub impact: f32,
    pub impact_heavy: f32,
    pub inner: f32,
    pub radius: f32,
    pub fuse: f32,
    pub speed: f32,
    pub pitch_offset: f32,
    pub cooldown: f32,
    pub charges: u32,
    pub bounce_shallow: f32,
    pub bounce_sharp: f32,
    /// The throw: `viewmodel`, `toss_pullout_time` and `toss_time`.
    pub viewmodel: String,
    pub pullout: f32,
    pub toss: f32,
    /// `projectilemodel` and `projectile_trail_effect_0`.
    pub model: String,
    pub trail: String,
}

/// An ordnance throw in progress: pull it out, hold it while the button is held (the fuse
/// burning for fused kinds), throw it on release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThrowPhase {
    Pullout,
    Hold,
    Toss,
    /// The gun coming back up after the throw.
    Raise,
}

#[derive(Resource, Default)]
pub struct OrdnanceThrow {
    pub phase: Option<(ThrowPhase, f32)>,
    /// Seconds held (cooking) before the toss.
    cooked: f32,
    /// A thrown tactical (Pulse Blade, A-Wall) rather than the ordnance.
    pub tactical: bool,
}

impl OrdnanceThrow {
    /// The gun is out of the hands.
    pub fn busy(&self) -> bool {
        self.phase.is_some_and(|(p, _)| p != ThrowPhase::Raise)
    }
}

/// How long the gun takes to come back up after a throw (the guns' `raise_seq`, 0.33 s).
pub const THROW_RAISE_SECS: f32 = 0.33;

#[derive(Resource)]
pub struct PilotAbilityDefs {
    pub tactical: Vec<TacticalDef>,
    pub ordnance: Vec<OrdnanceDef>,
}

/// A key from the script's MP_BASE block, else its top level.
fn mp(s: &PlayerSettings, k: &str, default: f32) -> f32 {
    s.get(&format!("{}{k}", crate::gamedata::base()))
        .or_else(|| s.get(&format!(".{k}")))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn cooldown(s: &PlayerSettings, default: f32) -> f32 {
    let per_shot = mp(s, "ammo_per_shot", 0.0);
    let rate = mp(s, "regen_ammo_refill_rate", 0.0);
    if per_shot > 0.0 && rate > 0.0 {
        per_shot / rate
    } else {
        default
    }
}

fn charges(s: &PlayerSettings) -> u32 {
    let per_shot = mp(s, "ammo_per_shot", 1.0).max(1.0);
    ((mp(s, "ammo_clip_size", per_shot) / per_shot) as u32).max(1)
}

impl PilotAbilityDefs {
    pub fn load(read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let pilot = PlayerSettings::load(
            "scripts/players/mp/pilot_base.set",
            crate::gamedata::sp(),
            read,
        )
        .unwrap_or_default();
        // With the fitted kits' mods (pas_power_cell, pas_ordnance_pack).
        let mut load = |w: &str| {
            crate::kits::apply(
                PlayerSettings::load(
                    &format!("scripts/weapons/{w}.txt"),
                    crate::gamedata::sp(),
                    read,
                )
                .unwrap_or_default(),
            )
        };
        let tactical: Vec<TacticalDef> = TACTICALS
            .iter()
            .map(|t| {
                let s = load(t.1);
                let thrown = matches!(t.0, Tactical::PulseBlade | Tactical::AWall);
                let mut d = TacticalDef {
                    duration: mp(&s, "fire_duration", 3.0),
                    cooldown: cooldown(&s, 15.0),
                    charges: charges(&s),
                    speed: mp(&s, "projectile_launch_speed", 1100.0),
                    regen_delay: 0.0,
                    use_rate: 0.0,
                    max_len_vert: 0.0,
                    viewmodel: if thrown {
                        s.get(".viewmodel").unwrap_or("").trim().to_string()
                    } else {
                        String::new()
                    },
                    pullout: mp(&s, "toss_pullout_time", 0.2),
                    toss: mp(&s, "toss_time", 0.3),
                    model: s.get(".projectilemodel").unwrap_or("").trim().to_string(),
                    trail: s
                        .get(".projectile_trail_effect_0")
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                };
                match t.0 {
                    Tactical::Grapple => {
                        // Grapple power, not ammo: each pull takes grapple_power_required of 100
                        // (shown as charges of that size).
                        let need = mp(&s, "grapple_power_required", 50.0).max(1.0);
                        let rate = pilot.f32("global.grapple_power_regen_rate", 3.0).max(0.1);
                        d.charges = (100.0 / need) as u32;
                        d.cooldown = need / rate;
                        d.regen_delay = pilot.f32("global.grapple_power_regen_delay", 3.0);
                        d.duration = 0.0;
                        d.speed = mp(&s, "grapple_maxLength", 1100.0);
                        d.max_len_vert = mp(&s, "grapple_maxLengthVert", d.speed);
                        d.use_rate = mp(&s, "grapple_power_use_rate", 50.0);
                    }
                    Tactical::PulseBlade => d.duration = SONAR_DURATION,
                    Tactical::HoloPilot => d.duration = DECOY_DURATION,
                    Tactical::AWall => d.duration = SHIELD_DURATION,
                    _ => {}
                }
                d
            })
            .collect();
        let ordnance: Vec<OrdnanceDef> = ORDNANCE
            .iter()
            .map(|o| {
                let s = load(o.1);
                let mut d = OrdnanceDef {
                    damage: mp(&s, "explosion_damage", 0.0),
                    damage_heavy: mp(&s, "explosion_damage_heavy_armor", 0.0),
                    impact: mp(&s, "damage_near_value", 0.0),
                    impact_heavy: mp(&s, "damage_near_value_titanarmor", 0.0),
                    inner: mp(&s, "explosion_inner_radius", 0.0),
                    radius: mp(&s, "explosionradius", 200.0),
                    fuse: mp(&s, "grenade_fuse_time", 0.0),
                    speed: mp(&s, "projectile_launch_speed", 1100.0),
                    pitch_offset: mp(&s, "projectile_launch_pitch_offset", 8.0),
                    cooldown: cooldown(&s, 12.0),
                    charges: charges(&s),
                    bounce_shallow: mp(&s, "grenade_bounce_vel_frac_shallow", 0.5),
                    bounce_sharp: mp(&s, "grenade_bounce_vel_frac_sharp", 0.3),
                    viewmodel: s.get(".viewmodel").unwrap_or("").trim().to_string(),
                    pullout: mp(&s, "toss_pullout_time", 0.35),
                    toss: mp(&s, "toss_time", 0.33),
                    model: s.get(".projectilemodel").unwrap_or("").trim().to_string(),
                    trail: s
                        .get(".projectile_trail_effect_0")
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                };
                if o.0 == Ordnance::Satchel {
                    d.speed = SATCHEL_THROW;
                }
                d
            })
            .collect();
        let tacs: Vec<String> = TACTICALS
            .iter()
            .zip(tactical.iter())
            .map(|(t, d)| {
                format!(
                    "{:?} {:.1}s x{} /{:.1}s",
                    t.0, d.duration, d.charges, d.cooldown
                )
            })
            .collect();
        let ords: Vec<String> = ORDNANCE
            .iter()
            .zip(ordnance.iter())
            .map(|(o, d)| {
                format!(
                    "{:?} {:.0}/{:.0} r{:.0} x{} /{:.1}s",
                    o.0, d.damage, d.damage_heavy, d.radius, d.charges, d.cooldown
                )
            })
            .collect();
        log::info!("pilot tacticals: {}", tacs.join(", "));
        log::info!("pilot ordnance: {}", ords.join(", "));
        Self { tactical, ordnance }
    }
}

/// The Holo Pilot decoy (server.dll CPlayerDecoy): a copy of the player that carries on with
/// what the player was doing when it was made, then runs straight, moved by its animation.
#[derive(Clone, Copy)]
pub struct Decoy {
    /// Feet position.
    pub pos: Vec3,
    /// Facing (the player's view yaw, flat).
    pub dir: Vec3,
    /// Seconds left (SetTimeout DECOY_DURATION).
    pub time: f32,
    pub state: DecoyState,
    /// Velocity while airborne.
    pub vel: Vec3,
    /// Created in the air: jump this fast up next tick (the decoy's double jump).
    pub jump: Option<f32>,
    /// It came out of a slide (lands sliding; a ledge launches it at slide speed).
    pub was_sliding: bool,
    /// Was strafing before going airborne (lands strafing).
    pub strafe_right: Option<bool>,
}

/// CPlayerDecoy's states (server.dll 0x1802a4360; WALK is never set).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DecoyState {
    Idle,
    CrouchIdle,
    CrouchWalk,
    CrouchStrafe {
        right: bool,
    },
    Run,
    Strafe {
        right: bool,
    },
    Sprint,
    /// Seconds of slide left.
    Slide {
        left: f32,
    },
    Air,
    /// Along a wall with this outward normal, at this speed.
    WallRun {
        normal: Vec3,
        speed: f32,
    },
    WallHang,
}

/// The stick push below which a standing decoy idles (`CreatePlayerDecoy( 0.65 )`).
const DECOY_STICK_TO_RUN: f32 = 0.65;
/// A slide lasts 2 s, slowing over the last 1.25 s (server.dll 0x1801c42cb).
const DECOY_SLIDE_TIME: f32 = 2.0;
const DECOY_SLIDE_SLOWDOWN: f32 = 1.25;
/// Wallrun speed ramps up at 750/s to 350 (server.dll 0x1801c8d44).
const DECOY_WALLRUN_ACCEL: f32 = 750.0;
const DECOY_WALLRUN_MAX: f32 = 350.0;
/// Half the hull width, and the ground-state step height.
const DECOY_HULL: f32 = 16.0;
const DECOY_STEP: f32 = 18.0;

impl Decoy {
    /// CreatePlayerDecoy (server.dll 0x1801c40d0): copy the player's position and view yaw and
    /// pick the state from what the player is doing.
    pub fn from_player(
        s: &tf_sim::pilot::PilotState,
        p: &tf_sim::pilot::PilotParams,
        forward: f32,
        right: f32,
    ) -> Self {
        let mut dir = Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0);
        let vel = Vec3::from(s.vel.to_array());
        let flat = Vec3::new(vel.x, vel.y, 0.0);
        let stick = (forward + right.abs()).clamp(0.0, 1.0);
        let strafing = right.abs() > forward.abs();
        let mut d = Decoy {
            pos: Vec3::from(s.pos.to_array()),
            dir,
            time: DECOY_DURATION,
            state: DecoyState::Idle,
            vel: Vec3::ZERO,
            jump: None,
            was_sliding: false,
            strafe_right: None,
        };
        d.state = match s.mode {
            PilotMove::Slide => {
                d.was_sliding = true;
                DecoyState::Slide {
                    left: DECOY_SLIDE_TIME,
                }
            }
            PilotMove::WallHang { .. } => DecoyState::WallHang,
            PilotMove::WallRun { .. } if flat.length() <= p.walk_speed => DecoyState::WallHang,
            PilotMove::WallRun { normal, .. } => {
                // Facing along the player's horizontal velocity.
                dir = flat.normalize_or(dir);
                d.dir = dir;
                DecoyState::WallRun {
                    normal: Vec3::from(normal.to_array()),
                    speed: 0.0,
                }
            }
            PilotMove::Air | PilotMove::Mantle { .. } => {
                // The horizontal speed carries on along the view yaw; a superjump-style jump
                // next tick (server.dll 0x1801c460e).
                let g = p.gravity;
                let mut h = p.double_jump_height;
                let mut vz = vel.z;
                if h > h * p.double_jump_min_frac + vz.max(0.0).powi(2) / (2.0 * g) {
                    vz = 0.0;
                } else {
                    h *= p.double_jump_min_frac;
                }
                d.vel = dir * flat.length();
                d.jump = Some((2.0 * g * h).sqrt() + vz);
                DecoyState::Air
            }
            PilotMove::Ground if s.crouched => {
                if strafing {
                    DecoyState::CrouchStrafe { right: right > 0.0 }
                } else if stick >= DECOY_STICK_TO_RUN {
                    DecoyState::CrouchWalk
                } else {
                    DecoyState::CrouchIdle
                }
            }
            PilotMove::Ground if s.sprinting => DecoyState::Sprint,
            PilotMove::Ground if strafing => DecoyState::Strafe { right: right > 0.0 },
            PilotMove::Ground if stick >= DECOY_STICK_TO_RUN => DecoyState::Run,
            PilotMove::Ground => DecoyState::Idle,
        };
        d
    }

    /// The clip a state plays (activity, with the weapon's "pistol" modifier where the model has
    /// one), as sequence names.
    fn clip(&self, pistol: bool) -> &'static str {
        let p =
            |rifle: &'static str, pistol_seq: &'static str| if pistol { pistol_seq } else { rifle };
        match self.state {
            DecoyState::Idle => p("CQB_Idle_MP", "CQB_Idle_MP_Pistol"),
            DecoyState::CrouchIdle => p("CQB_CrouchIdle_MP", "CrouchIdle_Pistol_MP"),
            DecoyState::CrouchWalk => p("CrouchRun_forward_mp", "CrouchRun_pistol_forward_mp"),
            DecoyState::CrouchStrafe { right } => {
                if right {
                    "CrouchRUN_mp_right"
                } else {
                    "CrouchRUN_mp_left"
                }
            }
            DecoyState::Run => p("Run_forward_mp", "Run_pistol_forward_mp"),
            DecoyState::Strafe { right } => match (right, pistol) {
                (true, false) => "Run_mp_right",
                (false, false) => "Run_mp_left",
                (true, true) => "Run_mp_right_pistol",
                (false, true) => "Run_mp_left_pistol",
            },
            DecoyState::Sprint => p("Sprint_mp_forward", "Sprint_mp_forward_Pistol"),
            DecoyState::Slide { .. } => p("Slide_forward_mp", "Slide_Pistol_forward_mp"),
            DecoyState::Air if self.was_sliding => {
                p("MP_Pt_Slide_Float", "MP_Pt_Pistol_Slide_Float")
            }
            DecoyState::Air => "Jump_float_MP",
            DecoyState::WallRun { normal, .. } => {
                // ACT_MP_WALLRUN_RIGHT when dot(right, normal) >= 0, i.e. the wall is on the
                // left: the clips are named for the way the body leans (a_pt_wallrun_right
                // has the feet to the left of the head).
                let right_dir = Vec3::new(self.dir.y, -self.dir.x, 0.0);
                match (right_dir.dot(normal) >= 0.0, pistol) {
                    (true, false) => "a_pt_wallrun_right",
                    (false, false) => "a_pt_wallrun_left",
                    (true, true) => "a_pt_wallrun_pistol_right",
                    (false, true) => "a_pt_wallrun_pistol_left",
                }
            }
            DecoyState::WallHang => p("pt_wallrun_hang_idle", "pt_wallrun_Pistol_hang_idle"),
        }
    }
}

/// The sequences the decoy plays (`Decoy::clip`).
const DECOY_SEQUENCES: &[&str] = &[
    "CQB_Idle_MP",
    "CQB_Idle_MP_Pistol",
    "CQB_CrouchIdle_MP",
    "CrouchIdle_Pistol_MP",
    "CrouchRun_forward_mp",
    "CrouchRun_pistol_forward_mp",
    "CrouchRUN_mp_right",
    "CrouchRUN_mp_left",
    "Run_forward_mp",
    "Run_pistol_forward_mp",
    "Run_mp_right",
    "Run_mp_left",
    "Run_mp_right_pistol",
    "Run_mp_left_pistol",
    "Sprint_mp_forward",
    "Sprint_mp_forward_Pistol",
    "Slide_forward_mp",
    "Slide_Pistol_forward_mp",
    "MP_Pt_Slide_Float",
    "MP_Pt_Pistol_Slide_Float",
    "Jump_float_MP",
    "a_pt_wallrun_right",
    "a_pt_wallrun_left",
    "a_pt_wallrun_pistol_right",
    "a_pt_wallrun_pistol_left",
    "pt_wallrun_hang_idle",
    "pt_wallrun_Pistol_hang_idle",
];

/// EnableCloak( rider, 7.0 ) in HandleAutoEject (_titan_health.gnut).
pub const AUTO_EJECT_CLOAK: f32 = 7.0;

/// What the Pilot's abilities are doing right now (read by movement, enemies and damage).
#[derive(Resource, Default)]
pub struct PilotStatus {
    /// Index into TACTICALS.
    pub tactical: usize,
    /// Seconds left of the active tactical (Stim, Cloak, Phase Shift).
    pub active: f32,
    pub tac_charges: u32,
    /// Seconds until the next tactical charge.
    pub tac_cooldown: f32,
    /// Grapple power (0-100) and seconds since the last pull started.
    pub grapple_power: f32,
    pub grapple_regen: f32,
    /// Power the current pull has drained so far (up to grapple_power_required), and whether it
    /// is hooked into a target (those take the whole charge at once).
    pub grapple_used: f32,
    pub grapple_entity: bool,
    /// Index into ORDNANCE.
    pub ordnance: usize,
    pub ord_charges: u32,
    /// Seconds until the next ordnance charge.
    pub ord_cooldown: f32,
    pub decoy: Option<Decoy>,
    /// Seconds into the decoy's dissolve (Decoy_Dissolve: the model burns out over
    /// DECOY_DISSOLVE_MODEL seconds while the `dissolve` system runs, DECOY_DISSOLVE_TOTAL in all).
    pub decoy_dissolve: Option<f32>,
    /// Pulse Blade: where it landed and how long it keeps revealing.
    pub pulse: Option<(Vec3, f32)>,
    /// Satchels waiting for the detonator.
    pub satchels: u32,
    /// Seconds left of Stim's speed_boost status effect.
    pub stim_boost: f32,
    /// Seconds left of the cloak an Auto-Eject gives (EnableCloak( rider, 7.0 )).
    pub eject_cloak: f32,
    /// Seconds until that cloak starts (1 s after the eject, 2 with a nuclear payload).
    pub eject_cloak_in: Option<f32>,
}

impl PilotStatus {
    /// Full charges for the current choices (new game, or a loadout change).
    pub fn refill(&mut self, defs: &PilotAbilityDefs) {
        self.tac_charges = defs.tactical[self.tactical].charges;
        self.tac_cooldown = 0.0;
        self.grapple_power = 100.0;
        self.grapple_regen = f32::MAX;
        self.ord_charges = defs.ordnance[self.ordnance].charges;
        self.ord_cooldown = 0.0;
        self.active = 0.0;
        self.stim_boost = 0.0;
    }
    fn is(&self, t: Tactical) -> bool {
        self.active > 0.0 && TACTICALS[self.tactical].0 == t
    }
    /// The speed_boost severity now (0 without Stim).
    pub fn speed_boost(&self) -> f32 {
        STIM_SEVERITY * (self.stim_boost / STIM_BOOST_EASE_OUT).clamp(0.0, 1.0)
    }
    /// client.dll 0x180329030: max speeds scale by 1 + 2 * the speed_boost severity (Stim's 0.4
    /// makes 1.8; STIM_EFFECT_SEVERITY's comment assumes movement_speedboost_extraScale 2.0).
    pub fn speed_scale(&self) -> f32 {
        1.0 + 2.0 * self.speed_boost()
    }
    pub fn stimmed(&self) -> bool {
        self.is(Tactical::Stim)
    }
    pub fn cloaked(&self) -> bool {
        self.is(Tactical::Cloak) || self.eject_cloak > 0.0
    }
    pub fn phased(&self) -> bool {
        self.is(Tactical::PhaseShift)
    }
    /// Whether an enemy this far away can see the Pilot.
    pub fn visible_at(&self, dist: f32) -> bool {
        !self.phased() && !(self.cloaked() && dist > CLOAK_SEEN_WITHIN)
    }
}

/// Who an enemy at `from` goes after: the Holo Pilot decoy when it's closer than the player or
/// the player can't be seen, else the player. Returns the target and whether it's the decoy.
pub fn enemy_target(
    player: Option<(Vec3, f32, f32, bool)>,
    status: &PilotStatus,
    from: Vec3,
) -> (Option<(Vec3, f32, f32, bool)>, bool) {
    if let Some(d) = status.decoy {
        let decoy = (d.pos, 16.0, 72.0, true);
        let prefer = match player {
            None => true,
            Some(p) => {
                (p.3 && !status.visible_at(p.0.distance(from)))
                    || d.pos.distance(from) < p.0.distance(from)
            }
        };
        if prefer {
            return (Some(decoy), true);
        }
    }
    (player, false)
}

/// A-Wall shields that stop enemy rounds.
pub struct Shield {
    /// Foot of the arc's centre, game space.
    pub center: Vec3,
    /// Direction the arc faces (horizontal unit vector).
    pub facing: Vec3,
    pub health: f32,
    pub time: f32,
    pub entity: Entity,
}

#[derive(Resource, Default)]
pub struct Shields(pub Vec<Shield>);

impl Shields {
    /// Whether a round from `from` along `dir` (unit) passes out through a shield's face within
    /// `max_t` (the way you shoot through your own A-Wall), from inside its circle.
    pub fn passes_out(&self, from: Vec3, dir: Vec3, max_t: f32) -> bool {
        let half = (SHIELD_FOV * 0.5).to_radians().cos();
        self.0.iter().any(|s| {
            let o = (from - s.center).truncate();
            let d = dir.truncate();
            let (a, b, c) = (
                d.length_squared(),
                2.0 * o.dot(d),
                o.length_squared() - SHIELD_RADIUS * SHIELD_RADIUS,
            );
            if a < 1e-6 || c > 0.0 {
                return false;
            }
            // From inside the circle, the far root is where it leaves.
            let t = (-b + (b * b - 4.0 * a * c).max(0.0).sqrt()) / (2.0 * a);
            if t < 0.0 || t > max_t {
                return false;
            }
            let p = from + dir * t;
            let out = (p - s.center).truncate().normalize_or_zero();
            let z = p.z - s.center.z;
            out.dot(s.facing.truncate()) >= half && (0.0..=SHIELD_HEIGHT).contains(&z)
        })
    }

    /// The first shield a round from `from` along `dir` (unit) hits within `max_t`, and where.
    pub fn block(&self, from: Vec3, dir: Vec3, max_t: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        let half = (SHIELD_FOV * 0.5).to_radians().cos();
        for (i, s) in self.0.iter().enumerate() {
            let o = (from - s.center).truncate();
            let d = dir.truncate();
            let a = d.length_squared();
            if a < 1e-8 {
                continue;
            }
            let b = 2.0 * o.dot(d);
            let c = o.length_squared() - SHIELD_RADIUS * SHIELD_RADIUS;
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                continue;
            }
            for t in [
                (-b - disc.sqrt()) / (2.0 * a),
                (-b + disc.sqrt()) / (2.0 * a),
            ] {
                if t <= 0.0 || t >= max_t || best.is_some_and(|(_, bt)| bt <= t) {
                    continue;
                }
                let p = from + dir * t;
                let up = p.z - s.center.z;
                let side = (p - s.center).truncate().normalize_or_zero();
                if (0.0..=SHIELD_HEIGHT).contains(&up) && side.dot(s.facing.truncate()) >= half {
                    best = Some((i, t));
                }
            }
        }
        best
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stuck {
    No,
    World,
    /// Stuck to an enemy at this offset from its feet.
    Enemy(Entity, Vec3),
}

#[derive(Component)]
pub struct Grenade {
    /// Index into ORDNANCE, or None for the Pulse Blade.
    kind: Option<usize>,
    pos: Vec3,
    vel: Vec3,
    /// Frag fuse.
    fuse: f32,
    stuck: Stuck,
    /// Seconds since it stuck or landed.
    since: f32,
    /// Next damage tick (Firestar, Electric Smoke).
    tick: f32,
    first: bool,
    /// The A-Wall's projectile: the yaw it was thrown at (the wall faces that way).
    wall: Option<f32>,
    /// Spin angle while flying (radians).
    spin: f32,
}

#[derive(Component)]
pub struct DecoyActor;
#[derive(Component)]
pub struct ShieldMesh;

#[derive(Resource)]
pub struct PilotKitAssets {
    shield_mesh: Handle<Mesh>,
    shield_mat: Handle<StandardMaterial>,
    /// The Holo Pilot: its anchor (hidden until used), actor and gun hand (`ja_c_propGun`).
    decoy: Option<Entity>,
    pub decoy_actor: Option<Entity>,
    pub decoy_hand: Option<Entity>,
}

/// Build the A-Wall mesh and the Holo Pilot model once, while loading.
#[allow(clippy::too_many_arguments)]
pub fn setup_pilot_kit(
    mut commands: Commands,
    gd: Res<GameData>,
    mut cache: ResMut<Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    // The arc: a strip of SHIELD_FOV degrees, radius SHIELD_RADIUS, facing +X, game space.
    let segs = 16;
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut idx = Vec::new();
    for i in 0..=segs {
        let a = (i as f32 / segs as f32 - 0.5) * SHIELD_FOV.to_radians();
        let (x, y) = (a.cos() * SHIELD_RADIUS, a.sin() * SHIELD_RADIUS);
        pos.push([x, y, 0.0]);
        pos.push([x, y, SHIELD_HEIGHT]);
        nrm.push([a.cos(), a.sin(), 0.0]);
        nrm.push([a.cos(), a.sin(), 0.0]);
        uv.push([i as f32 / segs as f32, 1.0]);
        uv.push([i as f32 / segs as f32, 0.0]);
        if i < segs {
            let b = (i * 2) as u32;
            idx.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
        }
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    let shield_mesh = meshes.add(mesh);
    let shield_mat = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::new(0.06, 0.16, 0.45, 1.0)),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        fog_enabled: false,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    // The decoy: a copy of the player (CreatePlayerDecoy), so the player's own body model with
    // its locomotion clips; it carries the player's gun (`update_decoy_gun`).
    let anchor = commands
        .spawn((game_tf(Vec3::ZERO, 0.0), Visibility::Hidden, DecoyActor))
        .id();
    let spec = crate::actor::ActorSpec {
        path: crate::pilotbody::PILOT_BODY_MODEL,
        sequences: DECOY_SEQUENCES,
        grids: &[],
        body: &[],
    };
    let (decoy, decoy_actor, decoy_hand) = match crate::actor::spawn_actor(
        &mut commands,
        anchor,
        &gd,
        &spec,
        &mut cache,
        &mut meshes,
        &mut images,
        &mut materials,
        &mut bindposes,
    ) {
        Ok(a) => (Some(anchor), Some(a.entity), a.joint("ja_c_propGun")),
        Err(e) => {
            log::warn!("holo pilot model: {e:#}");
            (None, None, None)
        }
    };
    commands.insert_resource(PilotKitAssets {
        shield_mesh,
        shield_mat,
        decoy,
        decoy_actor,
        decoy_hand,
    });
}

/// A game-space placement (feet position, yaw about Z) as a Bevy world transform (the world
/// root's Z-up to Y-up turn and unit scale applied).
fn game_tf(pos: Vec3, yaw: f32) -> Transform {
    let root = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::splat(crate::player::UNIT));
    root.mul_transform(Transform::from_translation(pos).with_rotation(Quat::from_rotation_z(yaw)))
}

fn view_dir(s: &tf_sim::pilot::PilotState, pitch_offset_deg: f32) -> Vec3 {
    let pitch = s.pitch - pitch_offset_deg.to_radians();
    Vec3::new(
        pitch.cos() * s.yaw.cos(),
        pitch.cos() * s.yaw.sin(),
        -pitch.sin(),
    )
}

/// Use the tactical (Q) and ordnance (G) on foot; tick charges, Stim's regen, the decoy and
/// the shields.
#[allow(clippy::too_many_arguments)]
pub fn pilot_abilities(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    defs: Res<PilotAbilityDefs>,
    settings: Res<crate::pilotctl::PilotSettings>,
    kit: Option<Res<PilotKitAssets>>,
    mut status: ResMut<PilotStatus>,
    mut shields: ResMut<Shields>,
    mut input: ResMut<PlayerInput>,
    mut pilots: Query<(&mut PlayerPilot, &mut PilotHealth)>,
    mut throw_state: ResMut<OrdnanceThrow>,
    (world, enemies, titans): (
        Res<crate::player::Collision>,
        Query<&crate::targets::Enemy>,
        Query<&crate::player::PlayerTitan>,
    ),
) {
    let dt = time.delta_secs().min(0.1);
    let mut tactical = std::mem::take(&mut input.tactical);
    let throw = std::mem::take(&mut input.throw);
    let ti = status.tactical.min(defs.tactical.len() - 1);
    let oi = status.ordnance.min(defs.ordnance.len() - 1);
    let def = &defs.tactical[ti];
    let odef = &defs.ordnance[oi];
    status.stim_boost = (status.stim_boost - dt).max(0.0);
    status.eject_cloak = (status.eject_cloak - dt).max(0.0);
    if let Some(t) = status.eject_cloak_in {
        if t <= dt {
            status.eject_cloak_in = None;
            status.eject_cloak = AUTO_EJECT_CLOAK;
            log::info!("Auto-Eject: cloaked for {AUTO_EJECT_CLOAK} s");
            crate::audio::event(&mut commands, "cloak_on_1P");
        } else {
            status.eject_cloak_in = Some(t - dt);
        }
    }
    if status.active > 0.0 {
        status.active -= dt;
        // The sustain loop the ability script keeps up while it runs (pilot_stimpack_loop_1P
        // etc.); it stops by itself if the tactical is reset from the menu.
        let sustain = match TACTICALS[ti].0 {
            Tactical::Stim => Some("pilot_stimpack_loop_1P"),
            Tactical::Cloak => Some("cloak_sustain_loop_1P"),
            Tactical::PhaseShift => Some("Pilot_PhaseShift_Loop_1P"),
            _ => None,
        };
        if let Some(ev) = sustain {
            crate::audio::loop_hold(&mut commands, "tactical", ev, None);
        }
        if status.active <= 0.0 {
            status.active = 0.0;
            let end = match TACTICALS[ti].0 {
                Tactical::Cloak => Some("cloak_interruptend_1P"),
                Tactical::PhaseShift => Some("Pilot_PhaseShift_End_1P"),
                _ => None,
            };
            crate::audio::loop_stop(&mut commands, "tactical", end);
        }
    }
    // Grapple power regenerates grapple_power_regen_rate per second from
    // grapple_power_regen_delay after the last pull started (client.dll 0x1802ce140); the HUD
    // shows it as charges of grapple_power_required.
    let need = 100.0 / def.charges.max(1) as f32;
    if TACTICALS[ti].0 == Tactical::Grapple {
        let rate = need / def.cooldown.max(0.1);
        status.grapple_regen = (status.grapple_regen + dt).min(1e6);
        if status.grapple_power < 100.0 && status.grapple_regen >= def.regen_delay {
            status.grapple_power = (status.grapple_power + rate * dt).min(100.0);
            if status.grapple_power >= 100.0 {
                crate::audio::event(&mut commands, "pilot_grapple_ready");
            }
        }
        status.tac_charges = ((status.grapple_power + 1e-3) / need) as u32;
        let next = need * (status.tac_charges + 1) as f32 - status.grapple_power;
        status.tac_cooldown = if status.tac_charges < def.charges {
            next / rate + (def.regen_delay - status.grapple_regen).max(0.0)
        } else {
            0.0
        };
    } else if status.tac_charges < def.charges && status.active <= 0.0 {
        // Charges refill one at a time (not while a timed tactical runs).
        status.tac_cooldown -= dt;
        if status.tac_cooldown <= 0.0 {
            status.tac_charges += 1;
            status.tac_cooldown = if status.tac_charges < def.charges {
                def.cooldown
            } else {
                0.0
            };
        }
    }
    if status.ord_charges < odef.charges {
        status.ord_cooldown -= dt;
        if status.ord_cooldown <= 0.0 {
            status.ord_charges += 1;
            status.ord_cooldown = if status.ord_charges < odef.charges {
                odef.cooldown
            } else {
                0.0
            };
        }
    }
    if let Some((_, t)) = status.pulse.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            status.pulse = None;
        }
    }

    // Shields wear out.
    for s in &mut shields.0 {
        s.time -= dt;
    }
    shields.0.retain(|s| {
        let keep = s.time > 0.0 && s.health > 0.0;
        if !keep {
            log::info!(
                "a-wall down with {:.0} of {SHIELD_HEALTH:.0} health left",
                s.health.max(0.0)
            );
            commands.entity(s.entity).despawn();
            crate::audio::event_at(&mut commands, "Hardcover_Shield_End_3P", s.center);
        }
        keep
    });

    let Ok((mut pilot, mut health)) = pilots.single_mut() else {
        return;
    };
    if *control != Control::Pilot || health.dead() {
        pilot.state.grapple = None;
        pilot.state.hook = None;
        pilot.state.grapple_events.clear();
        return;
    }
    // What the grapple did in the movement step. Power (client.dll 0x1804f5ed0 / 0x1804f55d0):
    // a pull off the world costs nothing for grapple_gracePeriod, then drains
    // grapple_power_use_rate per second up to grapple_power_required (a short hop is cheap); a
    // pull into a target takes the whole charge when it starts. A miss costs nothing.
    for ev in std::mem::take(&mut pilot.state.grapple_events) {
        match ev {
            GrappleEvent::Hooked { point, normal } => {
                let at = Vec3::from(point.to_array());
                log::info!(
                    "grapple hooked {:.0} units away",
                    (at - Vec3::from(pilot.state.eye(&settings.0).to_array())).length()
                );
                status.grapple_entity = false;
                crate::audio::event_at(&mut commands, "default_grapple_impact_1p_vs_3p", at);
                emit(
                    &mut commands,
                    Effect::Impact {
                        at: to_bevy(at),
                        normal: to_bevy(Vec3::from(normal.to_array())),
                        scale: 0.5,
                        energy: false,
                    },
                );
            }
            GrappleEvent::HookedEntity { id, point } => {
                let at = Vec3::from(point.to_array());
                log::info!(
                    "grapple hooked {} {:.0} units away",
                    if id == crate::pilotctl::BT_HOOK_ID {
                        "BT"
                    } else {
                        "an enemy"
                    },
                    (at - Vec3::from(pilot.state.eye(&settings.0).to_array())).length()
                );
                status.grapple_entity = true;
                crate::audio::event_at(&mut commands, "default_grapple_impact_1p_vs_3p", at);
            }
            GrappleEvent::Pulling => {
                status.grapple_used = 0.0;
                status.grapple_regen = 0.0;
                if status.grapple_entity {
                    status.grapple_power = (status.grapple_power - need).max(0.0);
                    status.grapple_used = need;
                }
            }
            GrappleEvent::Melee(_) => {
                // grapple_autoMeleeOnDetach: the grapple into someone ends in a melee swing.
                log::info!("grapple: auto melee");
                input.melee = true;
            }
            GrappleEvent::Missed => crate::audio::event(&mut commands, "pilot_grapple_retract_1p"),
            GrappleEvent::Released => {
                log::info!(
                    "grapple released ({:.1} of {need:.0} power used, {:.0} left)",
                    status.grapple_used,
                    status.grapple_power
                );
                crate::audio::event(&mut commands, "pilot_grapple_retract_1p")
            }
        }
    }
    if let Some(t) = pilot.state.grapple_pull_time() {
        if !status.grapple_entity && t >= GRAPPLE_GRACE && status.grapple_used < need {
            let take = (def.use_rate * dt)
                .min(need - status.grapple_used - 0.001)
                .max(0.0);
            status.grapple_used += take;
            status.grapple_power = (status.grapple_power - take).max(0.0);
        }
    }
    if status.is(Tactical::Stim) {
        health.health = (health.health + STIM_REGEN * dt).min(PILOT_HEALTH);
    }
    // TF_PILOT_GOD=1 keeps the Pilot alive (scripted ability tests).
    if std::env::var_os("TF_PILOT_GOD").is_some() {
        health.health = PILOT_HEALTH;
    }
    let eye = Vec3::from(pilot.state.eye(&settings.0).to_array());
    let look = view_dir(&pilot.state, 0.0);
    let pvel = Vec3::from(pilot.state.vel.to_array());
    let spend = |status: &mut PilotStatus| {
        if status.tac_charges == def.charges {
            status.tac_cooldown = def.cooldown;
        }
        status.tac_charges -= 1;
    };
    // The throw: pull out (toss_pullout_time), hold while the button is held, toss on
    // release (toss_time), then the gun comes back up.
    let mut launch = false;
    let (pullout, toss) = if throw_state.tactical {
        (def.pullout, def.toss)
    } else {
        (odef.pullout, odef.toss)
    };
    if let Some((phase, t0)) = throw_state.phase {
        let t = t0 + dt;
        throw_state.phase = Some((phase, t));
        match phase {
            ThrowPhase::Pullout if t >= pullout => {
                if input.ordnance && !throw_state.tactical {
                    throw_state.phase = Some((ThrowPhase::Hold, 0.0));
                } else {
                    throw_state.phase = Some((ThrowPhase::Toss, 0.0));
                    launch = true;
                }
            }
            ThrowPhase::Hold => {
                throw_state.cooked = t;
                // A fused grenade held past its fuse goes off in hand.
                if !input.ordnance || (odef.fuse > 0.0 && t >= odef.fuse) {
                    throw_state.phase = Some((ThrowPhase::Toss, 0.0));
                    launch = true;
                }
            }
            ThrowPhase::Toss if t >= toss => throw_state.phase = Some((ThrowPhase::Raise, 0.0)),
            ThrowPhase::Raise if t >= THROW_RAISE_SECS => throw_state.phase = None,
            _ => {}
        }
    }
    // Pulse Blade and A-Wall are thrown: the press starts the throw, the toss uses the charge.
    let thrown_tac = matches!(TACTICALS[ti].0, Tactical::PulseBlade | Tactical::AWall)
        && !def.viewmodel.is_empty();
    if tactical && thrown_tac && !launch {
        if status.tac_charges > 0 && status.active <= 0.0 && !throw_state.busy() {
            throw_state.phase = Some((ThrowPhase::Pullout, 0.0));
            throw_state.tactical = true;
        }
        tactical = false;
    }
    if launch && throw_state.tactical {
        tactical = true;
    }
    if tactical {
        let kind = TACTICALS[ti].0;
        if kind == Tactical::Grapple
            && (pilot.state.grapple.is_some() || pilot.state.hook.is_some_and(|h| h.back.is_none()))
        {
            // Pressing again while the hook is out lets go (CPlayer::Grapple, client.dll
            // 0x1802c8600).
            pilot.state.release_grapple(&settings.0);
        } else if status.tac_charges > 0 && status.active <= 0.0 {
            if kind != Tactical::Grapple {
                spend(&mut status);
            }
            crate::audio::event(&mut commands, TACTICALS[ti].4);
            log::info!("tactical: {kind:?}");
            match kind {
                Tactical::Stim | Tactical::Cloak | Tactical::PhaseShift => {
                    status.active = def.duration;
                    if kind == Tactical::Stim {
                        status.stim_boost = def.duration + STIM_BOOST_EXTRA;
                    }
                }
                // The hook flies out from the eye at grapple_shootVel up to grapple_maxLength
                // (tf-sim moves it and reports where it lands). When the straight shot would
                // hit nothing in reach, it is steered at a target in the aim's cone
                // (client.dll 0x1802cd1e0).
                Tactical::Grapple => {
                    let targets =
                        crate::pilotctl::hook_targets(enemies.iter(), titans.single().ok(), None);
                    let dir = grapple_snap(
                        SVec3::from(eye.to_array()),
                        SVec3::from(look.to_array()),
                        def.speed.max(def.max_len_vert),
                        &world.0,
                        &targets,
                    );
                    pilot
                        .state
                        .fire_grapple(dir, def.speed, def.max_len_vert, &settings.0);
                }
                Tactical::PulseBlade => {
                    let dir = view_dir(&pilot.state, 2.0);
                    commands.spawn((
                        Transform::from_translation(to_bevy(eye)),
                        crate::particles::Glow::new(Vec3::new(0.6, 2.0, 5.0), 0.12, false),
                        Grenade {
                            kind: None,
                            pos: eye + dir * 20.0,
                            vel: dir * def.speed + pvel,
                            fuse: 0.0,
                            stuck: Stuck::No,
                            since: 0.0,
                            tick: 0.0,
                            first: true,
                            wall: None,
                            spin: 0.0,
                        },
                    ));
                }
                Tactical::HoloPilot => {
                    let mut d =
                        Decoy::from_player(&pilot.state, &settings.0, input.forward, input.right);
                    d.time = def.duration;
                    log::debug!("holo pilot: {:?}", d.state);
                    status.decoy = Some(d);
                    if kit.as_ref().is_some_and(|k| k.decoy.is_none()) {
                        log::warn!("holo pilot has no model; enemies still target it");
                    }
                }
                Tactical::AWall => {
                    // ThrowDeployable (sh_deployable.gnut): from 15 units ahead of the eye,
                    // aimed 8 degrees above the view at DEPLOYABLE_THROW_POWER, up to three
                    // times harder looking down (GetDeployableThrowVelocity); it plants where
                    // it lands (OnDeployableCoverPlanted, in update_grenades).
                    let pitch = pilot.state.pitch.to_degrees() - 8.0;
                    let power =
                        DEPLOYABLE_THROW_POWER * (1.0 + 2.0 * (pitch / 80.0).clamp(0.0, 1.0));
                    let p = pitch.to_radians();
                    let dir = Vec3::new(
                        p.cos() * pilot.state.yaw.cos(),
                        p.cos() * pilot.state.yaw.sin(),
                        -p.sin(),
                    );
                    commands.spawn((
                        Transform::from_translation(to_bevy(eye)),
                        crate::particles::Glow::new(Vec3::new(0.6, 2.0, 5.0), 0.12, false),
                        Grenade {
                            kind: None,
                            pos: eye + look * 15.0,
                            vel: dir * power,
                            fuse: 0.0,
                            stuck: Stuck::No,
                            since: 0.0,
                            tick: 0.0,
                            first: true,
                            wall: Some(pilot.state.yaw),
                            spin: 0.0,
                        },
                    ));
                }
            }
        }
    }

    if throw && !throw_state.busy() {
        let kind = ORDNANCE[oi].0;
        if kind == Ordnance::Satchel && status.satchels > 0 {
            // The second press is the clacker.
            status.satchels = 0;
            crate::audio::event(&mut commands, "weapon_r1_satchel_armedbeep");
            commands.insert_resource(Detonate);
        } else if status.ord_charges > 0 {
            throw_state.phase = Some((ThrowPhase::Pullout, 0.0));
            throw_state.cooked = 0.0;
            throw_state.tactical = false;
        }
    }
    if launch && !throw_state.tactical {
        let kind = ORDNANCE[oi].0;
        if status.ord_charges > 0 {
            if status.ord_charges == odef.charges {
                status.ord_cooldown = odef.cooldown;
            }
            status.ord_charges -= 1;
            if kind == Ordnance::Satchel {
                status.satchels += 1;
            }
            let dir = view_dir(&pilot.state, odef.pitch_offset);
            crate::audio::event(&mut commands, ORDNANCE[oi].4);
            let color = match kind {
                Ordnance::Arc | Ordnance::ElectricSmoke => Vec3::new(1.0, 2.5, 6.0),
                Ordnance::GravityStar => Vec3::new(3.0, 1.0, 6.0),
                Ordnance::Satchel => Vec3::new(4.0, 0.3, 0.2),
                _ => Vec3::new(3.0, 1.2, 0.4),
            };
            commands.spawn((
                Transform::from_translation(to_bevy(eye)),
                crate::particles::Glow::new(color, 0.12, false),
                Grenade {
                    kind: Some(oi),
                    pos: eye + dir * 20.0,
                    vel: dir * odef.speed + pvel,
                    fuse: if odef.fuse > 0.0 {
                        (odef.fuse - throw_state.cooked).max(0.05)
                    } else {
                        0.0
                    },
                    stuck: Stuck::No,
                    since: 0.0,
                    tick: 0.0,
                    first: true,
                    wall: None,
                    spin: 0.0,
                },
            ));
        }
    }
}

/// Satchels go off this frame.
#[derive(Resource)]
pub struct Detonate;

fn radius_damage(
    enemies: &mut Query<(Entity, &mut Enemy)>,
    at: Vec3,
    inner: f32,
    radius: f32,
    dmg: f32,
    heavy: f32,
) -> u32 {
    let mut hits = 0;
    for (_, mut e) in enemies.iter_mut() {
        if !e.alive() {
            continue;
        }
        let d = (e.pos + Vec3::Z * e.height * 0.5).distance(at) - e.radius;
        if d < radius {
            let k = 1.0 - ((d - inner) / (radius - inner).max(1.0)).clamp(0.0, 1.0);
            let amount = if e.infantry { dmg } else { heavy } * k;
            if amount > 0.0 {
                // RadiusDamage: a Vortex or Thermal Shield doesn't stop the blast.
                e.damage_radius(amount, false);
                hits += 1;
                log::debug!(
                    "ordnance hit {} for {amount:.0}",
                    if e.infantry { "grunt" } else { "titan" }
                );
            }
        }
    }
    hits
}

/// Fly, bounce, stick and go off: all thrown ordnance and the Pulse Blade.
#[allow(clippy::too_many_arguments)]
pub fn update_grenades(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    defs: Res<PilotAbilityDefs>,
    detonate: Option<Res<Detonate>>,
    mut status: ResMut<PilotStatus>,
    mut grenades: Query<(Entity, &mut Grenade, &mut Transform)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    control: Res<Control>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    (kit, mut shields): (Option<Res<PilotKitAssets>>, ResMut<Shields>),
) {
    let dt = time.delta_secs().min(0.05);
    let boom = detonate.is_some();
    // Your own ordnance hurts you too (the explosion's pilot damage, by distance).
    let god = std::env::var_os("TF_PILOT_GOD").is_some();
    let self_damage = |pilots: &mut Query<(&PlayerPilot, &mut PilotHealth)>,
                       at: Vec3,
                       inner: f32,
                       radius: f32,
                       dmg: f32| {
        if *control != Control::Pilot || god {
            return;
        }
        for (p, mut h) in pilots.iter_mut() {
            let d = (Vec3::from(p.state.pos.to_array()) + Vec3::Z * 36.0).distance(at) - 16.0;
            if d < radius {
                let k = 1.0 - ((d - inner) / (radius - inner).max(1.0)).clamp(0.0, 1.0);
                if dmg * k > 0.0 {
                    h.damage(dmg * k);
                    log::info!("own ordnance hit the pilot for {:.0}", dmg * k);
                }
            }
        }
    };
    if boom {
        commands.remove_resource::<Detonate>();
    }
    for (id, mut g, mut tf) in &mut grenades {
        let kind = g.kind.map(|i| ORDNANCE[i].0);
        let def = g.kind.map(|i| &defs.ordnance[i]);
        // Follow the enemy it's stuck to.
        if let Stuck::Enemy(who, off) = g.stuck {
            match enemies.get(who) {
                Ok((_, e)) if e.alive() => g.pos = e.pos + off,
                _ => g.stuck = Stuck::World,
            }
        }
        if g.stuck == Stuck::No {
            g.vel.z -= GRENADE_GRAVITY
                * if g.wall.is_some() {
                    AWALL_GRAVITY_SCALE
                } else {
                    1.0
                }
                * dt;
            let step = g.vel * dt;
            let len = step.length();
            if len > 1e-4 {
                let dir = step / len;
                let mut t_end = len + GRENADE_RADIUS;
                let mut normal = None;
                if let Some(h) = world.0.raycast(
                    SVec3::from(g.pos.to_array()),
                    SVec3::from(dir.to_array()),
                    t_end,
                ) {
                    t_end = h.t;
                    normal = Some(Vec3::from(h.normal.to_array()));
                }
                // Direct hits on enemies (not for the bouncing frag).
                let mut hit_enemy = None;
                if kind != Some(Ordnance::Frag) {
                    for (eid, e) in enemies.iter() {
                        if e.alive() {
                            if let Some(t) = crate::weapons::ray_cylinder(
                                g.pos,
                                dir,
                                e.pos,
                                e.radius + 4.0,
                                e.height,
                            ) {
                                if t < t_end {
                                    t_end = t;
                                    hit_enemy = Some(eid);
                                }
                            }
                        }
                    }
                }
                if let Some(eid) = hit_enemy {
                    g.pos += dir * t_end;
                    if let (Some(d), Ok((_, mut e))) = (def, enemies.get_mut(eid)) {
                        let dmg = if e.infantry { d.impact } else { d.impact_heavy };
                        if dmg > 0.0 {
                            e.damage(dmg, false);
                        }
                    }
                    if g.kind.is_none() {
                        // The Pulse Blade's 100 on a direct hit.
                        if let Ok((_, mut e)) = enemies.get_mut(eid) {
                            e.damage(100.0, false);
                        }
                    }
                    let off = g.pos - enemies.get(eid).map(|(_, e)| e.pos).unwrap_or(g.pos);
                    g.stuck = Stuck::Enemy(eid, off);
                    g.since = 0.0;
                } else if let Some(n) = normal {
                    if kind == Some(Ordnance::Frag) {
                        // grenade_bounce_vel_frac_*: glancing hits keep more speed than head-on.
                        let d = def.unwrap();
                        let into = -dir.dot(n);
                        let keep = d.bounce_shallow
                            + (d.bounce_sharp - d.bounce_shallow) * into.clamp(0.0, 1.0);
                        let v = g.vel - 2.0 * g.vel.dot(n) * n;
                        g.vel = v * keep;
                        g.pos += dir * (t_end - GRENADE_RADIUS).max(0.0) + n * 0.5;
                    } else {
                        g.pos += dir * (t_end - GRENADE_RADIUS).max(0.0) + n * 1.0;
                        g.stuck = Stuck::World;
                        g.since = 0.0;
                        if kind == Some(Ordnance::Satchel) {
                            crate::audio::event_at(
                                &mut commands,
                                "weapon_r1_satchel_attach",
                                g.pos,
                            );
                        }
                    }
                } else {
                    g.pos += step;
                }
            }
        } else {
            g.since += dt;
        }
        tf.translation = to_bevy(g.pos);
        let landed = g.stuck != Stuck::No;
        // The model flies nose (+X) first: stars spin flat, the round grenades and devices
        // tumble, the Pulse Blade's kunai flies straight and sticks point first. Stuck, it
        // keeps its last pose.
        if !landed && g.vel.length_squared() > 1.0 {
            let x = g.vel.normalize();
            let y = Vec3::Z.cross(x).try_normalize().unwrap_or(Vec3::Y);
            let base = Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)));
            let (rate, axis) = match kind {
                Some(Ordnance::Firestar | Ordnance::GravityStar) => (25.0, Vec3::Z),
                None if g.wall.is_none() => (0.0, Vec3::Z),
                _ => (10.0, Vec3::Y),
            };
            g.spin += rate * dt;
            let q = base * Quat::from_axis_angle(axis, g.spin);
            // Game-space rotation in the Bevy frame (game -> Bevy is -90 degrees about X).
            let c = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
            tf.rotation = c * q * c.inverse();
        }

        // The A-Wall goes up where it lands: the arc's centre DEPLOYABLE_SHIELD_RADIUS - 1
        // behind the projectile (so its face stands at it) and 1 unit down, facing the throw
        // (DeployAmpedWall), drawn by P_pilot_amped_shield.
        if let Some(yaw) = g.wall {
            if landed {
                commands.entity(id).despawn();
                let fwd = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
                let center = g.pos - fwd * (SHIELD_RADIUS - 1.0) - Vec3::Z;
                let e = if std::env::var_os("TF_OLD_FX").is_some() {
                    kit.as_ref().map(|k| {
                        commands
                            .spawn((
                                Mesh3d(k.shield_mesh.clone()),
                                MeshMaterial3d(k.shield_mat.clone()),
                                game_tf(center, yaw),
                                ShieldMesh,
                            ))
                            .id()
                    })
                } else {
                    Some(
                        commands
                            .spawn((
                                Transform::from_translation(to_bevy(center))
                                    .looking_to(to_bevy(fwd).normalize(), Vec3::Y),
                                Visibility::default(),
                                crate::pfx::PfxTrail::oriented("P_pilot_amped_shield"),
                            ))
                            .id(),
                    )
                };
                if let Some(e) = e {
                    log::info!("a-wall up at {center:.0}");
                    // The other players get it too (net.rs).
                    if let Ok(mut w) = crate::net::WALLS.lock() {
                        w.push((
                            center,
                            fwd,
                            SHIELD_DURATION,
                            SHIELD_HEALTH,
                            crate::net::WALL_AWALL,
                        ));
                    }
                    shields.0.push(Shield {
                        center,
                        facing: fwd,
                        health: SHIELD_HEALTH,
                        time: SHIELD_DURATION,
                        entity: e,
                    });
                    crate::audio::event_at(&mut commands, "Hardcover_Shield_Start_3P", g.pos);
                }
            }
            continue;
        }

        // The Pulse Blade pulses once it lands.
        let Some(kind) = kind else {
            if landed && g.first {
                g.first = false;
                status.pulse = Some((g.pos, SONAR_DURATION));
                log::info!("pulse blade landed");
                crate::audio::event_at(&mut commands, "Pilot_PulseBlade_Sonar_Pulse_1P", g.pos);
                emit(
                    &mut commands,
                    Effect::DustRing {
                        at: to_bevy(g.pos),
                        radius: 6.0,
                    },
                );
            }
            if landed && g.since > SONAR_DURATION {
                commands.entity(id).despawn();
            }
            continue;
        };
        let d = def.unwrap();
        let sound = ORDNANCE[g.kind.unwrap()].5;
        match kind {
            Ordnance::Frag => {
                g.fuse -= dt;
                if g.fuse <= 0.0 {
                    commands.entity(id).despawn();
                    emit(
                        &mut commands,
                        Effect::Explosion {
                            at: to_bevy(g.pos),
                            scale: 1.2,
                        },
                    );
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(
                        &mut enemies,
                        g.pos,
                        d.inner,
                        d.radius,
                        d.damage,
                        d.damage_heavy,
                    );
                    self_damage(&mut pilots, g.pos, d.inner, d.radius, d.damage);
                    log::info!("frag exploded, {n} enemies hit");
                }
            }
            Ordnance::Arc => {
                if landed {
                    commands.entity(id).despawn();
                    emit(
                        &mut commands,
                        Effect::Impact {
                            at: to_bevy(g.pos),
                            normal: Vec3::Y,
                            scale: 4.0,
                            energy: true,
                        },
                    );
                    emit(
                        &mut commands,
                        Effect::DustRing {
                            at: to_bevy(g.pos),
                            radius: d.radius * crate::player::UNIT,
                        },
                    );
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(
                        &mut enemies,
                        g.pos,
                        d.inner,
                        d.radius,
                        d.damage,
                        d.damage_heavy,
                    );
                    self_damage(&mut pilots, g.pos, d.inner, d.radius, d.damage);
                    log::info!("arc grenade went off, {n} enemies hit");
                    for (_, mut e) in enemies.iter_mut() {
                        let dist = e.pos.distance(g.pos);
                        if e.alive() && dist < d.radius {
                            let k = 1.0 - dist / d.radius;
                            e.stun(ARC_STUN.0 + (ARC_STUN.1 - ARC_STUN.0) * k);
                        }
                    }
                }
            }
            Ordnance::Firestar => {
                if landed {
                    if g.first {
                        crate::audio::event_at(&mut commands, sound, g.pos);
                    }
                    g.tick -= dt;
                    if g.tick <= 0.0 {
                        g.tick += THERMITE_TICK;
                        // The first burst is stronger (x1.2, x2.5 against heavy armour).
                        let (m, mh) = if g.first { (1.2, 2.5) } else { (1.0, 1.0) };
                        radius_damage(
                            &mut enemies,
                            g.pos,
                            d.inner,
                            d.radius,
                            d.damage * m,
                            d.damage_heavy * mh,
                        );
                        emit(
                            &mut commands,
                            Effect::TrailPuff {
                                at: to_bevy(g.pos + Vec3::Z * 10.0),
                                scale: 1.5,
                            },
                        );
                        emit(
                            &mut commands,
                            Effect::Impact {
                                at: to_bevy(g.pos),
                                normal: Vec3::Y,
                                scale: 2.0,
                                energy: false,
                            },
                        );
                        g.first = false;
                    }
                    if g.since > THERMITE_BURN_TIME {
                        log::info!("firestar burnt out");
                        commands.entity(id).despawn();
                    }
                }
            }
            Ordnance::GravityStar => {
                if landed {
                    if g.since >= GRAV_POP_DELAY {
                        if g.first {
                            g.first = false;
                            if g.stuck == Stuck::World {
                                g.pos.z += GRAV_POP_HEIGHT;
                                g.stuck = Stuck::World;
                            }
                            crate::audio::event_at(
                                &mut commands,
                                "weapon_gravitystar_preexplo",
                                g.pos,
                            );
                            crate::net::event(crate::net::EV_GRAVITY_STAR, g.pos, Vec3::Z);
                        }
                        // Pull grunts in (the intense pull: accel 2000 up to 400 u/s).
                        for (_, mut e) in enemies.iter_mut() {
                            if e.alive() && e.infantry {
                                let to = g.pos - (e.pos + Vec3::Z * 36.0);
                                let dist = to.length();
                                if dist < GRAV_PULL_RADIUS && dist > 20.0 {
                                    let speed = (GRAV_PULL_ACCEL * (g.since - GRAV_POP_DELAY))
                                        .min(GRAV_PULL_SPEED);
                                    let step = to / dist * speed * dt;
                                    e.state.pos += SVec3::new(step.x, step.y, 0.0);
                                    e.pos = Vec3::from(e.state.pos.to_array());
                                    e.stun(0.3);
                                }
                            }
                        }
                    }
                    if g.since >= GRAV_POP_DELAY + GRAV_PULL_DELAY {
                        commands.entity(id).despawn();
                        emit(
                            &mut commands,
                            Effect::Explosion {
                                at: to_bevy(g.pos),
                                scale: 1.0,
                            },
                        );
                        crate::audio::event_at(&mut commands, sound, g.pos);
                        let n = radius_damage(
                            &mut enemies,
                            g.pos,
                            d.inner.min(d.radius - 1.0),
                            d.radius,
                            d.damage,
                            d.damage_heavy,
                        );
                        log::info!("gravity star exploded, {n} enemies hit");
                    }
                }
            }
            Ordnance::ElectricSmoke => {
                if landed {
                    if g.first {
                        g.first = false;
                        g.tick = SMOKE_DELAY;
                        crate::audio::event_at(&mut commands, sound, g.pos);
                    }
                    // Smoke and arcs filling the cloud.
                    if (g.since * 10.0) as u32 != ((g.since - dt) * 10.0) as u32 {
                        let a = g.since * 7.3;
                        let r = (d.radius * 0.6) * (a * 0.37).sin().abs();
                        let p = g.pos + Vec3::new(a.cos() * r, a.sin() * r, 30.0);
                        emit(
                            &mut commands,
                            Effect::TrailPuff {
                                at: to_bevy(p),
                                scale: 4.0,
                            },
                        );
                        emit(
                            &mut commands,
                            Effect::Impact {
                                at: to_bevy(p),
                                normal: Vec3::Y,
                                scale: 1.0,
                                energy: true,
                            },
                        );
                    }
                    g.tick -= dt;
                    while g.tick <= 0.0 {
                        g.tick += SMOKE_TICK;
                        radius_damage(
                            &mut enemies,
                            g.pos,
                            d.inner,
                            d.radius,
                            d.damage * SMOKE_TICK,
                            d.damage_heavy * SMOKE_TICK,
                        );
                    }
                    if g.since > SMOKE_LIFETIME {
                        log::info!("electric smoke cleared");
                        commands.entity(id).despawn();
                    }
                }
            }
            Ordnance::Satchel => {
                if boom {
                    commands.entity(id).despawn();
                    emit(
                        &mut commands,
                        Effect::Explosion {
                            at: to_bevy(g.pos),
                            scale: 1.5,
                        },
                    );
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(
                        &mut enemies,
                        g.pos,
                        d.inner,
                        d.radius,
                        d.damage,
                        d.damage_heavy,
                    );
                    self_damage(&mut pilots, g.pos, d.inner, d.radius, d.damage);
                    log::info!("satchel detonated, {n} enemies hit");
                }
            }
        }
    }
}

// --- HUD: the Pilot's Q / G slots, bottom left (shown on foot), and Pulse Blade markers ---

#[derive(Component)]
pub struct PilotSlots;
#[derive(Component)]
pub struct PilotSlotShade(usize);
#[derive(Component)]
pub struct PilotSlotText(usize);
#[derive(Component)]
pub struct PilotSlotIcon(usize);
#[derive(Component)]
pub struct PhaseTint;
#[derive(Component)]
pub struct PilotSlotLabel(usize);
#[derive(Component)]
pub struct PulseMarker(usize);

const PULSE_MARKERS: usize = 24;

pub fn spawn_pilot_slots(
    mut commands: Commands,
    ui: Res<UiAssets>,
    status: Res<PilotStatus>,
    hud: Query<Entity, With<crate::hud::HudRoot>>,
) {
    let vh = Val::Vh;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vh(crate::hud::SLOTS_X),
                top: vh(crate::hud::SLOTS_Y),
                column_gap: vh(crate::hud::SLOT_GAP),
                ..default()
            },
            Visibility::Hidden,
            PilotSlots,
        ))
        .id();
    // Part of the cockpit HUD: sways and powers on/off with it.
    if let Ok(h) = hud.single() {
        commands.entity(h).add_child(root);
    }
    let tac = TACTICALS[status.tactical];
    let ord = ORDNANCE[status.ordnance];
    for (i, (icon, key)) in [(tac.2, "Q"), (ord.2, "G")].into_iter().enumerate() {
        let p = crate::hud::slot_column(&mut commands, &ui, icon, key);
        commands.entity(p.icon).insert(PilotSlotIcon(i));
        commands.entity(p.shade).insert(PilotSlotShade(i));
        commands.entity(p.state).insert(PilotSlotText(i));
        commands.entity(p.label).insert(PilotSlotLabel(i));
        commands.entity(root).add_child(p.col);
    }
    // The old Phase Shift / Cloak screen tints (TF_NO_PHASE_FX / TF_NO_CLOAK_FX).
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        PhaseTint,
    ));
    // Pulse Blade reveals: red diamonds over enemies, seen through walls.
    for i in 0..PULSE_MARKERS {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: vh(1.4),
                height: vh(1.4),
                margin: UiRect {
                    left: vh(-0.7),
                    top: vh(-0.7),
                    ..default()
                },
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(1.0, 0.25, 0.15)),
            BackgroundColor(Color::srgba(1.0, 0.25, 0.15, 0.25)),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            Visibility::Hidden,
            PulseMarker(i),
        ));
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_pilot_slots(
    control: Res<Control>,
    game: Res<crate::game::Game>,
    defs: Res<PilotAbilityDefs>,
    status: Res<PilotStatus>,
    mut root: Query<&mut Visibility, (With<PilotSlots>, Without<PulseMarker>)>,
    hidden: Res<crate::hud::HudHidden>,
    mut shades: Query<(&PilotSlotShade, &mut Node), Without<PulseMarker>>,
    mut texts: Query<(&PilotSlotText, &mut Text), Without<PilotSlotLabel>>,
    mut tint: Query<&mut BackgroundColor, (With<PhaseTint>, Without<PulseMarker>)>,
    mut icons: Query<(&PilotSlotIcon, &mut ImageNode)>,
    mut labels: Query<(&PilotSlotLabel, &mut Text), Without<PilotSlotText>>,
    ui: Res<UiAssets>,
) {
    // The choices can change in the menu.
    if status.is_changed() {
        let (t, o) = (TACTICALS[status.tactical], ORDNANCE[status.ordnance]);
        for (i, mut img) in &mut icons {
            if let Some(new) = ui.image(if i.0 == 0 { t.2 } else { o.2 }) {
                *img = new;
            }
        }
        for (i, mut l) in &mut labels {
            let s = if i.0 == 0 { t.3 } else { o.3 };
            if l.0 != s {
                l.0 = s.to_string();
            }
        }
    }
    let playing = matches!(
        game.state,
        crate::game::GameState::Playing | crate::game::GameState::Intermission
    ) && !hidden.0;
    if let Ok(mut v) = root.single_mut() {
        v.set_if_neq(if *control == Control::Pilot && playing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let def = &defs.tactical[status.tactical.min(defs.tactical.len() - 1)];
    let odef = &defs.ordnance[status.ordnance.min(defs.ordnance.len() - 1)];
    let tac_frac = if status.active > 0.0 || status.tac_charges > 0 {
        0.0
    } else {
        (status.tac_cooldown / def.cooldown.max(0.1)).clamp(0.0, 1.0)
    };
    let ord_frac = if status.ord_charges > 0 {
        0.0
    } else {
        (status.ord_cooldown / odef.cooldown.max(0.1)).clamp(0.0, 1.0)
    };
    for (s, mut n) in &mut shades {
        crate::hud::set(
            &mut n,
            |n| &mut n.height,
            Val::Percent(100.0 * if s.0 == 0 { tac_frac } else { ord_frac }),
        );
    }
    for (s, mut t) in &mut texts {
        let v = if s.0 == 0 {
            if status.active > 0.0 {
                format!("{:.0}", status.active.ceil())
            } else if status.tac_charges == 0 {
                format!("{:.0}", status.tac_cooldown.ceil())
            } else if def.charges > 1 {
                format!("x{}", status.tac_charges)
            } else {
                String::new()
            }
        } else if status.satchels > 0 {
            "DETONATE".to_string()
        } else {
            format!("x{}", status.ord_charges)
        };
        if t.0 != v {
            t.0 = v;
        }
    }
    if let Ok(mut bg) = tint.single_mut() {
        bg.0 = if *control != Control::Pilot {
            Color::NONE
        } else if status.phased() && std::env::var_os("TF_NO_PHASE_FX").is_some() {
            // (cloak::phase_shift_fx draws Phase Shift with the game's colour correction.)
            Color::srgba(0.25, 0.55, 1.0, 0.28)
        } else if status.cloaked() && std::env::var_os("TF_NO_CLOAK_FX").is_some() {
            // (cloak.rs draws the cloak on the arms and gun.)
            Color::srgba(0.6, 0.8, 1.0, 0.08)
        } else {
            Color::NONE
        };
    }
}

/// Mark enemies within the Pulse Blade's radius while it pulses.
pub fn update_pulse_markers(
    status: Res<PilotStatus>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    enemies: Query<&Enemy>,
    mut markers: Query<(&PulseMarker, &mut Node, &mut Visibility)>,
) {
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };
    let mut spots = Vec::new();
    if let Some((at, _)) = status.pulse {
        for e in &enemies {
            if e.alive() && e.pos.distance(at) < SONAR_RADIUS {
                if let Ok(p) =
                    camera.world_to_viewport(cam_tf, to_bevy(e.pos + Vec3::Z * e.height * 0.6))
                {
                    spots.push(p);
                }
            }
        }
    }
    for (m, mut n, mut v) in &mut markers {
        match spots.get(m.0) {
            Some(p) => {
                n.left = Val::Px(p.x);
                n.top = Val::Px(p.y);
                v.set_if_neq(Visibility::Inherited);
            }
            None => {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// The grapple's rope while the hook is out (flying, stuck or coming back): thin segments from
/// the left hand (beside and below the view) through the rope's bends to the hook, placed after
/// the camera moves so they don't trail a frame behind.
#[allow(clippy::type_complexity)]
pub fn grapple_cable(
    mut commands: Commands,
    pilots: Query<&PlayerPilot>,
    control: Res<Control>,
    cams: Query<&Transform, (With<crate::player::MainCamera>, Without<GrappleCable>)>,
    mut cables: Query<(&GrappleCable, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let rope: Vec<Vec3> = pilots
        .single()
        .ok()
        .filter(|_| *control == Control::Pilot)
        .map(|p| {
            p.state
                .rope()
                .iter()
                .map(|v| to_bevy(Vec3::from(v.to_array())))
                .collect()
        })
        .unwrap_or_default();
    if cables.is_empty() {
        // The hook flies straight; stuck, the rope has up to three bends (client.dll keeps four
        // points), so four segments.
        let mesh = meshes.add(Cylinder::new(1.0, 1.0));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.1, 0.1, 0.11),
            metallic: 0.2,
            perceptual_roughness: 0.6,
            ..default()
        });
        for i in 0..4 {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::default(),
                Visibility::Hidden,
                GrappleCable(i),
                bevy::light::NotShadowCaster,
            ));
        }
        return;
    }
    let Ok(cam) = cams.single() else {
        for (_, _, mut vis) in &mut cables {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    let u = crate::player::UNIT;
    // Camera space: x right, y up, -z forward; game units scaled to metres.
    let hand = cam.translation + cam.rotation * (Vec3::new(-7.0, -9.0, -16.0) * u);
    let points: Vec<Vec3> = std::iter::once(hand).chain(rope).collect();
    for (seg, mut tf, mut vis) in &mut cables {
        let i = seg.0;
        if i + 1 >= points.len() {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let (a, b) = (points[i], points[i + 1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        // The engine draws ropes no thinner than a pixel or so on screen (rope_solid_minwidth),
        // so the far end stays visible: the radius grows with the distance from the camera.
        let mid = a + d * 0.5;
        let r = (0.35 * u).max((mid - cam.translation).length() * 0.0012);
        *tf = Transform {
            translation: mid,
            rotation: Quat::from_rotation_arc(Vec3::Y, d / len),
            scale: Vec3::new(r, len, r),
        };
        vis.set_if_neq(Visibility::Visible);
    }
}

/// One segment of the grapple's rope (its index from the hand).
#[derive(Component)]
pub struct GrappleCable(pub usize);

/// Thrown ordnance and tactical models by path (None: failed to build).
#[derive(Resource, Default)]
pub struct GrenadeModels(
    pub std::collections::HashMap<String, Option<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>>,
);

/// Build every thrown model once, under the loading screen (built on the first throw, the
/// A-Wall's stalled that frame for 70 ms).
#[allow(clippy::too_many_arguments)]
pub fn preload_grenade_models(
    gd: Res<GameData>,
    mut cache: ResMut<Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    defs: Option<Res<PilotAbilityDefs>>,
    mut models: ResMut<GrenadeModels>,
    mut done: Local<bool>,
) {
    let Some(defs) = defs else { return };
    if *done {
        return;
    }
    *done = true;
    let paths: Vec<String> = defs
        .ordnance
        .iter()
        .map(|o| o.model.clone())
        .chain(defs.tactical.iter().map(|t| t.model.clone()))
        .filter(|m| !m.is_empty())
        .collect();
    for m in paths {
        models.0.entry(m.clone()).or_insert_with(|| {
            crate::world::build_static_model(
                &gd,
                &m,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
            )
            .ok()
            .map(|(parts, _)| parts)
        });
    }
}

/// A grenade's model and trail are attached.
#[derive(Component)]
pub struct GrenadeVisual;

/// Give each thrown grenade its script's `projectilemodel` (in place of the glow stand-in) and
/// `projectile_trail_effect_0`.
#[allow(clippy::too_many_arguments)]
pub fn grenade_visuals(
    mut commands: Commands,
    gd: Res<GameData>,
    mut cache: ResMut<Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    defs: Option<Res<PilotAbilityDefs>>,
    new: Query<(Entity, &Grenade), Without<GrenadeVisual>>,
    mut models: ResMut<GrenadeModels>,
) {
    let Some(defs) = defs else { return };
    let models = &mut models.0;
    for (e, g) in &new {
        commands.entity(e).insert(GrenadeVisual);
        let (model, trail) = match (g.kind, g.wall) {
            (Some(i), _) => (&defs.ordnance[i].model, &defs.ordnance[i].trail),
            (None, Some(_)) => {
                let t = &defs.tactical[TACTICALS
                    .iter()
                    .position(|t| t.0 == Tactical::AWall)
                    .unwrap_or(0)];
                (&t.model, &t.trail)
            }
            (None, None) => {
                let t = &defs.tactical[TACTICALS
                    .iter()
                    .position(|t| t.0 == Tactical::PulseBlade)
                    .unwrap_or(0)];
                (&t.model, &t.trail)
            }
        };
        if std::env::var_os("TF_OLD_FX").is_some() {
            continue;
        }
        if !trail.is_empty() {
            commands.entity(e).insert(crate::pfx::PfxTrail::new(trail));
        }
        if model.is_empty() {
            continue;
        }
        let parts = models
            .entry(model.clone())
            .or_insert_with(|| {
                match crate::world::build_static_model(
                    &gd,
                    model,
                    &mut cache,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                ) {
                    Ok((parts, _)) => Some(parts),
                    Err(err) => {
                        log::warn!("grenade model {model}: {err:#}");
                        None
                    }
                }
            })
            .clone();
        let Some(parts) = parts else { continue };
        commands
            .entity(e)
            .remove::<crate::particles::Glow>()
            .insert(Visibility::default());
        // The model is in game axes and units: turn it into Bevy's Y-up, metre space.
        let local = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::splat(crate::player::UNIT));
        let holder = commands.spawn((local, Visibility::default())).id();
        commands.entity(e).add_child(holder);
        for (mesh, mat) in parts {
            let c = commands
                .spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY))
                .id();
            commands.entity(holder).add_child(c);
        }
    }
}

/// What `update_decoy` keeps between frames: the gun it carries, whether that gun plays the
/// pistol sequences, the playing clip and its cycle.
#[derive(Default)]
pub struct DecoyRig {
    gun: Option<Entity>,
    pistol: bool,
    clip: Option<usize>,
    cycle: f32,
}

/// Move and animate the Holo Pilot (server.dll CPlayerDecoy think, 0x1801c8330). On the ground
/// it is moved by its animation's root motion at playback rate 1 along its facing, stepping up
/// to 18 units; at a wall it stays put and keeps animating; off a ledge it launches at 1.2 x its
/// speed and falls (gravity), landing into a run, sprint or slide. A slide lasts 2 s; a wallrun
/// speeds up to 350 and jumps off when the wall ends. It carries the player's gun
/// (`playermodel`, in `ja_c_propGun`) and plays the pistol sequences for a "pistol" one.
#[allow(clippy::too_many_arguments)]
pub fn update_decoy(
    mut commands: Commands,
    mut rig: Local<DecoyRig>,
    time: Res<Time>,
    world: Res<Collision>,
    settings: Res<crate::pilotctl::PilotSettings>,
    mut status: ResMut<PilotStatus>,
    kit: Option<Res<PilotKitAssets>>,
    loadouts: Query<&crate::pilotweapon::PilotLoadout>,
    mut actors: Query<&mut crate::actor::Actor>,
    mut decoys: Query<(&mut Transform, &mut Visibility), With<DecoyActor>>,
    gd: Res<GameData>,
    mut cache: ResMut<Cache>,
    // Grouped: the system is at Bevy's parameter limit.
    assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    ),
    mut dissolve: (
        Query<&Children>,
        Query<&MeshMaterial3d<StandardMaterial>>,
        Local<Vec<(Entity, Handle<StandardMaterial>, Handle<StandardMaterial>)>>,
    ),
) {
    let Some(kit) = kit else { return };
    let dt = time.delta_secs().min(0.1);
    let (mut meshes, mut images, mut materials, mut bindposes) = assets;
    let (children, mesh_mats, fading) = (&dissolve.0, &dissolve.1, &mut *dissolve.2);
    let Some(mut d) = status.decoy else {
        // The dissolve: the decoy (and its gun) stay, fading out, while the sparks run.
        if let (Some(t), Some(anchor)) = (status.decoy_dissolve, kit.decoy) {
            if fading.is_empty() && t == 0.0 {
                for e in children.iter_descendants(anchor) {
                    let Ok(m) = mesh_mats.get(e) else { continue };
                    let Some(src) = materials.get(&m.0).cloned() else {
                        continue;
                    };
                    let copy = materials.add(StandardMaterial {
                        alpha_mode: AlphaMode::Blend,
                        ..src
                    });
                    commands.entity(e).insert(MeshMaterial3d(copy.clone()));
                    fading.push((e, m.0.clone(), copy));
                }
            }
            let t = t + dt;
            let (a, b) = DECOY_DISSOLVE_FADE;
            let alpha = (1.0 - (t - a) / (b - a)).clamp(0.0, 1.0);
            for (_, _, copy) in fading.iter() {
                if let Some(m) = materials.get_mut(copy) {
                    m.base_color = m.base_color.with_alpha(alpha);
                }
            }
            if t >= DECOY_DISSOLVE_TOTAL {
                for (e, orig, _) in fading.drain(..) {
                    if let Ok(mut c) = commands.get_entity(e) {
                        c.insert(MeshMaterial3d(orig));
                    }
                }
                if let Ok((_, mut vis)) = decoys.single_mut() {
                    vis.set_if_neq(Visibility::Hidden);
                }
                status.decoy_dissolve = None;
            } else {
                status.decoy_dissolve = Some(t);
                return;
            }
        }
        if let Some(e) = rig.gun.take() {
            commands.entity(e).despawn();
        }
        rig.clip = None;
        return;
    };
    // The gun, once, from the weapon in hand.
    if rig.gun.is_none() {
        if let (Some(hand), Some(g)) = (
            kit.decoy_hand,
            loadouts.iter().next().and_then(|lo| lo.guns.get(lo.active)),
        ) {
            rig.pistol = g.def.activity_modifier == "pistol";
            let path = g.def.playermodel.clone();
            if !path.is_empty() {
                let spec = crate::actor::ActorSpec {
                    path: &path,
                    sequences: &[],
                    grids: &[],
                    body: &[],
                };
                match crate::actor::spawn_actor(
                    &mut commands,
                    hand,
                    &gd,
                    &spec,
                    &mut cache,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                    &mut bindposes,
                ) {
                    Ok(a) => rig.gun = Some(a.entity),
                    Err(e) => log::warn!("holo pilot gun {path}: {e:#}"),
                }
            }
        }
    }
    let p = &settings.0;
    let Ok(mut actor) = kit
        .decoy_actor
        .map_or(Err(()), |e| actors.get_mut(e).map_err(|_| ()))
    else {
        return;
    };
    let clip = actor
        .clip(d.clip(rig.pistol))
        .or_else(|| actor.clip(d.clip(false)));
    if clip != rig.clip {
        // A new activity starts its sequence from the beginning.
        rig.clip = clip;
        rig.cycle = 0.0;
    }
    let ground_speed = clip.map_or(0.0, |c| actor.clips[c].ground_speed);
    if let Some(c) = clip {
        rig.cycle = (rig.cycle + dt / actor.clips[c].duration.max(1e-3)).rem_euclid(1.0);
        actor.autoplay = false;
        actor.aim = None;
        actor.layers = vec![crate::actor::Layer {
            clip: c,
            cycle: rig.cycle,
            weight: 1.0,
        }];
    }
    let ray = |o: Vec3, dir: Vec3, len: f32| {
        world
            .0
            .raycast(SVec3::from(o.to_array()), SVec3::from(dir.to_array()), len)
            .map(|h| {
                (
                    Vec3::from(h.point.to_array()),
                    Vec3::from(h.normal.to_array()),
                )
            })
    };
    let blocked = |pos: Vec3, dir: Vec3, dist: f32| {
        ray(pos + Vec3::Z * (DECOY_STEP + 2.0), dir, dist + DECOY_HULL)
            .is_some_and(|(_, n)| n.z.abs() < 0.7)
    };
    let ground_below = |pos: Vec3, below: f32| {
        ray(pos + Vec3::Z * DECOY_STEP, -Vec3::Z, DECOY_STEP + below)
            .filter(|(_, n)| n.z > 0.7)
            .map(|(h, _)| h.z)
    };
    let right = Vec3::new(d.dir.y, -d.dir.x, 0.0);
    d.time -= dt;
    match d.state {
        DecoyState::Air => {
            if let Some(j) = d.jump.take() {
                d.vel.z = j;
            }
            d.vel.z -= p.gravity * dt;
            let flat = Vec3::new(d.vel.x, d.vel.y, 0.0);
            if flat.length() > 1e-3 && blocked(d.pos, flat.normalize(), flat.length() * dt) {
                d.vel.x = 0.0;
                d.vel.y = 0.0;
            }
            let next = d.pos + d.vel * dt;
            // Land (SurfaceTouch): ground under the feet while falling.
            let land = (d.vel.z <= 0.0)
                .then(|| ground_below(next, (-d.vel.z * dt).max(1.0)))
                .flatten();
            if let Some(z) = land.filter(|&z| z >= next.z - 1.0) {
                d.pos = Vec3::new(next.x, next.y, z);
                let speed = Vec2::new(d.vel.x, d.vel.y).length();
                d.state = if let Some(r) = d.strafe_right {
                    DecoyState::Strafe { right: r }
                } else if speed < p.sprint_speed {
                    DecoyState::Run
                } else if d.was_sliding {
                    DecoyState::Slide {
                        left: DECOY_SLIDE_TIME,
                    }
                } else {
                    DecoyState::Sprint
                };
                d.vel = Vec3::ZERO;
            } else {
                d.pos = next;
            }
        }
        DecoyState::WallRun { normal, speed } => {
            let speed = (speed + DECOY_WALLRUN_ACCEL * dt).min(DECOY_WALLRUN_MAX);
            let along = (d.dir - normal * d.dir.dot(normal)).normalize_or(d.dir);
            let on_wall = ray(d.pos + Vec3::Z * 16.0, -normal, 13.0 + DECOY_HULL + 4.7)
                .filter(|(_, n)| (-0.35..=0.7).contains(&n.z));
            if on_wall.is_none() || blocked(d.pos, along, speed * dt) {
                // Off the end of the wall: jump away from it (server.dll 0x1801c9350).
                let strength = p.wallrun_jump_input + p.wallrun_jump_out;
                let out = (d.dir + normal).normalize_or(normal);
                d.vel =
                    Vec3::new(out.x, out.y, 0.0) * strength + Vec3::Z * strength * (normal.z + 0.2);
                d.state = DecoyState::Air;
            } else {
                d.pos += along * speed * dt;
                d.state = DecoyState::WallRun { normal, speed };
            }
        }
        DecoyState::Idle | DecoyState::CrouchIdle | DecoyState::WallHang => {
            if d.state != DecoyState::WallHang && ground_below(d.pos, 4.0).is_none() {
                d.state = DecoyState::Air;
            }
        }
        _ => {
            // Ground states: root motion along the facing (sideways for a strafe).
            let (move_dir, speed) = match d.state {
                DecoyState::Strafe { right: r } | DecoyState::CrouchStrafe { right: r } => {
                    (if r { right } else { -right }, ground_speed)
                }
                DecoyState::Slide { left } => {
                    // Sprint speed x 1.5, slowing (cubic) over the last 1.25 s.
                    let into = (DECOY_SLIDE_SLOWDOWN - left).max(0.0) / DECOY_SLIDE_SLOWDOWN;
                    (d.dir, p.sprint_speed * (1.0 - into.powi(3)) * 1.5)
                }
                _ => (d.dir, ground_speed),
            };
            if let DecoyState::Slide { left } = d.state {
                d.state = if left - dt <= 0.0 {
                    DecoyState::CrouchIdle
                } else {
                    DecoyState::Slide { left: left - dt }
                };
            }
            let step = move_dir * speed * dt;
            if !blocked(d.pos, move_dir, step.length()) {
                let next = d.pos + step;
                if let Some(z) = ground_below(next, DECOY_STEP) {
                    d.pos = Vec3::new(next.x, next.y, z);
                } else {
                    // Off a ledge: launch at 1.2 x the state's speed.
                    let launch = match d.state {
                        DecoyState::Sprint => p.sprint_speed,
                        DecoyState::Slide { .. } => p.sprint_speed * 1.5,
                        _ => p.walk_speed,
                    };
                    d.strafe_right = match d.state {
                        DecoyState::Strafe { right: r } => Some(r),
                        _ => None,
                    };
                    d.pos = next;
                    d.vel = move_dir * launch * 1.2;
                    d.state = DecoyState::Air;
                }
            }
        }
    }
    if let Ok((mut tf, mut vis)) = decoys.single_mut() {
        *tf = game_tf(d.pos, d.dir.y.atan2(d.dir.x));
        vis.set_if_neq(Visibility::Inherited);
    }
    if d.time <= 0.0 {
        // CleanupExistingDecoy: Decoy_Dissolve, holopilot_end_3P, and the dissolve's own
        // sparks and sound (C_EntityDissolve) on the decoy, which keeps moving nowhere.
        status.decoy = None;
        status.decoy_dissolve = Some(0.0);
        crate::audio::event_at(&mut commands, "holopilot_end_3P", d.pos);
        crate::audio::event_at(&mut commands, "Death.Dissolve", d.pos);
        if let Some(anchor) = kit.decoy {
            commands.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 36.0)),
                crate::pfx::PfxTrail::oriented("dissolve"),
                crate::pfx::PfxOneShot(DECOY_DISSOLVE_TOTAL),
                ChildOf(anchor),
            ));
        }
    } else {
        status.decoy = Some(d);
    }
}
