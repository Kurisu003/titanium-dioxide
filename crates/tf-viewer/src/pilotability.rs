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

/// Stim's speed boost (STIM_EFFECT_SEVERITY with movement_speedboost_extraScale 2.0).
const STIM_SPEED: f32 = 1.4;
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
const GRAV_POP_DELAY: f32 = 0.8;
const GRAV_PULL_DELAY: f32 = 2.0;
const GRAV_POP_HEIGHT: f32 = 60.0;
const GRAV_PULL_RADIUS: f32 = 300.0;
const GRAV_PULL_ACCEL: f32 = 2000.0;
const GRAV_PULL_SPEED: f32 = 400.0;
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
    (Tactical::Stim, "mp_ability_heal", "rui/pilot_loadout/tactical/pilot_tactical_stim", "STIM", "pilot_stimpack_activate_1P"),
    (Tactical::Cloak, "mp_ability_cloak", "rui/pilot_loadout/tactical/pilot_tactical_cloak", "CLOAK", "cloak_on_1P"),
    (Tactical::PhaseShift, "mp_ability_shifter", "rui/pilot_loadout/tactical/pilot_tactical_phase_shift", "PHASE SHIFT", "Pilot_PhaseShift_Activate_1P"),
    (Tactical::Grapple, "mp_ability_grapple", "rui/pilot_loadout/tactical/pilot_tactical_grapple", "GRAPPLE", "pilot_grapple_fire"),
    (Tactical::PulseBlade, "mp_weapon_grenade_sonar", "rui/pilot_loadout/tactical/pilot_tactical_pulse_blade", "PULSE BLADE", "Pilot_PulseBlade_Throw_1P"),
    (Tactical::HoloPilot, "mp_ability_holopilot", "rui/pilot_loadout/tactical/pilot_tactical_holo_pilot", "HOLO PILOT", "holopilot_deploy_1p"),
    (Tactical::AWall, "mp_weapon_deployable_cover", "rui/pilot_loadout/tactical/pilot_tactical_hardcover", "A-WALL", "Pilot_Hardcover_Toss_1P"),
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
    (Ordnance::Frag, "mp_weapon_frag_grenade", "rui/pilot_loadout/ordnance/frag", "FRAG GRENADE", "Weapon_FragGrenade_Throw", "Explo_FragGrenade_Impact_3P"),
    (Ordnance::Arc, "mp_weapon_grenade_emp", "rui/pilot_loadout/ordnance/arc_grenade", "ARC GRENADE", "weapon_empgrenade_throw", "explo_proximityemp_impact_3p"),
    (Ordnance::Firestar, "mp_weapon_thermite_grenade", "rui/pilot_loadout/ordnance/firestar", "FIRESTAR", "weapon_firestar_throw_1p", "explo_firestar_impact"),
    (Ordnance::GravityStar, "mp_weapon_grenade_gravity", "rui/pilot_loadout/ordnance/gravity_grenade", "GRAVITY STAR", "weapon_gravitystar_throw_1p", "gravitystar_explo_3p"),
    (Ordnance::ElectricSmoke, "mp_weapon_grenade_electric_smoke", "rui/pilot_loadout/ordnance/electric_smoke", "ELECTRIC SMOKE", "weapon_electric_smoke_throw_1p", "explo_electric_smoke_impact"),
    (Ordnance::Satchel, "mp_weapon_satchel", "rui/pilot_loadout/ordnance/satchel", "SATCHEL", "weapon_r1_satchel_throw", "explo_satchel_impact_3p"),
];

pub struct TacticalDef {
    pub duration: f32,
    /// Seconds per charge.
    pub cooldown: f32,
    pub charges: u32,
    /// Throw speed (Pulse Blade, A-Wall) or grapple reach.
    pub speed: f32,
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
    s.get(&format!("mp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).and_then(|v| v.parse().ok()).unwrap_or(default)
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
        let pilot = PlayerSettings::load("scripts/players/mp/pilot_base.set", false, read).unwrap_or_default();
        let mut load = |w: &str| PlayerSettings::load(&format!("scripts/weapons/{w}.txt"), false, read).unwrap_or_default();
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
                    viewmodel: if thrown { s.get(".viewmodel").unwrap_or("").trim().to_string() } else { String::new() },
                    pullout: mp(&s, "toss_pullout_time", 0.2),
                    toss: mp(&s, "toss_time", 0.3),
                    model: s.get(".projectilemodel").unwrap_or("").trim().to_string(),
                    trail: s.get(".projectile_trail_effect_0").unwrap_or("").trim().to_string(),
                };
                match t.0 {
                    Tactical::Grapple => {
                        // Grapple power, not ammo: each shot needs grapple_power_required of 100.
                        let need = mp(&s, "grapple_power_required", 50.0).max(1.0);
                        let rate = pilot.f32(".grapple_power_regen_rate", 3.0).max(0.1);
                        d.charges = (100.0 / need) as u32;
                        d.cooldown = need / rate;
                        d.duration = 0.0;
                        d.speed = mp(&s, "grapple_maxLength", 1100.0);
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
                    trail: s.get(".projectile_trail_effect_0").unwrap_or("").trim().to_string(),
                };
                if o.0 == Ordnance::Satchel {
                    d.speed = SATCHEL_THROW;
                }
                d
            })
            .collect();
        let tacs: Vec<String> = TACTICALS.iter().zip(tactical.iter()).map(|(t, d)| format!("{:?} {:.1}s x{} /{:.1}s", t.0, d.duration, d.charges, d.cooldown)).collect();
        let ords: Vec<String> = ORDNANCE
            .iter()
            .zip(ordnance.iter())
            .map(|(o, d)| format!("{:?} {:.0}/{:.0} r{:.0} x{} /{:.1}s", o.0, d.damage, d.damage_heavy, d.radius, d.charges, d.cooldown))
            .collect();
        log::info!("pilot tacticals: {}", tacs.join(", "));
        log::info!("pilot ordnance: {}", ords.join(", "));
        Self { tactical, ordnance }
    }
}

/// The Holo Pilot decoy.
#[derive(Clone, Copy)]
pub struct Decoy {
    pub pos: Vec3,
    pub dir: Vec3,
    pub time: f32,
}

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
    /// Index into ORDNANCE.
    pub ordnance: usize,
    pub ord_charges: u32,
    /// Seconds until the next ordnance charge.
    pub ord_cooldown: f32,
    pub decoy: Option<Decoy>,
    /// Pulse Blade: where it landed and how long it keeps revealing.
    pub pulse: Option<(Vec3, f32)>,
    /// Satchels waiting for the detonator.
    pub satchels: u32,
}

impl PilotStatus {
    /// Full charges for the current choices (new game, or a loadout change).
    pub fn refill(&mut self, defs: &PilotAbilityDefs) {
        self.tac_charges = defs.tactical[self.tactical].charges;
        self.tac_cooldown = 0.0;
        self.ord_charges = defs.ordnance[self.ordnance].charges;
        self.ord_cooldown = 0.0;
        self.active = 0.0;
    }
    fn is(&self, t: Tactical) -> bool {
        self.active > 0.0 && TACTICALS[self.tactical].0 == t
    }
    pub fn speed_scale(&self) -> f32 {
        if self.is(Tactical::Stim) {
            STIM_SPEED
        } else {
            1.0
        }
    }
    pub fn cloaked(&self) -> bool {
        self.is(Tactical::Cloak)
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
pub fn enemy_target(player: Option<(Vec3, f32, f32, bool)>, status: &PilotStatus, from: Vec3) -> (Option<(Vec3, f32, f32, bool)>, bool) {
    if let Some(d) = status.decoy {
        let decoy = (d.pos, 16.0, 72.0, true);
        let prefer = match player {
            None => true,
            Some(p) => (p.3 && !status.visible_at(p.0.distance(from))) || d.pos.distance(from) < p.0.distance(from),
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
            let (a, b, c) = (d.length_squared(), 2.0 * o.dot(d), o.length_squared() - SHIELD_RADIUS * SHIELD_RADIUS);
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
            for t in [(-b - disc.sqrt()) / (2.0 * a), (-b + disc.sqrt()) / (2.0 * a)] {
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
    /// The Holo Pilot's anchor (hidden until used).
    decoy: Option<Entity>,
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
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
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
    // The decoy: a Pilot in the reaper suit, running.
    let anchor = commands.spawn((game_tf(Vec3::ZERO, 0.0), Visibility::Hidden, DecoyActor)).id();
    let spec = crate::actor::ActorSpec { path: "models/humans/pilots/pilot_medium_reaper_m.mdl", sequences: &["Run_forward_mp"], grids: &[], body: &[] };
    let decoy = match crate::actor::spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
        Ok(_) => Some(anchor),
        Err(e) => {
            log::warn!("holo pilot model: {e:#}");
            None
        }
    };
    commands.insert_resource(PilotKitAssets { shield_mesh, shield_mat, decoy });
}

/// A game-space placement (feet position, yaw about Z) as a Bevy world transform (the world
/// root's Z-up to Y-up turn and unit scale applied).
fn game_tf(pos: Vec3, yaw: f32) -> Transform {
    let root = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(crate::player::UNIT));
    root.mul_transform(Transform::from_translation(pos).with_rotation(Quat::from_rotation_z(yaw)))
}

fn view_dir(s: &tf_sim::pilot::PilotState, pitch_offset_deg: f32) -> Vec3 {
    let pitch = s.pitch - pitch_offset_deg.to_radians();
    Vec3::new(pitch.cos() * s.yaw.cos(), pitch.cos() * s.yaw.sin(), -pitch.sin())
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
    world: Res<Collision>,
    kit: Option<Res<PilotKitAssets>>,
    mut status: ResMut<PilotStatus>,
    mut shields: ResMut<Shields>,
    mut input: ResMut<PlayerInput>,
    mut pilots: Query<(&mut PlayerPilot, &mut PilotHealth)>,
    mut decoys: Query<(&mut Transform, &mut Visibility), With<DecoyActor>>,
    mut throw_state: ResMut<OrdnanceThrow>,
) {
    let dt = time.delta_secs().min(0.1);
    let mut tactical = std::mem::take(&mut input.tactical);
    let throw = std::mem::take(&mut input.throw);
    let ti = status.tactical.min(defs.tactical.len() - 1);
    let oi = status.ordnance.min(defs.ordnance.len() - 1);
    let def = &defs.tactical[ti];
    let odef = &defs.ordnance[oi];
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
    // Charges refill one at a time (not while a timed tactical runs).
    if status.tac_charges < def.charges && status.active <= 0.0 {
        status.tac_cooldown -= dt;
        if status.tac_cooldown <= 0.0 {
            status.tac_charges += 1;
            status.tac_cooldown = if status.tac_charges < def.charges { def.cooldown } else { 0.0 };
            if status.tac_charges == def.charges && TACTICALS[ti].0 == Tactical::Grapple {
                crate::audio::event(&mut commands, "pilot_grapple_ready");
            }
        }
    }
    if status.ord_charges < odef.charges {
        status.ord_cooldown -= dt;
        if status.ord_cooldown <= 0.0 {
            status.ord_charges += 1;
            status.ord_cooldown = if status.ord_charges < odef.charges { odef.cooldown } else { 0.0 };
        }
    }
    if let Some((_, t)) = status.pulse.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            status.pulse = None;
        }
    }

    // The decoy runs on along the ground until its time is up or it hits a wall.
    if let Some(mut d) = status.decoy {
        d.time -= dt;
        let run = d.dir * settings.0.sprint_speed;
        let ahead = world.0.raycast(SVec3::from((d.pos + Vec3::Z * 40.0).to_array()), SVec3::from(d.dir.to_array()), 40.0);
        if ahead.is_none() {
            d.pos += run * dt;
        }
        if let Some(h) = world.0.raycast(SVec3::from((d.pos + Vec3::Z * 40.0).to_array()), -SVec3::Z, 400.0) {
            d.pos.z = h.point.z;
        }
        if let Ok((mut tf, mut vis)) = decoys.single_mut() {
            *tf = game_tf(d.pos, d.dir.y.atan2(d.dir.x));
            *vis = if d.time > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        }
        if d.time <= 0.0 {
            status.decoy = None;
            crate::audio::event_at(&mut commands, "holopilot_end_3P", d.pos);
        } else {
            status.decoy = Some(d);
        }
    }
    // Shields wear out.
    for s in &mut shields.0 {
        s.time -= dt;
    }
    shields.0.retain(|s| {
        let keep = s.time > 0.0 && s.health > 0.0;
        if !keep {
            log::info!("a-wall down with {:.0} of {SHIELD_HEALTH:.0} health left", s.health.max(0.0));
            commands.entity(s.entity).despawn();
            crate::audio::event_at(&mut commands, "Hardcover_Shield_End_3P", s.center);
        }
        keep
    });

    let Ok((mut pilot, mut health)) = pilots.single_mut() else { return };
    if *control != Control::Pilot || health.dead() {
        pilot.state.grapple = None;
        return;
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
    let (pullout, toss) = if throw_state.tactical { (def.pullout, def.toss) } else { (odef.pullout, odef.toss) };
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
    let thrown_tac = matches!(TACTICALS[ti].0, Tactical::PulseBlade | Tactical::AWall) && !def.viewmodel.is_empty();
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
        if kind == Tactical::Grapple && pilot.state.grapple.is_some() {
            pilot.state.release_grapple(&settings.0);
            crate::audio::event(&mut commands, "pilot_grapple_retract_1p");
        } else if status.tac_charges > 0 && status.active <= 0.0 {
            spend(&mut status);
            crate::audio::event(&mut commands, TACTICALS[ti].4);
            log::info!("tactical: {kind:?}");
            match kind {
                Tactical::Stim | Tactical::Cloak | Tactical::PhaseShift => status.active = def.duration,
                Tactical::Grapple => {
                    match world.0.raycast(SVec3::from(eye.to_array()), SVec3::from(look.to_array()), def.speed) {
                        Some(h) => {
                            let at = Vec3::from(h.point.to_array());
                            log::info!("grapple hooked {:.0} units away", h.t);
                            pilot.state.attach_grapple(SVec3::from(at.to_array()));
                            crate::audio::event_at(&mut commands, "default_grapple_impact_1p_vs_3p", at);
                            emit(&mut commands, Effect::Impact { at: to_bevy(at), normal: to_bevy(Vec3::from(h.normal.to_array())), scale: 0.5, energy: false });
                        }
                        // Out of reach: the hook comes straight back.
                        None => crate::audio::event(&mut commands, "pilot_grapple_retract_1p"),
                    }
                }
                Tactical::PulseBlade => {
                    let dir = view_dir(&pilot.state, 2.0);
                    commands.spawn((
                        Transform::from_translation(to_bevy(eye)),
                        crate::particles::Glow::new(Vec3::new(0.6, 2.0, 5.0), 0.12, false),
                        Grenade { kind: None, pos: eye + dir * 20.0, vel: dir * def.speed + pvel, fuse: 0.0, stuck: Stuck::No, since: 0.0, tick: 0.0, first: true, wall: None, spin: 0.0 },
                    ));
                }
                Tactical::HoloPilot => {
                    let dir = Vec3::new(pilot.state.yaw.cos(), pilot.state.yaw.sin(), 0.0);
                    let at = Vec3::from(pilot.state.pos.to_array()) + dir * 40.0;
                    status.decoy = Some(Decoy { pos: at, dir, time: def.duration });
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
                    let power = DEPLOYABLE_THROW_POWER * (1.0 + 2.0 * (pitch / 80.0).clamp(0.0, 1.0));
                    let p = pitch.to_radians();
                    let dir = Vec3::new(p.cos() * pilot.state.yaw.cos(), p.cos() * pilot.state.yaw.sin(), -p.sin());
                    commands.spawn((
                        Transform::from_translation(to_bevy(eye)),
                        crate::particles::Glow::new(Vec3::new(0.6, 2.0, 5.0), 0.12, false),
                        Grenade { kind: None, pos: eye + look * 15.0, vel: dir * power, fuse: 0.0, stuck: Stuck::No, since: 0.0, tick: 0.0, first: true, wall: Some(pilot.state.yaw), spin: 0.0 },
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
                Grenade { kind: Some(oi), pos: eye + dir * 20.0, vel: dir * odef.speed + pvel, fuse: if odef.fuse > 0.0 { (odef.fuse - throw_state.cooked).max(0.05) } else { 0.0 }, stuck: Stuck::No, since: 0.0, tick: 0.0, first: true, wall: None, spin: 0.0 },
            ));
        }
    }
}

/// Satchels go off this frame.
#[derive(Resource)]
pub struct Detonate;

fn radius_damage(enemies: &mut Query<(Entity, &mut Enemy)>, at: Vec3, inner: f32, radius: f32, dmg: f32, heavy: f32) -> u32 {
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
                e.damage(amount, false);
                hits += 1;
                log::debug!("ordnance hit {} for {amount:.0}", if e.infantry { "grunt" } else { "titan" });
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
    let self_damage = |pilots: &mut Query<(&PlayerPilot, &mut PilotHealth)>, at: Vec3, inner: f32, radius: f32, dmg: f32| {
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
            g.vel.z -= GRENADE_GRAVITY * if g.wall.is_some() { AWALL_GRAVITY_SCALE } else { 1.0 } * dt;
            let step = g.vel * dt;
            let len = step.length();
            if len > 1e-4 {
                let dir = step / len;
                let mut t_end = len + GRENADE_RADIUS;
                let mut normal = None;
                if let Some(h) = world.0.raycast(SVec3::from(g.pos.to_array()), SVec3::from(dir.to_array()), t_end) {
                    t_end = h.t;
                    normal = Some(Vec3::from(h.normal.to_array()));
                }
                // Direct hits on enemies (not for the bouncing frag).
                let mut hit_enemy = None;
                if kind != Some(Ordnance::Frag) {
                    for (eid, e) in enemies.iter() {
                        if e.alive() {
                            if let Some(t) = crate::weapons::ray_cylinder(g.pos, dir, e.pos, e.radius + 4.0, e.height) {
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
                        let keep = d.bounce_shallow + (d.bounce_sharp - d.bounce_shallow) * into.clamp(0.0, 1.0);
                        let v = g.vel - 2.0 * g.vel.dot(n) * n;
                        g.vel = v * keep;
                        g.pos += dir * (t_end - GRENADE_RADIUS).max(0.0) + n * 0.5;
                    } else {
                        g.pos += dir * (t_end - GRENADE_RADIUS).max(0.0) + n * 1.0;
                        g.stuck = Stuck::World;
                        g.since = 0.0;
                        if kind == Some(Ordnance::Satchel) {
                            crate::audio::event_at(&mut commands, "weapon_r1_satchel_attach", g.pos);
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
                    kit.as_ref().map(|k| commands.spawn((Mesh3d(k.shield_mesh.clone()), MeshMaterial3d(k.shield_mat.clone()), game_tf(center, yaw), ShieldMesh)).id())
                } else {
                    Some(commands.spawn((Transform::from_translation(to_bevy(center)).looking_to(to_bevy(fwd).normalize(), Vec3::Y), Visibility::default(), crate::pfx::PfxTrail::oriented("P_pilot_amped_shield"))).id())
                };
                if let Some(e) = e {
                    log::info!("a-wall up at {center:.0}");
                    shields.0.push(Shield { center, facing: fwd, health: SHIELD_HEALTH, time: SHIELD_DURATION, entity: e });
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
                emit(&mut commands, Effect::DustRing { at: to_bevy(g.pos), radius: 6.0 });
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
                    emit(&mut commands, Effect::Explosion { at: to_bevy(g.pos), scale: 1.2 });
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(&mut enemies, g.pos, d.inner, d.radius, d.damage, d.damage_heavy);
                    self_damage(&mut pilots, g.pos, d.inner, d.radius, d.damage);
                    log::info!("frag exploded, {n} enemies hit");
                }
            }
            Ordnance::Arc => {
                if landed {
                    commands.entity(id).despawn();
                    emit(&mut commands, Effect::Impact { at: to_bevy(g.pos), normal: Vec3::Y, scale: 4.0, energy: true });
                    emit(&mut commands, Effect::DustRing { at: to_bevy(g.pos), radius: d.radius * crate::player::UNIT });
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(&mut enemies, g.pos, d.inner, d.radius, d.damage, d.damage_heavy);
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
                        radius_damage(&mut enemies, g.pos, d.inner, d.radius, d.damage * m, d.damage_heavy * mh);
                        emit(&mut commands, Effect::TrailPuff { at: to_bevy(g.pos + Vec3::Z * 10.0), scale: 1.5 });
                        emit(&mut commands, Effect::Impact { at: to_bevy(g.pos), normal: Vec3::Y, scale: 2.0, energy: false });
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
                            crate::audio::event_at(&mut commands, "weapon_gravitystar_preexplo", g.pos);
                        }
                        // Pull grunts in (the intense pull: accel 2000 up to 400 u/s).
                        for (_, mut e) in enemies.iter_mut() {
                            if e.alive() && e.infantry {
                                let to = g.pos - (e.pos + Vec3::Z * 36.0);
                                let dist = to.length();
                                if dist < GRAV_PULL_RADIUS && dist > 20.0 {
                                    let speed = (GRAV_PULL_ACCEL * (g.since - GRAV_POP_DELAY)).min(GRAV_PULL_SPEED);
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
                        emit(&mut commands, Effect::Explosion { at: to_bevy(g.pos), scale: 1.0 });
                        crate::audio::event_at(&mut commands, sound, g.pos);
                        let n = radius_damage(&mut enemies, g.pos, d.inner.min(d.radius - 1.0), d.radius, d.damage, d.damage_heavy);
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
                        emit(&mut commands, Effect::TrailPuff { at: to_bevy(p), scale: 4.0 });
                        emit(&mut commands, Effect::Impact { at: to_bevy(p), normal: Vec3::Y, scale: 1.0, energy: true });
                    }
                    g.tick -= dt;
                    while g.tick <= 0.0 {
                        g.tick += SMOKE_TICK;
                        radius_damage(&mut enemies, g.pos, d.inner, d.radius, d.damage * SMOKE_TICK, d.damage_heavy * SMOKE_TICK);
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
                    emit(&mut commands, Effect::Explosion { at: to_bevy(g.pos), scale: 1.5 });
                    crate::audio::event_at(&mut commands, sound, g.pos);
                    let n = radius_damage(&mut enemies, g.pos, d.inner, d.radius, d.damage, d.damage_heavy);
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

pub fn spawn_pilot_slots(mut commands: Commands, ui: Res<UiAssets>, status: Res<PilotStatus>, hud: Query<Entity, With<crate::hud::HudRoot>>) {
    let vh = Val::Vh;
    let dim = Color::srgba(1.0, 1.0, 1.0, 0.55);
    let root = commands
        .spawn((Node { position_type: PositionType::Absolute, left: vh(4.0), bottom: vh(4.0), column_gap: vh(1.2), ..default() }, Visibility::Hidden, PilotSlots))
        .id();
    // Part of the cockpit HUD: sways and powers on/off with it.
    if let Ok(h) = hud.single() {
        commands.entity(h).add_child(root);
    }
    let tac = TACTICALS[status.tactical];
    let ord = ORDNANCE[status.ordnance];
    for (i, (icon, key, label)) in [(tac.2, "Q", tac.3), (ord.2, "G", ord.3)].into_iter().enumerate() {
        let col = commands.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.4), ..default() }).id();
        let frame = commands
            .spawn((Node { width: vh(7.0), height: vh(7.0), border: UiRect::all(Val::Px(1.0)), overflow: Overflow::clip(), ..default() }, BorderColor::all(dim), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4))))
            .id();
        let ic = commands
            .spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ui.image(icon).unwrap_or_default(), PilotSlotIcon(i)))
            .id();
        let shade = commands
            .spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(0.0), height: Val::Percent(0.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)), PilotSlotShade(i)))
            .id();
        let state = commands
            .spawn((Text::new(""), TextFont { font: ui.bold_font.clone(), ..default() }, TextColor(Color::srgb(1.0, 0.75, 0.25)), crate::ui::VhText(1.4), Node { position_type: PositionType::Absolute, bottom: vh(0.3), ..default() }, PilotSlotText(i)))
            .id();
        commands.entity(frame).add_children(&[ic, shade, state]);
        let k = commands.spawn((Text::new(key), TextFont { font: ui.bold_font.clone(), ..default() }, TextColor(Color::WHITE), crate::ui::VhText(1.5))).id();
        let l = commands.spawn((Text::new(label), TextFont { font: ui.font.clone(), ..default() }, TextColor(dim), crate::ui::VhText(1.0), PilotSlotLabel(i))).id();
        commands.entity(col).add_children(&[frame, k, l]);
        commands.entity(root).add_child(col);
    }
    // Phase Shift's blue wash and Cloak's faint shimmer.
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::NONE),
        PhaseTint,
    ));
    // Pulse Blade reveals: red diamonds over enemies, seen through walls.
    for i in 0..PULSE_MARKERS {
        commands.spawn((
            Node { position_type: PositionType::Absolute, width: vh(1.4), height: vh(1.4), margin: UiRect { left: vh(-0.7), top: vh(-0.7), ..default() }, border: UiRect::all(Val::Px(2.0)), ..default() },
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
    let playing = matches!(game.state, crate::game::GameState::Playing | crate::game::GameState::Intermission) && !hidden.0;
    if let Ok(mut v) = root.single_mut() {
        *v = if *control == Control::Pilot && playing { Visibility::Inherited } else { Visibility::Hidden };
    }
    let def = &defs.tactical[status.tactical.min(defs.tactical.len() - 1)];
    let odef = &defs.ordnance[status.ordnance.min(defs.ordnance.len() - 1)];
    let tac_frac = if status.active > 0.0 || status.tac_charges > 0 { 0.0 } else { (status.tac_cooldown / def.cooldown.max(0.1)).clamp(0.0, 1.0) };
    let ord_frac = if status.ord_charges > 0 { 0.0 } else { (status.ord_cooldown / odef.cooldown.max(0.1)).clamp(0.0, 1.0) };
    for (s, mut n) in &mut shades {
        n.height = Val::Percent(100.0 * if s.0 == 0 { tac_frac } else { ord_frac });
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
        } else if status.phased() {
            Color::srgba(0.25, 0.55, 1.0, 0.28)
        } else if status.cloaked() {
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
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let mut spots = Vec::new();
    if let Some((at, _)) = status.pulse {
        for e in &enemies {
            if e.alive() && e.pos.distance(at) < SONAR_RADIUS {
                if let Ok(p) = camera.world_to_viewport(cam_tf, to_bevy(e.pos + Vec3::Z * e.height * 0.6)) {
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
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
}

/// The grapple's cable while hooked: a thin line from the left hand (beside and below the view)
/// to the hook, placed after the camera moves so it doesn't trail a frame behind.
#[allow(clippy::type_complexity)]
pub fn grapple_cable(
    mut commands: Commands,
    pilots: Query<&PlayerPilot>,
    control: Res<Control>,
    cams: Query<&Transform, (With<crate::player::MainCamera>, Without<GrappleCable>)>,
    mut cables: Query<(&mut Transform, &mut Visibility), With<GrappleCable>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hook = pilots.single().ok().and_then(|p| p.state.grapple).filter(|_| *control == Control::Pilot);
    let Ok((mut tf, mut vis)) = cables.single_mut() else {
        let mesh = meshes.add(Cylinder::new(1.0, 1.0));
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.1, 0.11), metallic: 0.2, perceptual_roughness: 0.6, ..default() });
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::default(), Visibility::Hidden, GrappleCable, bevy::light::NotShadowCaster));
        return;
    };
    let (Some(hook), Ok(cam)) = (hook, cams.single()) else {
        *vis = Visibility::Hidden;
        return;
    };
    let u = crate::player::UNIT;
    // Camera space: x right, y up, -z forward; game units scaled to metres.
    let hand = cam.translation + cam.rotation * (Vec3::new(-7.0, -9.0, -16.0) * u);
    let end = to_bevy(Vec3::from(hook.to_array()));
    let d = end - hand;
    let len = d.length();
    if len < 1e-3 {
        *vis = Visibility::Hidden;
        return;
    }
    *tf = Transform { translation: hand + d * 0.5, rotation: Quat::from_rotation_arc(Vec3::Y, d / len), scale: Vec3::new(0.35 * u, len, 0.35 * u) };
    *vis = Visibility::Visible;
}

#[derive(Component)]
pub struct GrappleCable;

/// Thrown ordnance and tactical models by path (None: failed to build).
#[derive(Resource, Default)]
pub struct GrenadeModels(pub std::collections::HashMap<String, Option<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>>);

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
    let paths: Vec<String> = defs.ordnance.iter().map(|o| o.model.clone()).chain(defs.tactical.iter().map(|t| t.model.clone())).filter(|m| !m.is_empty()).collect();
    for m in paths {
        models.0.entry(m.clone()).or_insert_with(|| crate::world::build_static_model(&gd, &m, &mut cache, &mut meshes, &mut images, &mut materials).ok().map(|(parts, _)| parts));
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
                let t = &defs.tactical[TACTICALS.iter().position(|t| t.0 == Tactical::AWall).unwrap_or(0)];
                (&t.model, &t.trail)
            }
            (None, None) => {
                let t = &defs.tactical[TACTICALS.iter().position(|t| t.0 == Tactical::PulseBlade).unwrap_or(0)];
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
            .or_insert_with(|| match crate::world::build_static_model(&gd, model, &mut cache, &mut meshes, &mut images, &mut materials) {
                Ok((parts, _)) => Some(parts),
                Err(err) => {
                    log::warn!("grenade model {model}: {err:#}");
                    None
                }
            })
            .clone();
        let Some(parts) = parts else { continue };
        commands.entity(e).remove::<crate::particles::Glow>().insert(Visibility::default());
        // The model is in game axes and units: turn it into Bevy's Y-up, metre space.
        let local = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(crate::player::UNIT));
        let holder = commands.spawn((local, Visibility::default())).id();
        commands.entity(e).add_child(holder);
        for (mesh, mat) in parts {
            let c = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY)).id();
            commands.entity(holder).add_child(c);
        }
    }
}
