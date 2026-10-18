//! BT's loadout kits: each primary brings its own defensive (Q), utility (E), ordnance (G) and core (V), as
//! in the campaign. Which abilities a loadout has comes from the game's
//! `datatable/titan_properties.rpak` (the row whose `primary` is the loadout's weapon); the
//! numbers come from the ability scripts (SP_BASE values where a script has them).
//!
//! - Expedition (XO-16) and Brute (Quad Rocket): Multi-Target Missiles and the Vortex Shield
//!   (abilities.rs / combat.rs); Burst Core, and Flight Core for Brute.
//! - Ion: Laser Shot and the energy Vortex Shield on the shared 1000-point energy pool
//!   (`sharedEnergyRegenRate` 100/s after 0.2 s), Laser Core.
//! - Scorch: Firewall (a wave of thermite), Thermal Shield, Flame Core.
//! - Ronin: Arc Wave, Sword Block, Sword Core.
//! - Tone: Tracker Rockets on 40mm lock stacks, Particle Wall, Salvo Core.
//! - Northstar: Cluster Missile, Tether Trap, Flight Core.
//! - Legion: Power Shot, Gun Shield, Smart Core.
//!
//! The utility slot is the table's `antirodeo` column: Electric Smoke (Expedition), Sonar Pulse
//! (Tone), Slow Trap (Scorch), Hover (Brute, Northstar), Laser Tripwire (Ion), Phase Dash
//! (Ronin), Ammo Swap (Legion). Cooldowns are `ammo_per_shot / regen_ammo_refill_rate`,
//! charges `ammo_clip_size / ammo_per_shot`, durations `fire_duration`.
//!
//! Waves (Firewall, Arc Wave, Flame Core) follow `WeaponAttackWave`: one segment every
//! `wave_step_dist` per server frame (taken as 0.1 s, the same 10 Hz the NPC fire cap uses),
//! up to `wave_max_count` segments, stopping at walls and drops.

use crate::abilities::{spawn_missile, CoreState, MissileAssets, MissileSpec, Ordnance, TitanCore};
use crate::combat::{TitanHealth, Vortex, VortexMode};
use crate::gamedata::GameData;
use crate::particles::{emit, Effect, Glow};
use crate::pfx::Cp;
use crate::pilotctl::Control;
use crate::player::{to_bevy, CameraMode, Collision, PlayerInput, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use crate::weapons::{ray_cylinder, TITAN_ARSENAL};
use bevy::prelude::*;
use std::collections::HashMap;
use tf_assets::settings::PlayerSettings;
use tf_sim::glam::Vec3 as SVec3;

/// One server frame (WaitFrame in the wave and thermite scripts).
const SERVER_FRAME: f32 = 0.1;

// Utility slot numbers from the ability scripts (mp_titanability_*.nut).
/// Electric Smoke: SmokescreenStruct's lifetime is engine-side; 7 s is an approximation.
const TITAN_SMOKE_LIFETIME: f32 = 7.0;
const TITAN_SMOKE_INNER: f32 = 320.0;
const TITAN_SMOKE_OUTER: f32 = 375.0;
const TITAN_SMOKE_DPS_PILOT: f32 = 45.0;
const TITAN_SMOKE_DPS_TITAN: f32 = 450.0;
const SONAR_PULSE_DURATION: f32 = 5.0;
/// SONAR_PULSE_RADIUS (mp_titanability_sonar_pulse.nut).
pub const SONAR_PULSE_RADIUS: f32 = 1250.0;
/// mp_titanability_sonar_pulse.txt: projectile_launch_speed 5000, projectile_gravity_scale 3 (of
/// the 750 u/s² projectile gravity).
const SONAR_SHOT_SPEED: f32 = 5000.0;
/// FireGenericBoltWithDrop (_weapon_utility.nut) sets the bolt's gravity to 1, over the weapon's
/// projectile_gravity_scale 3.
const SONAR_SHOT_GRAVITY: f32 = DEPLOYABLE_GRAVITY;
const PHASE_DASH_SPEED: f32 = 1000.0;
const PHASE_DASH_TIME: f32 = 1.0;
const PHASE_FADE: f32 = 0.5;
const HOVER_LERP_IN: f32 = 0.5;
const HOVER_RISE: f32 = 450.0;
/// mp_titanability_hover.nut horizontalVelocity (350 with the Northstar passive).
const HOVER_HORIZ: f32 = 250.0;
/// PROTO_FlightCore horizontalVelocity (350 with PAS_NORTHSTAR_FLIGHTCORE), and its takeoff
/// before the rockets deploy.
const FLIGHT_CORE_HORIZ: f32 = 200.0;
const FLIGHT_CORE_TAKEOFF: f32 = 1.0;
const HOVER_FADE: f32 = 0.75;
const SLOW_TRAP_LIFETIME: f32 = 12.0;
const SLOW_TRAP_BUILD_TIME: f32 = 1.0;
const SLOW_TRAP_RADIUS: f32 = 240.0;
/// The gas's move_slow strength (its status effect severity isn't in the script; approximate).
/// Power Shot modes (the Predator Cannon's CloseRangePowerShot / LongRangePowerShot mods).
const POWER_SHOT_CLOSE_SPREAD: f32 = 16.0;
const POWER_SHOT_CLOSE_NEAR: f32 = 800.0;
const POWER_SHOT_CLOSE_FAR: f32 = 1600.0;
const POWER_SHOT_CLOSE_FAR_PILOT: f32 = 50.0;
const POWER_SHOT_SPLASH_HEAVY: f32 = 1800.0;
/// mp_titanweapon_40mm.nut's TRACKER_LIFETIME in SP (MP: 15): how long each tracker mark
/// lasts. BT carries the SP loadouts.
const TRACKER_LIFETIME: f32 = 60.0;
/// Tracker marks per target: the tracker HUD's art and the rockets go up to three.
pub const TRACKER_MAX: usize = 3;
/// SmartAmmo_SetMissileSpeed in mp_titanweapon_tracker_rockets.nut.
const TRACKER_ROCKET_SPEED: f32 = 1800.0;
const SLOW_TRAP_SLOW: f32 = 0.5;
/// GAS_FX_HEIGHT and FIRE_TRAP_MINI_EXPLOSION_RADIUS (mp_titanability_slow_trap.nut).
const SLOW_TRAP_GAS_HEIGHT: f32 = 45.0;
const FIRE_TRAP_MINI_EXPLOSION_RADIUS: f32 = 75.0;
const LASER_TRIP_LIFETIME: f32 = 12.0;
const LASER_TRIP_BUILD_TIME: f32 = 1.0;
const LASER_TRIP_DAMAGE: f32 = 200.0;
const LASER_TRIP_DAMAGE_HEAVY: f32 = 1500.0;
const LASER_TRIP_DEPLOY_POWER: f32 = 900.0;
const LASER_TRIP_DEPLOY_SIDE_POWER: f32 = 1200.0;
const LASER_TRIP_MAX: usize = 9;
/// The Power Shot mods' charge_time (mp_titanweapon_predator_cannon.txt).
const POWER_SHOT_CHARGE: f32 = 0.75;
/// LASER_TRIP_INNER_RADIUS / LASER_TRIP_OUTER_RADIUS: a pylon's blast.
const LASER_TRIP_RADIUS: f32 = 400.0;
/// The beam's box: 8 either side of the line between the pylons (LaserPylonSetThink).
const LASER_TRIP_BEAM_HALF: f32 = 8.0;
/// DeployLaserPylon: the hidden pylon rises 45 units above the planted point (straight up the
/// upright tower) over the build time. Its model's "center" attachment, where the beams start
/// and end, is at its origin.
const LASER_TRIP_RISE: f32 = 45.0;
/// The tower prop (LASER_TRIP_MODEL), which plays "trip_wire_closed_to_open" once planted.
pub const LASER_TRIP_MODEL: &str = "models/weapons/titan_trip_wire/titan_trip_wire.mdl";
/// Deployables fall under the projectile gravity (750 u/s² at scale 1).
const DEPLOYABLE_GRAVITY: f32 = 750.0;
/// Tether Trap (mp_titanability_tether_trap.nut / mp_weapon_tether.nut): in SP each use
/// fires two tethers at 1000 u/s, offset up 0.3 and right ±0.2 of the view; they plant on
/// floors (normal · up ≥ 0.7) and bounce off walls; ProximityTetherThink arms 0.5 + 1.0 s
/// after landing, lives 60 s and catches enemy Titans within 450 units in line of sight.
const TETHER_SPEED: f32 = 1000.0;
const TETHER_OFFSET_UP: f32 = 0.3;
const TETHER_OFFSET_RIGHT: f32 = 0.2;
const TETHER_PLANT_DOT: f32 = 0.7;
const TETHER_ARM: f32 = 1.5;
const TETHER_LIFETIME: f32 = 60.0;
const TETHER_CATCH_RADIUS_NPC: f32 = 450.0;
/// projectile_max_deployed.
const TETHERS_MAX: usize = 4;
/// Bounces keep the frag's default velocity fractions (the tether file sets none).
const TETHER_BOUNCE_SHALLOW: f32 = 0.5;
const TETHER_BOUNCE_SHARP: f32 = 0.3;

/// The tether's model (TETHER_3P_MODEL caber_shot_thrown_xl.mdl), parented under the world.
#[derive(Resource)]
pub struct TetherAssets {
    pub parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    pub root: Entity,
}

/// Spawn a tether entity at `pos` (game space) with the model (a child of the world root, so
/// its transform stays in game units), or a glow in Bevy space without it.
fn spawn_tether(
    commands: &mut Commands,
    assets: Option<&TetherAssets>,
    pos: Vec3,
    extra: impl Bundle,
) -> Entity {
    match assets {
        Some(a) => {
            let e = commands
                .spawn((
                    Transform::from_translation(pos),
                    Visibility::default(),
                    extra,
                ))
                .id();
            for (mesh, mat) in &a.parts {
                let c = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat.clone()),
                        Transform::IDENTITY,
                    ))
                    .id();
                commands.entity(e).add_child(c);
            }
            commands.entity(a.root).add_child(e);
            e
        }
        None => commands
            .spawn((
                Transform::from_translation(to_bevy(pos)),
                Glow::new(Vec3::new(1.5, 3.0, 6.0), 0.4, false),
                extra,
            ))
            .id(),
    }
}

/// A tether grenade in flight (FireTether: FireWeaponGrenade at 1000 u/s).
#[derive(Component)]
pub struct TetherShot {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    /// Tumble (FireTether gives each a random angular velocity up to 360°/s per axis).
    spin: Vec3,
}

/// Electric Smoke cloud: damages after `delay`, every 0.1 s, out to the outer radius.
#[derive(Component)]
pub struct TitanSmoke {
    pos: Vec3,
    life: f32,
    delay: f32,
    tick: f32,
    puff: f32,
    /// Deployed by an enemy Titan: hurts the Pilot and BT instead of the enemies.
    enemy: bool,
    hurt_pilot: bool,
}

/// TitanSmokescreen: an Electric Smoke cloud at `at`, facing `fwd` (`enemy` when an enemy
/// Titan's). Its effect is `P_wpn_smk_electric` at the script's five `fxOffsets` (200 back and
/// to either side 20 up, and 148 up above the middle and the back), with the cloud's lifetime
/// on control point 1 (what the system's operators read it as). The Smokescreen script isn't
/// in the shipped scripts, so the plain variant (beside `_burn_mod` and `_heal`) is a guess.
pub fn spawn_smoke(commands: &mut Commands, at: Vec3, fwd: Vec3, enemy: bool) {
    commands.spawn((
        Transform::from_translation(to_bevy(at)),
        TitanSmoke {
            pos: at,
            life: TITAN_SMOKE_LIFETIME,
            delay: 1.0,
            tick: 0.0,
            puff: 0.0,
            enemy,
            hurt_pilot: false,
        },
    ));
    if std::env::var_os("TF_OLD_FX").is_some() {
        return;
    }
    let cp = Cp::facing(at, Vec3::new(fwd.x, fwd.y, 0.0).normalize_or(Vec3::X));
    for o in [
        Vec3::new(-200.0, 0.0, 20.0),
        Vec3::new(0.0, 200.0, 20.0),
        Vec3::new(0.0, -200.0, 20.0),
        Vec3::new(0.0, 0.0, 148.0),
        Vec3::new(-200.0, 0.0, 148.0),
    ] {
        let pos = at + cp.fwd * o.x + cp.left * o.y + cp.up * o.z;
        // Control point 1's X is the cloud's lifetime: its smoke lives 1.25-1.5 times that and
        // its arcs stop after it.
        crate::pfx::emit_named_cps(
            commands,
            "P_wpn_smk_electric",
            &[
                Cp { pos, ..cp },
                Cp {
                    pos: Vec3::new(TITAN_SMOKE_LIFETIME, 0.0, 0.0),
                    ..cp
                },
            ],
            Some(TITAN_SMOKE_LIFETIME),
        );
    }
}

/// Scorch's Slow Trap canister: flies, lands, builds, then gasses a circle that slows.
#[derive(Component)]
pub struct SlowTrap {
    pos: Vec3,
    vel: Vec3,
    landed: bool,
    build: f32,
    life: f32,
    puff: f32,
}

/// One Laser Tripwire pylon; beams run between the armed pylons of a group.
#[derive(Component)]
pub struct LaserPylon {
    pos: Vec3,
    vel: Vec3,
    landed: bool,
    build: f32,
    life: f32,
    group: u32,
    /// projectileID: 0 thrown left, 1 straight, 2 right. Each pylon beams to the next one.
    id: u8,
    /// The throw's heading (proj.savedAngles keeps only the yaw): the tower's.
    yaw: f32,
    /// The pylon this one beams to and that pylon's beam point, once connected.
    link: Option<(Entity, Vec3)>,
}

/// The tower model under a planted pylon; `actor` is set once the model is spawned.
#[derive(Component)]
pub struct LaserTower {
    age: f32,
    actor: Option<Entity>,
}

/// Fly a thrown deployable until it lands (returns true once on the ground).
fn fly_deployable(world: &Collision, pos: &mut Vec3, vel: &mut Vec3, dt: f32) -> bool {
    vel.z -= DEPLOYABLE_GRAVITY * dt;
    let step = *vel * dt;
    let len = step.length().max(1e-4);
    let d = step / len;
    if let Some(h) = world.0.raycast(
        SVec3::from(pos.to_array()),
        SVec3::from(d.to_array()),
        len + 8.0,
    ) {
        *pos += d * (h.t - 8.0).max(0.0);
        let n = Vec3::from(h.normal.to_array());
        if n.z > 0.5 {
            return true;
        }
        // Slide off walls.
        *vel -= n * vel.dot(n) * 1.5;
        return false;
    }
    *pos += step;
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OrdKind {
    #[default]
    Missiles,
    LaserShot,
    Firewall,
    Cluster,
    ArcWave,
    Tracker,
    PowerShot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DefKind {
    #[default]
    Vortex,
    IonVortex,
    HeatShield,
    SwordBlock,
    ParticleWall,
    Tether,
    GunShield,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UtilKind {
    #[default]
    Smoke,
    SonarPulse,
    SlowTrap,
    LaserTrip,
    PhaseDash,
    Hover,
    AmmoSwap,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CoreKind {
    #[default]
    Burst,
    Flight,
    Laser,
    FlameWave,
    Sword,
    Salvo,
    Smart,
}

/// (weapon script, kind, icon and label if the script or localisation has none)
const ORDNANCE: &[(&str, OrdKind, &str, &str)] = &[
    (
        "mp_titanweapon_shoulder_rockets",
        OrdKind::Missiles,
        "rui/titan_loadout/ordnance/multilock_rockets_menu",
        "MULTI-TARGET MISSILES",
    ),
    (
        "mp_titanweapon_laser_lite",
        OrdKind::LaserShot,
        "rui/titan_loadout/ordnance/laser_shot_menu",
        "LASER SHOT",
    ),
    (
        "mp_titanweapon_flame_wall",
        OrdKind::Firewall,
        "rui/titan_loadout/ordnance/flame_wall_menu",
        "FIREWALL",
    ),
    (
        "mp_titanweapon_dumbfire_rockets",
        OrdKind::Cluster,
        "rui/titan_loadout/ordnance/cluster_missile_menu",
        "CLUSTER MISSILE",
    ),
    (
        "mp_titanweapon_arc_wave",
        OrdKind::ArcWave,
        "rui/titan_loadout/ordnance/arc_wave_menu",
        "ARC WAVE",
    ),
    (
        "mp_titanweapon_tracker_rockets",
        OrdKind::Tracker,
        "rui/titan_loadout/ordnance/tracking_rockets_menu",
        "TRACKER ROCKETS",
    ),
    (
        "mp_titanability_power_shot",
        OrdKind::PowerShot,
        "rui/titan_loadout/ordnance/concussive_shot_short_menu",
        "POWER SHOT",
    ),
];

const DEFENSIVE: &[(&str, DefKind, &str, &str)] = &[
    (
        "mp_titanweapon_vortex_shield",
        DefKind::Vortex,
        "rui/titan_loadout/defensive/titan_defensive_vortex_menu",
        "VORTEX SHIELD",
    ),
    (
        "mp_titanweapon_vortex_shield_ion",
        DefKind::IonVortex,
        "rui/titan_loadout/defensive/titan_defensive_vortex_menu",
        "VORTEX SHIELD",
    ),
    (
        "mp_titanweapon_heat_shield",
        DefKind::HeatShield,
        "rui/titan_loadout/defensive/titan_defensive_heat_shield_menu",
        "THERMAL SHIELD",
    ),
    (
        "mp_titanability_basic_block",
        DefKind::SwordBlock,
        "rui/titan_loadout/defensive/titan_defensive_sword_block_menu",
        "SWORD BLOCK",
    ),
    (
        "mp_titanability_particle_wall",
        DefKind::ParticleWall,
        "rui/titan_loadout/defensive/titan_defensive_particle_wall_menu",
        "PARTICLE WALL",
    ),
    (
        "mp_titanability_tether_trap",
        DefKind::Tether,
        "rui/titan_loadout/ordnance/tether_menu",
        "TETHER TRAP",
    ),
    (
        "mp_titanability_gun_shield",
        DefKind::GunShield,
        "rui/titan_loadout/defensive/titan_defensive_gun_shield_menu",
        "GUN SHIELD",
    ),
];

const UTILITY: &[(&str, UtilKind, &str, &str)] = &[
    (
        "mp_titanability_smoke",
        UtilKind::Smoke,
        "rui/titan_loadout/tactical/titan_tactical_electric_smoke_menu",
        "ELECTRIC SMOKE",
    ),
    (
        "mp_titanability_sonar_pulse",
        UtilKind::SonarPulse,
        "rui/titan_loadout/tactical/titan_tactical_radar_pulse_menu",
        "SONAR PULSE",
    ),
    (
        "mp_titanability_slow_trap",
        UtilKind::SlowTrap,
        "rui/titan_loadout/tactical/titan_tactical_slow_trap_menu",
        "SLOW TRAP",
    ),
    (
        "mp_titanability_laser_trip",
        UtilKind::LaserTrip,
        "rui/titan_loadout/tactical/titan_tactical_laser_tripwire_menu",
        "LASER TRIPWIRE",
    ),
    (
        "mp_titanability_phase_dash",
        UtilKind::PhaseDash,
        "rui/titan_loadout/tactical/titan_tactical_phase_dash_menu",
        "PHASE DASH",
    ),
    (
        "mp_titanability_hover",
        UtilKind::Hover,
        "rui/titan_loadout/tactical/titan_tactical_hover_menu",
        "HOVER",
    ),
    (
        "mp_titanability_ammo_swap",
        UtilKind::AmmoSwap,
        "rui/titan_loadout/tactical/titan_tactical_ammo_swap_menu",
        "AMMO SWAP",
    ),
];

const CORES: &[(&str, CoreKind, &str, &str)] = &[
    (
        "mp_titancore_amp_core",
        CoreKind::Burst,
        "rui/titan_loadout/core/titan_core_burst_core",
        "BURST CORE",
    ),
    (
        "mp_titancore_flight_core",
        CoreKind::Flight,
        "rui/titan_loadout/core/titan_core_flight",
        "FLIGHT CORE",
    ),
    (
        "mp_titancore_laser_cannon",
        CoreKind::Laser,
        "rui/titan_loadout/core/titan_core_laser",
        "LASER CORE",
    ),
    (
        "mp_titancore_flame_wave",
        CoreKind::FlameWave,
        "rui/titan_loadout/core/titan_core_flame_wave",
        "FLAME CORE",
    ),
    (
        "mp_titancore_shift_core",
        CoreKind::Sword,
        "rui/titan_loadout/core/titan_core_sword",
        "SWORD CORE",
    ),
    (
        "mp_titancore_salvo_core",
        CoreKind::Salvo,
        "rui/titan_loadout/core/titan_core_salvo",
        "SALVO CORE",
    ),
    (
        "mp_titancore_siege_mode",
        CoreKind::Smart,
        "rui/titan_loadout/core/titan_core_smart",
        "SMART CORE",
    ),
];

/// The weapon scripts behind a kit's ordnance, defensive, utility and core.
pub fn kit_scripts(k: &ActiveKit) -> [&'static str; 4] {
    [
        ORDNANCE.iter().find(|o| o.1 == k.ord).map_or("", |o| o.0),
        DEFENSIVE.iter().find(|d| d.1 == k.def).map_or("", |d| d.0),
        UTILITY.iter().find(|u| u.1 == k.util).map_or("", |u| u.0),
        CORES.iter().find(|c| c.1 == k.core).map_or("", |c| c.0),
    ]
}

/// titan_properties rows for BT's SP loadouts, used if the datatable can't be read:
/// (primary, ordnance, special, antirodeo, core).
const FALLBACK: &[(&str, &str, &str, &str, &str)] = &[
    (
        "mp_titanweapon_xo16_shorty",
        "mp_titanweapon_shoulder_rockets",
        "mp_titanweapon_vortex_shield",
        "mp_titanability_smoke",
        "mp_titancore_amp_core",
    ),
    (
        "mp_titanweapon_sticky_40mm",
        "mp_titanweapon_tracker_rockets",
        "mp_titanability_particle_wall",
        "mp_titanability_sonar_pulse",
        "mp_titancore_salvo_core",
    ),
    (
        "mp_titanweapon_meteor",
        "mp_titanweapon_flame_wall",
        "mp_titanweapon_heat_shield",
        "mp_titanability_slow_trap",
        "mp_titancore_flame_wave",
    ),
    (
        "mp_titanweapon_rocketeer_rocketstream",
        "mp_titanweapon_shoulder_rockets",
        "mp_titanweapon_vortex_shield",
        "mp_titanability_hover",
        "mp_titancore_flight_core",
    ),
    (
        "mp_titanweapon_particle_accelerator",
        "mp_titanweapon_laser_lite",
        "mp_titanweapon_vortex_shield_ion",
        "mp_titanability_laser_trip",
        "mp_titancore_laser_cannon",
    ),
    (
        "mp_titanweapon_leadwall",
        "mp_titanweapon_arc_wave",
        "mp_titanability_basic_block",
        "mp_titanability_phase_dash",
        "mp_titancore_shift_core",
    ),
    (
        "mp_titanweapon_sniper",
        "mp_titanweapon_dumbfire_rockets",
        "mp_titanability_tether_trap",
        "mp_titanability_hover",
        "mp_titancore_flight_core",
    ),
    (
        "mp_titanweapon_predator_cannon",
        "mp_titanability_power_shot",
        "mp_titanability_gun_shield",
        "mp_titanability_ammo_swap",
        "mp_titancore_siege_mode",
    ),
];

/// One loadout's kit.
#[derive(Clone, Debug)]
pub struct KitSpec {
    pub ord: OrdKind,
    pub def: DefKind,
    pub util: UtilKind,
    pub core: CoreKind,
    pub ord_icon: String,
    pub def_icon: String,
    pub util_icon: String,
    pub util_label: String,
    pub util_sound: String,
    pub core_icon: String,
    pub ord_label: String,
    pub def_label: String,
    pub core_label: String,
    pub ord_sound: String,
    /// The ordnance script's deploy_time: an offhand fires once it has deployed.
    pub ord_deploy: f32,
    pub def_sound: String,
}

/// Numbers from the ability scripts.
/// A utility ability's script numbers.
#[derive(Clone, Copy, Debug)]
pub struct UtilNumbers {
    pub cooldown: f32,
    pub charges: u32,
    pub duration: f32,
}

#[derive(Clone, Debug)]
pub struct KitNumbers {
    /// By script name (UTILITY).
    pub util: HashMap<&'static str, UtilNumbers>,
    // Ion
    pub energy_max: f32,
    pub energy_regen: f32,
    pub energy_delay: f32,
    pub laser_cost: f32,
    pub laser_titan: f32,
    pub laser_pilot: f32,
    pub laser_interval: f32,
    pub ion_vortex_drain: f32,
    // Scorch
    pub firewall_cd: f32,
    pub firewall_step: f32,
    pub firewall_count: u32,
    /// The ignited Slow Trap's fire lines (WeaponAttackWave with the slow trap weapon).
    pub trap_step: f32,
    pub trap_count: u32,
    pub thermite_life: f32,
    pub thermite_tick: f32,
    pub thermite_tick_pilot: f32,
    pub thermite_radius: f32,
    // Northstar
    pub cluster_cd: f32,
    pub cluster_speed: f32,
    pub cluster_heavy: f32,
    pub cluster_pilot: f32,
    pub cluster_inner: f32,
    pub cluster_radius: f32,
    pub cluster_bursts: u32,
    pub cluster_duration: f32,
    pub cluster_range: f32,
    pub tether_cd: f32,
    pub tether_hold: f32,
    // Ronin
    pub arc_cd: f32,
    pub arc_titan: f32,
    pub arc_pilot: f32,
    pub arc_step: f32,
    pub arc_count: u32,
    pub block_scale: f32,
    pub block_scale_core: f32,
    pub sword_damage: f32,
    pub sword_core_bonus: f32,
    pub sword_range: f32,
    // Tone
    pub tracker_count: u32,
    pub tracker_rate: f32,
    pub tracker_damage: f32,
    pub tracker_delay: f32,
    pub wall_cd: f32,
    pub wall_hp: f32,
    pub wall_life: f32,
    // Legion
    pub power_cd: f32,
    pub power_titan: f32,
    pub power_pilot: f32,
    pub power_radius: f32,
    pub power_inner: f32,
    pub gun_shield_cd: f32,
    pub gun_shield_hp: f32,
    pub gun_shield_life: f32,
    // Cores (charge-up, duration, the core's deploy_time when it holsters the primary first:
    // the gun's holster_time and this come before the charge-up).
    pub core_timing: HashMap<&'static str, (f32, f32, Option<f32>)>,
    pub flight_rate: f32,
    pub flight_rocket: MissileSpec,
    pub laser_core_tick: f32,
    pub laser_core_range: f32,
    pub laser_core_radius: f32,
    pub flame_titan: f32,
    pub flame_pilot: f32,
    pub flame_step: f32,
    pub flame_count: u32,
    pub salvo_count: u32,
    pub salvo_rate: f32,
    pub salvo_rocket: MissileSpec,
}

/// SP_BASE, then MP_BASE, then the top level of a weapon script.
fn num(s: &PlayerSettings, k: &str, d: f32) -> f32 {
    for pre in ["sp_base.", "mp_base.", "."] {
        if let Some(v) = s
            .get(&format!("{pre}{k}"))
            .and_then(|v| v.parse::<f32>().ok())
        {
            return v;
        }
    }
    d
}

/// Seconds to refill an ammo-regen ability (ammo_per_shot / regen_ammo_refill_rate).
fn regen_cd(s: &PlayerSettings, d: f32) -> f32 {
    let per = num(s, "ammo_per_shot", 0.0);
    let rate = num(s, "regen_ammo_refill_rate", 0.0);
    if per > 0.0 && rate > 0.0 {
        per / rate
    } else {
        d
    }
}

impl KitNumbers {
    fn load(read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let mut w = |n: &str| {
            PlayerSettings::load(&format!("scripts/weapons/{n}.txt"), true, read)
                .unwrap_or_default()
        };
        let laser = w("mp_titanweapon_laser_lite");
        let firewall = w("mp_titanweapon_flame_wall");
        let slow_trap = w("mp_titanability_slow_trap");
        let cluster = w("mp_titanweapon_dumbfire_rockets");
        let tether = w("mp_titanability_tether_trap");
        let arc = w("mp_titanweapon_arc_wave");
        let sword = w("melee_titan_sword");
        let tracker = w("mp_titanweapon_tracker_rockets");
        let wall = w("mp_titanability_particle_wall");
        let power = w("mp_titanability_power_shot");
        let gun = w("mp_titanability_gun_shield");
        let flight = w("mp_titanweapon_flightcore_rockets");
        let laser_core = w("mp_titancore_laser_cannon");
        let flame = w("mp_titancore_flame_wave");
        let salvo = w("mp_titancore_salvo_core");
        let mut core_timing = HashMap::new();
        for (id, _, _, _) in CORES {
            let s = w(id);
            core_timing.insert(
                *id,
                (
                    num(&s, "charge_time", 1.0).max(0.1),
                    num(&s, "core_duration", 6.0),
                    (num(&s, "offhand_holster_primary", 0.0) > 0.5)
                        .then(|| num(&s, "deploy_time", 0.2)),
                ),
            );
        }
        // Burst Core's spin-up is BT's core animation (abilities::CORE_CHARGEUP).
        if let Some(t) = core_timing.get_mut("mp_titancore_amp_core") {
            t.0 = crate::abilities::CORE_CHARGEUP;
        }
        let mut util = HashMap::new();
        for (id, _, _, _) in UTILITY {
            let s = w(id);
            let per_shot = num(&s, "ammo_per_shot", 100.0).max(1.0);
            util.insert(
                *id,
                UtilNumbers {
                    cooldown: per_shot / num(&s, "regen_ammo_refill_rate", 5.0).max(0.01),
                    charges: (num(&s, "ammo_clip_size", per_shot) / per_shot)
                        .floor()
                        .max(1.0) as u32,
                    duration: num(&s, "fire_duration", 0.0),
                },
            );
        }
        Self {
            util,
            energy_max: 1000.0,  // ION_ENERGY_MAX / sharedEnergyTotal
            energy_regen: 100.0, // titan_atlas_stickybomb.set sharedEnergyRegenRate [$sp]
            energy_delay: 0.2,   // sharedEnergyRegenDelay
            laser_cost: num(&laser, "shared_energy_cost", 500.0),
            laser_titan: num(&laser, "damage_near_value_titanarmor", 1500.0),
            laser_pilot: num(&laser, "damage_near_value", 300.0),
            laser_interval: 1.0 / num(&laser, "fire_rate", 1.5).max(0.1),
            // shared_energy_charge_cost 2 per 60 Hz frame while held.
            ion_vortex_drain: 120.0,
            firewall_cd: regen_cd(&firewall, 10.0),
            firewall_step: num(&firewall, "wave_step_dist", 100.0),
            firewall_count: num(&firewall, "wave_max_count", 15.0) as u32,
            trap_step: num(&slow_trap, "wave_step_dist", 100.0),
            trap_count: num(&slow_trap, "wave_max_count", 15.0) as u32,
            // FLAME_WALL_THERMITE_DURATION * SP_FLAME_WALL_DURATION_SCALE for the player.
            thermite_life: 5.2 * 1.75,
            thermite_tick: 100.0,      // PLAYER_METEOR_DAMAGE_TICK
            thermite_tick_pilot: 20.0, // PLAYER_METEOR_DAMAGE_TICK_PILOT
            thermite_radius: 60.0,     // FLAME_WALL_DAMAGE_RADIUS_DEF
            cluster_cd: regen_cd(&cluster, 9.0),
            cluster_speed: 3500.0, // FireClusterRocket missileSpeed
            cluster_heavy: num(&cluster, "explosion_damage_heavy_armor", 150.0),
            cluster_pilot: num(&cluster, "explosion_damage", 66.0),
            cluster_inner: num(&cluster, "explosion_inner_radius", 150.0),
            cluster_radius: num(&cluster, "explosionradius", 220.0),
            cluster_bursts: 20,    // CLUSTER_ROCKET_BURST_COUNT
            cluster_duration: 5.0, // CLUSTER_ROCKET_DURATION
            cluster_range: 250.0,  // CLUSTER_ROCKET_BURST_RANGE
            tether_cd: regen_cd(&tether, 20.0),
            tether_hold: 4.0,
            arc_cd: regen_cd(&arc, 10.0),
            arc_titan: num(&arc, "damage_near_value_titanarmor", 1500.0),
            arc_pilot: num(&arc, "damage_near_value", 250.0),
            arc_step: num(&arc, "wave_step_dist", 112.0),
            arc_count: num(&arc, "wave_max_count", 15.0) as u32,
            block_scale: 0.3,       // TITAN_BLOCK_DAMAGE_REDUCTION
            block_scale_core: 0.15, // SWORD_CORE_BLOCK_DAMAGE_REDUCTION
            sword_damage: num(&sword, "melee_damage_heavyarmor", 625.0),
            sword_core_bonus: 1400.0, // super_charged: melee_damage ++1400
            sword_range: num(&sword, "melee_range", 325.0),
            tracker_count: num(&tracker, "burst_fire_count", 6.0) as u32,
            tracker_rate: num(&tracker, "fire_rate", 14.0),
            tracker_damage: num(&tracker, "damage_near_value", 350.0),
            tracker_delay: num(&tracker, "burst_fire_delay", 0.7),
            wall_cd: regen_cd(&wall, 14.0),
            wall_hp: 1750.0, // SHIELD_WALL_HEALTH (SP)
            wall_life: 8.0,  // SHIELD_WALL_DURATION
            power_cd: regen_cd(&power, 8.0),
            power_titan: num(&power, "damage_near_value_titanarmor", 2000.0),
            power_pilot: num(&power, "damage_near_value", 200.0),
            power_radius: num(&power, "explosionradius", 150.0),
            power_inner: num(&power, "explosion_inner_radius", 50.0),
            gun_shield_cd: regen_cd(&gun, 8.0),
            gun_shield_hp: 2500.0, // TITAN_GUN_SHIELD_HEALTH
            gun_shield_life: num(&gun, "fire_duration", 6.0),
            core_timing,
            flight_rate: num(&flight, "fire_rate", 12.0),
            flight_rocket: MissileSpec {
                speed: num(&flight, "projectile_launch_speed", 3000.0),
                homing: 0.0,
                direct: num(&flight, "damage_near_value_titanarmor", 200.0),
                splash: num(&flight, "explosion_damage_heavy_armor", 200.0),
                radius: num(&flight, "explosionradius", 300.0),
                out_time: 0.0,
            },
            laser_core_tick: num(&laser_core, "damage_near_value_titanarmor", 200.0),
            laser_core_range: num(&laser_core, "sustained_laser_range", 6000.0),
            laser_core_radius: num(&laser_core, "sustained_laser_radius", 18.0),
            flame_titan: num(&flame, "damage_near_value_titanarmor", 4000.0),
            flame_pilot: num(&flame, "damage_near_value", 300.0),
            flame_step: num(&flame, "wave_step_dist", 120.0),
            flame_count: num(&flame, "wave_max_count", 15.0) as u32,
            salvo_count: num(&salvo, "burst_fire_count", 20.0) as u32,
            salvo_rate: num(&salvo, "fire_rate", 12.0),
            salvo_rocket: MissileSpec {
                speed: num(&salvo, "projectile_launch_speed", 1000.0),
                homing: crate::abilities::MTMS.homing,
                direct: num(&salvo, "damage_near_value_titanarmor", 120.0),
                splash: num(&salvo, "explosion_damage_heavy_armor", 120.0),
                radius: num(&salvo, "explosionradius", 150.0),
                out_time: 0.2,
            },
        }
    }
}

#[derive(Resource)]
pub struct KitDefs {
    pub kits: Vec<KitSpec>,
    pub n: KitNumbers,
    /// The Predator Cannon in its two ammo modes (plain, and with `LongRangeAmmo`), for
    /// Legion's Ammo Swap.
    pub predator: [crate::weapons::WeaponDef; 2],
}

impl KitDefs {
    /// Read titan_properties and the ability scripts.
    pub fn load(gd: &GameData, strings: Option<&crate::pilotweapon::Strings>) -> Self {
        let table = gd.paks.iter().find_map(|p| {
            tf_assets::datatable::DataTable::load(p, "datatable/titan_properties.rpak")
        });
        let mut read = |p: &str| {
            gd.read_file(p)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).to_string())
        };
        let n = KitNumbers::load(&mut read);
        // A weapon script's localised name, 1P fire sound, deploy time and HUD icon.
        let mut info = |id: &str| -> (Option<String>, String, f32, Option<String>) {
            let Some(s) =
                PlayerSettings::load(&format!("scripts/weapons/{id}.txt"), true, &mut read)
            else {
                return (None, String::new(), 0.0, None);
            };
            let name = s
                .get(".printname")
                .map(|k| k.trim_start_matches('#').to_string())
                .and_then(|k| strings.and_then(|t| t.0.get(&k)).map(|v| v.to_uppercase()));
            let deploy = s
                .get(".deploy_time")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0.0);
            let icon = s
                .get(".hud_icon")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            (
                name,
                s.get(".fire_sound_1_player_1p")
                    .unwrap_or_default()
                    .to_string(),
                deploy,
                icon,
            )
        };
        let mut kits = Vec::new();
        for gun in TITAN_ARSENAL {
            // (ordnance, special, core, core icon)
            let row: Option<(String, String, String, String, String)> =
                table.as_ref().and_then(|t| {
                    let col = |n: &str| t.column(n);
                    let (p, o, s, a, c, i) = (
                        col("primary")?,
                        col("ordnance")?,
                        col("special")?,
                        col("antirodeo")?,
                        col("coreAbility")?,
                        col("coreReadyIcon")?,
                    );
                    t.rows.iter().find(|r| r[p].text() == gun.id).map(|r| {
                        (
                            r[o].text(),
                            r[s].text(),
                            r[a].text(),
                            r[c].text(),
                            r[i].text().replace('\\', "/"),
                        )
                    })
                });
            let (ord_id, def_id, util_id, core_id, core_icon) = row.unwrap_or_else(|| {
                let f = FALLBACK
                    .iter()
                    .find(|f| f.0 == gun.id)
                    .copied()
                    .unwrap_or(FALLBACK[0]);
                (
                    f.1.to_string(),
                    f.2.to_string(),
                    f.3.to_string(),
                    f.4.to_string(),
                    String::new(),
                )
            });
            let u = UTILITY
                .iter()
                .find(|x| x.0 == util_id)
                .copied()
                .unwrap_or(UTILITY[0]);
            let (util_name, util_sound, _, util_icon) = info(u.0);
            let o = ORDNANCE
                .iter()
                .find(|x| x.0 == ord_id)
                .copied()
                .unwrap_or(ORDNANCE[0]);
            let d = DEFENSIVE
                .iter()
                .find(|x| x.0 == def_id)
                .copied()
                .unwrap_or(DEFENSIVE[0]);
            let c = CORES
                .iter()
                .find(|x| x.0 == core_id)
                .copied()
                .unwrap_or(CORES[0]);
            let (ord_name, ord_sound, ord_deploy, ord_icon) = info(o.0);
            let (def_name, def_sound, _, def_icon) = info(d.0);
            let (core_name, _, _, _) = info(c.0);
            kits.push(KitSpec {
                ord: o.1,
                def: d.1,
                util: u.1,
                core: c.1,
                // The scripts' hud_icon (the slot art); the table's menu art if a script has none.
                ord_icon: ord_icon.unwrap_or_else(|| o.2.to_string()),
                def_icon: def_icon.unwrap_or_else(|| d.2.to_string()),
                util_icon: util_icon.unwrap_or_else(|| u.2.to_string()),
                util_label: util_name.unwrap_or_else(|| u.3.to_string()),
                util_sound,
                core_icon: if core_icon.is_empty() {
                    c.2.to_string()
                } else {
                    core_icon
                },
                ord_label: ord_name.unwrap_or_else(|| o.3.to_string()),
                def_label: def_name.unwrap_or_else(|| d.3.to_string()),
                core_label: core_name.unwrap_or_else(|| c.3.to_string()),
                ord_sound,
                ord_deploy,
                def_sound,
            });
        }
        log::info!(
            "titan kits ({}): {}",
            if table.is_some() {
                "titan_properties"
            } else {
                "fallback table"
            },
            kits.iter()
                .zip(TITAN_ARSENAL)
                .map(|(k, g)| format!(
                    "{} {:?}/{:?}/{:?}/{:?}",
                    g.kit, k.def, k.util, k.ord, k.core
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let predator = [
            crate::weapons::WeaponDef::load_modded(
                "mp_titanweapon_predator_cannon",
                &[],
                &mut read,
            ),
            crate::weapons::WeaponDef::load_modded(
                "mp_titanweapon_predator_cannon",
                &["LongRangeAmmo"],
                &mut read,
            ),
        ];
        Self { kits, n, predator }
    }
}

/// The current loadout's kit (read by abilities, combat and weapons).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveKit {
    pub index: usize,
    pub ord: OrdKind,
    pub def: DefKind,
    pub util: UtilKind,
    pub core: CoreKind,
}

/// A shield that stops bolts arriving against its front (Particle Wall, Gun Shield);
/// targets::update_bolts tests against these and reports the damage each one soaks up.
#[derive(Clone, Copy, Debug)]
pub struct BoltShield {
    /// The disc's centre, or the bottom of the arc's axis.
    pub center: Vec3,
    /// Facing direction (bolts arriving against it are stopped).
    pub normal: Vec3,
    pub radius: f32,
    owner: Option<Entity>,
    /// A Particle Wall: instead of a disc, the outside of a vertical cylinder of `radius`
    /// around `center`, `height` tall and `cos_half_fov` wide about `normal`
    /// (CreateShieldWithSettings: SF_ABSORB_CYLINDER, radius, height, bullet_fov).
    arc: Option<(f32, f32)>,
}

impl BoltShield {
    fn disc(center: Vec3, normal: Vec3, radius: f32, owner: Option<Entity>) -> Self {
        Self {
            center,
            normal,
            radius,
            owner,
            arc: None,
        }
    }

    /// A Particle Wall around `origin` facing `normal` (SHIELD_WALL_RADIUS 180, height
    /// radius * 2, SHIELD_WALL_FOV 120).
    fn wall(origin: Vec3, normal: Vec3, owner: Option<Entity>) -> Self {
        let n = Vec3::new(normal.x, normal.y, 0.0).normalize_or(Vec3::X);
        Self {
            center: origin,
            normal: n,
            radius: SHIELD_WALL_RADIUS,
            owner,
            arc: Some((
                (SHIELD_WALL_FOV * 0.5).to_radians().cos(),
                SHIELD_WALL_RADIUS * 2.0,
            )),
        }
    }

    /// Where the segment `from + dir * t, t < max` meets the shield from its front.
    fn hit_t(&self, from: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let t = match self.arc {
            None => {
                let denom = dir.dot(self.normal);
                if denom >= -1e-4 {
                    return None;
                }
                let t = (self.center - from).dot(self.normal) / denom;
                ((from + dir * t).distance(self.center) < self.radius).then_some(t)?
            }
            Some((cos_half, height)) => {
                // Entering the cylinder from outside, within the arc and its height.
                let (p, d) = ((from - self.center).truncate(), dir.truncate());
                let (a, b, c) = (
                    d.length_squared(),
                    p.dot(d),
                    p.length_squared() - self.radius * self.radius,
                );
                let disc = b * b - a * c;
                if a < 1e-8 || c <= 0.0 || disc < 0.0 {
                    return None;
                }
                let t = (-b - disc.sqrt()) / a;
                let h = from + dir * t;
                let radial = (h - self.center).truncate().normalize_or_zero();
                let inside = radial.dot(self.normal.truncate()) >= cos_half
                    && h.z >= self.center.z
                    && h.z <= self.center.z + height;
                inside.then_some(t)?
            }
        };
        (t > 0.0 && t < max).then_some(t)
    }
}

/// CreateParticleWallForOwnerFromDirection / CreateShieldWithSettings (_particle_wall.gnut).
const SHIELD_WALL_RADIUS: f32 = 180.0;
const SHIELD_WALL_FOV: f32 = 120.0;

#[derive(Resource, Default)]
pub struct BoltShields {
    pub shields: Vec<BoltShield>,
    /// (shield index, damage) since last frame.
    pub absorbed: Vec<(usize, f32)>,
}

impl BoltShields {
    /// First shield the segment `from + dir * t, t < max` passes through from the front.
    pub fn hit(&self, from: Vec3, dir: Vec3, max: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in self.shields.iter().enumerate() {
            if let Some(t) = s
                .hit_t(from, dir, max)
                .filter(|&t| best.is_none_or(|b| t < b.1))
            {
                best = Some((i, t));
            }
        }
        best
    }
}

/// The Titans BT's projectiles damaged this frame (Tone's 40mm builds lock stacks on them).
#[derive(Resource, Default)]
pub struct TitanBoltHits(pub Vec<Entity>);

/// What the HUD shows for the Q and E slots.
#[derive(Resource, Default)]
pub struct KitView {
    pub ord_left: f32,
    pub ord_text: String,
    pub def_left: f32,
    pub def_text: String,
    pub util_left: f32,
    pub util_text: String,
    /// The kit's core icon (drawn in the HUD's meter).
    pub core_icon: String,
}

/// What the utility slot does to BT's movement this frame (read by player::simulate).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct TitanMoveMods {
    /// Hover: (vertical velocity, horizontal speed cap).
    pub fly: Option<(f32, f32)>,
    /// A velocity to set once (Phase Dash's launch).
    pub launch: Option<Vec3>,
    /// Movement speed multiplier (move_slow after a Phase Dash, dodge_speed_slow after a hover).
    pub speed_scale: f32,
    /// Seconds of an enemy Arc Wave's move_slow left.
    pub arc_slow: f32,
    /// Phase Dash in progress: BT isn't drawn (player::update_camera) and takes no damage.
    pub phased: bool,
}

/// Per-Titan kit state.
#[derive(Component)]
pub struct KitState {
    pub ord_cd: f32,
    pub ord_cd_max: f32,
    pub def_cd: f32,
    pub def_cd_max: f32,
    /// Utility slot: recharge of the next charge, charges held, and the running effect.
    pub util_cd: f32,
    pub util_cd_max: f32,
    pub util_charges: u32,
    prev_util: bool,
    /// Phase Dash: seconds left phased (and the fade of its move_slow after).
    pub phase: f32,
    phase_slow: f32,
    /// Hover: seconds flown so far and the flight's length (0 = not hovering), then the
    /// dodge_speed_slow fade.
    hover_t: f32,
    hover_len: f32,
    hover_slow: f32,
    /// airSpeed for this flight (250 for the Hover ability, 200 for the Flight Core).
    hover_horiz: f32,
    /// Laser Tripwire groups deployed (each use is one group of three pylons).
    trip_group: u32,
    phase_hidden: bool,
    /// Ion's shared energy.
    pub energy: f32,
    pub energy_delay: f32,
    /// Sword Block held (BT can't fire).
    pub blocking: bool,
    /// Gun Shield: seconds and health left.
    pub gun_shield: f32,
    pub gun_shield_hp: f32,
    gun_vis: Option<Entity>,
    gun_vis_fp: bool,
    pub melee_damage: f32,
    pub melee_range: f32,
    prev_ord: bool,
    /// Time left until a pressed ordnance fires (its deploy).
    pub ord_pending: Option<f32>,
    /// Power Shot armed: seconds charged so far (the charge runs once the gun is zoomed in).
    pub power: Option<f32>,
    /// Power Shot's charge effect on the barrel.
    power_fx: Option<Entity>,
    prev_def: bool,
    /// Rockets waiting to launch: target and how they fly, at `queue_rate` per second.
    queue: Vec<(Option<Entity>, MissileSpec)>,
    queue_rate: f32,
    queue_timer: f32,
    pod_right: bool,
    core_was_active: bool,
    /// The Laser Core's beam and end-glow systems while it fires.
    laser_fx: Option<(Entity, Entity)>,
    core_tick: f32,
    /// Tone's tracker marks: each marked enemy with the seconds left on each of its marks.
    pub locks: Vec<(Entity, Vec<f32>)>,
    rng: u64,
}

impl Default for KitState {
    fn default() -> Self {
        Self {
            ord_cd: 0.0,
            ord_cd_max: 1.0,
            def_cd: 0.0,
            def_cd_max: 1.0,
            util_cd: 0.0,
            util_cd_max: 1.0,
            util_charges: 1,
            prev_util: false,
            phase: 0.0,
            phase_slow: 0.0,
            hover_t: 0.0,
            hover_len: 0.0,
            hover_slow: 0.0,
            hover_horiz: HOVER_HORIZ,
            trip_group: 0,
            phase_hidden: false,
            energy: 1000.0,
            energy_delay: 0.0,
            blocking: false,
            gun_shield: 0.0,
            gun_shield_hp: 0.0,
            gun_vis: None,
            gun_vis_fp: false,
            melee_damage: 500.0,
            melee_range: 280.0,
            prev_ord: false,
            ord_pending: None,
            power: None,
            power_fx: None,
            prev_def: false,
            queue: Vec::new(),
            queue_rate: 12.0,
            queue_timer: 0.0,
            pod_right: false,
            core_was_active: false,
            laser_fx: None,
            core_tick: 0.0,
            locks: Vec::new(),
            rng: 0x2545_F491_4F6C_DD1D,
        }
    }
}

impl KitState {
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveKind {
    Arc,
    Firewall,
    Flame,
}

/// A fresh group id for the player's grouped effects (tripwire pylons, cluster bursts, ...):
/// a counter rather than the clock, so a scripted run plays the same every time.
fn next_group() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Group ids for enemy waves (above any player group).
static ENEMY_WAVE_GROUP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1 << 30);

/// An enemy Scorch's Firewall or Ronin's Arc Wave from `from` along `dir` (WeaponAttackWave
/// with the weapon files' wave_step_dist / wave_max_count; the Arc Wave's npc damage 50 to
/// Pilots and 1000 to Titans over its 112 radius, the Firewall's thermite patches).
pub fn spawn_enemy_wave(commands: &mut Commands, kind: WaveKind, from: Vec3, dir: Vec3) {
    let dir = dir.with_z(0.0).normalize_or(Vec3::X);
    let group = ENEMY_WAVE_GROUP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let wave = match kind {
        WaveKind::Arc => Wave {
            kind,
            pos: from + dir * 25.0,
            head: from + dir * 25.0,
            first: true,
            dir,
            step: ENEMY_ARC_STEP,
            left: ENEMY_ARC_COUNT,
            timer: 0.0,
            radius: 112.0,
            titan: ENEMY_ARC_TITAN,
            pilot: ENEMY_ARC_PILOT,
            group,
            enemy: true,
            trap: None,
            row: false,
        },
        _ => Wave {
            kind: WaveKind::Firewall,
            pos: from + dir * 25.0,
            head: from + dir * 25.0,
            first: true,
            dir,
            step: ENEMY_FIREWALL_STEP,
            left: ENEMY_FIREWALL_COUNT,
            timer: 0.0,
            radius: 60.0,
            titan: 0.0,
            pilot: 0.0,
            group,
            enemy: true,
            trap: None,
            row: false,
        },
    };
    log::info!("enemy {kind:?} wave from {:?}", from.round());
    commands.spawn(wave);
}

/// mp_titanweapon_flame_wall.txt / mp_titanweapon_arc_wave.txt for NPC Titans.
const ENEMY_FIREWALL_STEP: f32 = 100.0;
const ENEMY_FIREWALL_COUNT: u32 = 15;
/// FLAME_WALL_THERMITE_DURATION: an NPC's fire doesn't get the SP_FLAME_WALL_DURATION_SCALE.
const ENEMY_FIREWALL_THERMITE_SECS: f32 = 5.2;
const ENEMY_ARC_STEP: f32 = 112.0;
const ENEMY_ARC_COUNT: u32 = 15;
const ENEMY_ARC_TITAN: f32 = 1000.0;
const ENEMY_ARC_PILOT: f32 = 50.0;
/// ArcWaveOnDamage: move_slow 0.5 for 2 s.
const ARC_SLOW: f32 = 0.5;
const ARC_SLOW_SECS: f32 = 2.0;

/// A wave crawling along the ground (WeaponAttackWave).
#[derive(Component)]
pub struct Wave {
    kind: WaveKind,
    /// The last segment's ground position (lastDownPos).
    pos: Vec3,
    /// The forward trace's end (the script's `pos`), where the next step starts from.
    head: Vec3,
    first: bool,
    dir: Vec3,
    step: f32,
    left: u32,
    timer: f32,
    radius: f32,
    titan: f32,
    pilot: f32,
    /// Waves of one shot hit each enemy once (they share an inflictor).
    group: u32,
    /// An enemy Titan's wave: it hurts BT and the Pilot instead.
    enemy: bool,
    /// An ignited Slow Trap's fire line: the trap's position (its `P_meteor_trap_burn` runs
    /// from the line's end back to it).
    trap: Option<Vec3>,
    /// The Flame Core's middle wave, which carries each row's effects.
    row: bool,
}

/// A patch of Firewall thermite.
#[derive(Component)]
pub struct Thermite {
    pos: Vec3,
    life: f32,
    group: u32,
    /// An enemy Scorch's fire: burns BT and the Pilot instead.
    enemy: bool,
    /// Damage radius (FLAME_WALL_DAMAGE_RADIUS_DEF 60; an ignited Slow Trap's lines 75).
    radius: f32,
    /// Seconds to the next flame puff (TF_OLD_FX).
    flicker: f32,
    /// An ignited Slow Trap's segment: the trap's centre, and the seconds to its next damage
    /// tick (FireTrap_DamageAreaOverTime: a RadiusDamage, then wait 0.2).
    trap: Option<(Vec3, f32)>,
}

/// FireTrap_DamageAreaOverTime's wait between a segment's damage ticks.
const FIRE_TRAP_TICK: f32 = 0.2;

/// A piece of the T-203's thermite (Proto_MeteorCreatesThermite, mp_titanweapon_meteor.nut):
/// an invisible debris prop that falls, bounces and burns for its lifetime, dealing a meteor
/// tick within METEOR_THERMITE_DAMAGE_RADIUS_DEF (or the distance it moved, when faster) every
/// 0.1 s from 0.2 s after landing. Its gas-igniting reach is the same circle.
#[derive(Component)]
pub struct MeteorThermite {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    age: f32,
    /// At rest (the base piece stops where a shot hit the world: StopPhysics).
    still: bool,
    /// Where it was at the last damage tick.
    last: Vec3,
    tick: f32,
    /// The shot it came from: CreateOncePerTickDamageInflictorHelper lets each target take one
    /// tick per server tick from all of a shot's pieces together.
    shot: u32,
}

/// Fire this frame that can ignite Slow Trap gas from outside the kit systems: T-203 impact
/// blasts (pushed by the bolts, drained by `kit_world`) and burning thermite pieces.
#[derive(Resource, Default)]
pub struct FireSpots {
    pub blasts: Vec<(Vec3, f32)>,
    burning: Vec<(Vec3, f32)>,
}

/// METEOR_THERMITE_DAMAGE_RADIUS_DEF.
const METEOR_THERMITE_RADIUS: f32 = 45.0;
/// thermiteLifetimeMin/Max (2.0, 2.5) times SP_THERMITE_DURATION_SCALE for the player.
const METEOR_THERMITE_LIFE: (f32, f32) = (2.0 * 1.25, 2.5 * 1.25);

/// Proto_MeteorCreatesThermite: where a T-203 shell lands, a base piece (P_wpn_meteor_exp) and
/// four trails (P_wpn_meteor_exp_trail) thrown along the shell's velocity, its speed capped at
/// 2500 and scaled by 0.35 (singleplayer), the trails each nudged 50 along a random forward
/// and up. `vel` is the shell's velocity; `on_world` stops the base where it hit.
pub fn spawn_meteor_thermite(
    commands: &mut Commands,
    at: Vec3,
    vel: Vec3,
    on_world: bool,
    seed: u32,
) {
    let mut rng = seed | 1;
    let mut rand = move |lo: f32, hi: f32| {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        lo + (hi - lo) * (rng as f32 / u32::MAX as f32)
    };
    let v = vel.normalize_or_zero() * vel.length().min(2500.0) * 0.35;
    let shot = seed;
    let piece = |pos: Vec3, vel: Vec3, life: f32, still: bool| MeteorThermite {
        pos,
        vel,
        life,
        age: 0.0,
        still,
        last: pos,
        tick: 0.2,
        shot,
    };
    commands.spawn((
        Transform::from_translation(to_bevy(at)),
        crate::pfx::PfxTrail::oriented("P_wpn_meteor_exp"),
        piece(at, v, METEOR_THERMITE_LIFE.1, on_world),
    ));
    for _ in 0..4 {
        // AnglesToForward / AnglesToUp of random angles in -360..360.
        let (p, y, r) = (
            rand(-360.0, 360.0).to_radians(),
            rand(-360.0, 360.0).to_radians(),
            rand(-360.0, 360.0).to_radians(),
        );
        let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
        let fwd = Vec3::new(cp * cy, cp * sy, -sp);
        let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
        let life = rand(METEOR_THERMITE_LIFE.0, METEOR_THERMITE_LIFE.1);
        commands.spawn((
            Transform::from_translation(to_bevy(at)),
            crate::pfx::PfxTrail::oriented("P_wpn_meteor_exp_trail"),
            piece(at, v + fwd * 50.0 + up * 50.0, life, false),
        ));
    }
}

/// The T-203's thermite pieces: debris physics against the world (prop_physics in
/// TRACE_COLLISION_GROUP_DEBRIS, so they pass through Titans), burning and damage ticks.
pub fn meteor_thermite(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    defs: Option<Res<KitDefs>>,
    mut spots: ResMut<FireSpots>,
    mut pieces: Query<(Entity, &mut MeteorThermite, &mut Transform)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    mut cores: Query<&mut TitanCore, With<PlayerTitan>>,
) {
    let dt = time.delta_secs().min(0.1);
    spots.burning.clear();
    let (tick_titan, tick_pilot) = defs.as_ref().map_or((100.0, 20.0), |d| {
        (d.n.thermite_tick, d.n.thermite_tick_pilot)
    });
    let mut burned: Vec<(u32, Entity)> = Vec::new();
    for (id, mut t, mut tf) in &mut pieces {
        t.age += dt;
        if t.age >= t.life {
            commands.entity(id).despawn();
            continue;
        }
        if !t.still {
            // Bounce and friction are guesses (the engine's debris physics, empty_physics.mdl).
            t.vel.z -= DEPLOYABLE_GRAVITY * dt;
            let step = t.vel * dt;
            let len = step.length();
            if len > 1e-4 {
                let d = step / len;
                match world.0.raycast(
                    SVec3::from(t.pos.to_array()),
                    SVec3::from(d.to_array()),
                    len + 2.0,
                ) {
                    Some(h) => {
                        let n = Vec3::from(h.normal.to_array());
                        t.pos += d * (h.t - 2.0).max(0.0);
                        let vn = t.vel.dot(n);
                        let tangent = (t.vel - n * vn) * (-dt * 6.0).exp();
                        t.vel = tangent - n * vn.min(0.0) * 0.25;
                        if n.z > 0.7 && t.vel.length() < 30.0 {
                            t.still = true;
                        }
                    }
                    None => t.pos += step,
                }
            }
            tf.translation = to_bevy(t.pos);
        }
        // "thermite falls and ignites" after 0.2 s, then burns every 0.1 s.
        if t.age < 0.2 {
            continue;
        }
        let radius = METEOR_THERMITE_RADIUS.max(t.pos.distance(t.last));
        spots.burning.push((t.pos, radius));
        t.tick -= dt;
        if t.tick > 0.0 {
            continue;
        }
        t.tick += 0.1;
        t.last = t.pos;
        let Ok(mut core) = cores.single_mut() else {
            continue;
        };
        for (eid, mut e) in enemies.iter_mut() {
            if !e.alive() || burned.contains(&(t.shot, eid)) {
                continue;
            }
            let d = (e.pos - t.pos).truncate().length() - e.radius;
            if d < radius && t.pos.z > e.pos.z - radius && t.pos.z < e.pos.z + e.height + radius {
                burned.push((t.shot, eid));
                let amount = if e.infantry { tick_pilot } else { tick_titan };
                let hit = e.damage_radius(amount, false);
                core.credit_inflicted(hit);
            }
        }
    }
}

/// Flame Core: each row's effects reach FLAME_WALL_MAX_HEIGHT (110) past the outer waves,
/// PROJECTILE_SEPARATION (128) from the middle one (mp_titancore_flame_wave.nut).
const FLAME_ROW_HALF_WIDTH: f32 = 128.0 + 110.0;

#[derive(Component)]
pub struct ClusterRocket {
    pos: Vec3,
    vel: Vec3,
    age: f32,
}

/// The Cluster Missile's bursts (StartClusterExplosions / ClusterRocketBursts): the
/// CLUSTER_BASE_FX at the hit, then after `projectile_explosion_delay` a group of burst
/// projectiles (CreateClusterBurst: P_wpn_dumbfire_burst_trail on a grenade) that each hop from
/// pop to pop around the hit, exploding at every landing with the exp_rocket_cluster_secondary
/// table. Everything stops CLUSTER_ROCKET_DURATION after the bursts start.
#[derive(Component)]
pub struct ClusterBursts {
    center: Vec3,
    normal: Vec3,
    delay: f32,
    life: f32,
    base: Option<Entity>,
    hops: Vec<ClusterHop>,
    rng: u64,
}

/// One burst projectile: flying ballistically (GetVelocityForDestOverTime, gravity 750) from
/// `from` to `to` over `dur`, with `left` hops after this one.
struct ClusterHop {
    ent: Entity,
    from: Vec3,
    to: Vec3,
    vel: Vec3,
    t: f32,
    dur: f32,
    left: u32,
}

/// CLUSTER_ROCKET_BURST_GROUP_SIZE, _BURST_DELAY and _BURST_OFFSET are code-side constants the
/// scripts don't show; these are guesses (a group of 5 hops every 1.25 s over the 5 s).
const CLUSTER_GROUP_SIZE: u32 = 5;
const CLUSTER_BURST_DELAY: f32 = 0.25;
const CLUSTER_BURST_OFFSET: f32 = 0.2;
/// mp_titanweapon_dumbfire_rockets' projectile_explosion_delay.
const CLUSTER_EXPLOSION_DELAY: f32 = 0.5;

/// A Particle Wall: the shield arc and P_xo_shield_wall around `origin` (the owner's feet
/// less 64; an NPC's goes 100 ahead first), facing `normal`.
#[derive(Component)]
pub struct ParticleWall {
    origin: Vec3,
    normal: Vec3,
    life: f32,
    hp: f32,
    /// An enemy Tone's wall: it stops the player's shots instead of enemy rounds.
    enemy: bool,
}

impl ParticleWall {
    /// An enemy Titan's wall (SHIELD_WALL_HEALTH 1750, SP_PARTICLE_WALL_DURATION 8).
    pub fn enemy(feet: Vec3, normal: Vec3) -> Self {
        Self {
            origin: feet + normal * 100.0 - Vec3::Z * 64.0,
            normal,
            life: 8.0,
            hp: 1750.0,
            enemy: true,
        }
    }

    /// Put up another player's wall (net.rs): `enemy` when it is an opponent's.
    pub fn spawn(
        commands: &mut Commands,
        origin: Vec3,
        normal: Vec3,
        life: f32,
        hp: f32,
        enemy: bool,
    ) {
        commands.spawn((
            Transform::from_translation(to_bevy(origin)),
            Visibility::default(),
            ParticleWall {
                origin,
                normal,
                life,
                hp,
                enemy,
            },
        ));
    }
}

/// Enemy Particle Walls, for the player's shots and missiles to stop at (rebuilt each frame).
#[derive(Resource, Default)]
pub struct EnemyWalls {
    pub walls: Vec<BoltShield>,
    /// (wall entity, damage) since last frame.
    pub absorbed: Vec<(Entity, f32)>,
}

impl EnemyWalls {
    /// First wall the segment `from + dir * t, t < max` passes through from its front.
    pub fn hit(&self, from: Vec3, dir: Vec3, max: f32) -> Option<(Entity, f32)> {
        let mut best: Option<(Entity, f32)> = None;
        for w in &self.walls {
            if let Some(t) = w
                .hit_t(from, dir, max)
                .filter(|&t| best.is_none_or(|b| t < b.1))
            {
                best = w.owner.map(|e| (e, t));
            }
        }
        best
    }
}

#[derive(Component)]
pub struct TetherTrap {
    pos: Vec3,
    arm: f32,
    victim: Option<(Entity, Vec3, f32)>,
    life: f32,
}

fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    )
}

/// ApplyTrackerMark (mp_titanweapon_40mm.nut): one more tracker mark on `target`, lasting
/// TRACKER_LIFETIME; the owner hears HUD_40mm_TrackerBeep_Locked at three, _Hit below. With three
/// marks already, the oldest is renewed (a guess: the engine's SmartAmmo tracker list isn't in the
/// scripts) and nothing plays, as the script returns when the count doesn't change.
fn apply_tracker_mark(
    commands: &mut Commands,
    locks: &mut Vec<(Entity, Vec<f32>)>,
    target: Entity,
) {
    let marks = match locks.iter_mut().find(|l| l.0 == target) {
        Some(l) => &mut l.1,
        None => {
            locks.push((target, Vec::new()));
            &mut locks.last_mut().unwrap().1
        }
    };
    if marks.len() >= TRACKER_MAX {
        if let Some(oldest) = marks.iter_mut().min_by(|a, b| a.total_cmp(b)) {
            *oldest = TRACKER_LIFETIME;
        }
        return;
    }
    marks.push(TRACKER_LIFETIME);
    log::debug!("tone tracker marks on {target:?}: {}", marks.len());
    crate::audio::event(
        commands,
        if marks.len() >= TRACKER_MAX {
            "HUD_40mm_TrackerBeep_Locked"
        } else {
            "HUD_40mm_TrackerBeep_Hit"
        },
    );
}

/// Damage an enemy with titan-armour or pilot damage and credit BT's core.
fn hurt(e: &mut Enemy, titan: f32, pilot: f32, core: &mut TitanCore) {
    let amount = if e.infantry { pilot } else { titan };
    let hit = e.damage(amount, false);
    log::debug!(
        "kit hit {} for {:.0}",
        if e.infantry { "grunt" } else { "titan" },
        hit.dealt
    );
    core.credit_inflicted(hit);
}

/// Hitscan along `dir`: world distance and the first enemy in the way.
fn trace(
    world: &Collision,
    enemies: &Query<(Entity, &mut Enemy)>,
    from: Vec3,
    dir: Vec3,
    max: f32,
    pad: f32,
) -> (f32, Option<Entity>) {
    let mut t = world
        .0
        .raycast(
            SVec3::from(from.to_array()),
            SVec3::from(dir.to_array()),
            max,
        )
        .map(|h| h.t)
        .unwrap_or(max);
    let mut who = None;
    for (id, e) in enemies.iter() {
        if e.alive() {
            if let Some(h) = ray_cylinder(from, dir, e.pos, e.radius + pad, e.height) {
                if h < t {
                    t = h;
                    who = Some(id);
                }
            }
        }
    }
    (t, who)
}

fn splash(
    enemies: &mut Query<(Entity, &mut Enemy)>,
    at: Vec3,
    inner: f32,
    radius: f32,
    titan: f32,
    pilot: f32,
    core: &mut TitanCore,
) {
    for (_, mut e) in enemies.iter_mut() {
        if !e.alive() {
            continue;
        }
        let d = ((e.pos + Vec3::Z * e.height * 0.5) - at).length() - e.radius;
        if d < radius {
            let k = 1.0 - ((d - inner) / (radius - inner).max(1.0)).clamp(0.0, 1.0);
            let amount = if e.infantry { pilot } else { titan } * k;
            let hit = e.damage(amount, false);
            log::debug!(
                "kit splash {} for {:.0}",
                if e.infantry { "grunt" } else { "titan" },
                hit.dealt
            );
            core.credit_inflicted(hit);
        }
    }
}

/// Load the kit table once, and apply the selected loadout's kit when it changes.
#[allow(clippy::too_many_arguments)]
pub fn apply_kit(
    mut commands: Commands,
    gd: Res<GameData>,
    strings: Option<Res<crate::pilotweapon::Strings>>,
    defs: Option<Res<KitDefs>>,
    mut active: ResMut<ActiveKit>,
    mut loaded: Local<bool>,
    mut titans: Query<(
        &mut TitanCore,
        &mut Vortex,
        &mut KitState,
        &crate::weapons::Weapon,
    )>,
) {
    let Some(defs) = defs else {
        commands.insert_resource(KitDefs::load(&gd, strings.as_deref()));
        return;
    };
    let Ok((mut core, mut vortex, mut st, weapon)) = titans.single_mut() else {
        return;
    };
    // The loadout BT is holding (switch_titan_weapon swaps the gun, and the kit follows).
    let index = weapon.arsenal.min(defs.kits.len().saturating_sub(1));
    let spec = &defs.kits[index];
    let n = &defs.n;
    // A core in use finishes as the kit it started with; the loadout's own core takes over once
    // the meter is building again.
    if core.state == CoreState::Building && core.kind != spec.core {
        core.kind = spec.core;
        let id = CORES
            .iter()
            .find(|c| c.1 == spec.core)
            .map(|c| c.0)
            .unwrap_or("mp_titancore_amp_core");
        let (charge, duration, holster) =
            n.core_timing.get(id).copied().unwrap_or((1.0, 6.0, None));
        // A core that holsters the primary (Salvo, Laser) fires after the gun's holster and its
        // own draw, then its charge-up.
        core.chargeup = charge + holster.map_or(0.0, |d| weapon.def.holster_time + d);
        core.duration = duration;
    }
    if *loaded && index == active.index {
        return;
    }
    *loaded = true;
    *active = ActiveKit {
        index,
        ord: spec.ord,
        def: spec.def,
        util: spec.util,
        core: spec.core,
    };
    let un = n
        .util
        .get(
            UTILITY
                .iter()
                .find(|u| u.1 == spec.util)
                .map(|u| u.0)
                .unwrap_or("mp_titanability_smoke"),
        )
        .copied()
        .unwrap_or(UtilNumbers {
            cooldown: 20.0,
            charges: 1,
            duration: 0.0,
        });
    st.util_cd = 0.0;
    st.util_cd_max = un.cooldown;
    st.util_charges = un.charges;
    st.phase = 0.0;
    st.hover_len = 0.0;
    vortex.mode = match spec.def {
        DefKind::IonVortex => VortexMode::Ion,
        DefKind::HeatShield => VortexMode::Heat,
        _ => VortexMode::Vortex,
    };
    st.ord_cd = 0.0;
    st.def_cd = 0.0;
    st.blocking = false;
    st.gun_shield = 0.0;
    st.queue.clear();
    let ronin = spec.def == DefKind::SwordBlock;
    st.melee_damage = if ronin { n.sword_damage } else { 500.0 };
    st.melee_range = if ronin { n.sword_range } else { 280.0 };
    log::info!(
        "BT kit: {:?} / {:?} / {:?} / {:?}",
        spec.def,
        spec.util,
        spec.ord,
        spec.core
    );
}

/// Ordnance and defensive buttons, Ion's energy, core effects and rocket queues.
#[allow(clippy::too_many_arguments)]
pub fn kit_input(
    mut commands: Commands,
    time: Res<Time>,
    input: Res<PlayerInput>,
    mode: Res<CameraMode>,
    settings: Res<TitanSettings>,
    world: Res<Collision>,
    defs: Option<Res<KitDefs>>,
    active: Res<ActiveKit>,
    assets: Option<Res<MissileAssets>>,
    mut bolt_hits: ResMut<TitanBoltHits>,
    control: Res<crate::pilotctl::Control>,
    mut titans: Query<(
        &PlayerTitan,
        &mut KitState,
        &mut TitanCore,
        &mut TitanHealth,
        &Vortex,
        &mut crate::weapons::Weapon,
    )>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    mut mods: ResMut<TitanMoveMods>,
    (mut vis, tether_assets, gun_muzzle): (
        Query<&mut Visibility>,
        Option<Res<TetherAssets>>,
        Res<crate::weapons::GunMuzzle>,
    ),
) {
    let Some(defs) = defs else { return };
    let n = &defs.n;
    let dt = time.delta_secs().min(0.1);
    let Ok((titan, mut st, mut core, mut health, vortex, mut weapon)) = titans.single_mut() else {
        return;
    };
    let frozen = *mode == CameraMode::Free
        || health.dead_for.is_some()
        || *control != crate::pilotctl::Control::Titan;
    let s = &titan.state;
    let eye = Vec3::from(s.eye(&settings.0).to_array());
    let dir = view_dir(s.yaw, s.pitch);
    // Waves and walls go where BT aims, along the ground.
    let fwd = Vec3::new(dir.x, dir.y, 0.0).normalize_or(Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0));
    let right = Vec3::new(fwd.y, -fwd.x, 0.0);

    st.ord_cd = (st.ord_cd - dt).max(0.0);
    st.def_cd = (st.def_cd - dt).max(0.0);
    // Ion's energy regenerates after a short pause from the last use.
    if st.energy_delay > 0.0 {
        st.energy_delay -= dt;
    } else if !(vortex.active && active.def == DefKind::IonVortex) {
        st.energy = (st.energy + n.energy_regen * dt).min(n.energy_max);
    }
    if vortex.active && active.def == DefKind::IonVortex {
        st.energy = (st.energy - n.ion_vortex_drain * dt).max(0.0);
        st.energy_delay = n.energy_delay;
    }

    // Tone's tracker marks from 40mm damage (Tracker40mm_DamagedTarget -> ApplyTrackerMark: a
    // damage callback, so a round the Vortex caught or a shield soaked marks nothing). Only
    // Titans carry marks (the bolt code lists the Titans it damaged).
    for id in bolt_hits.0.drain(..) {
        if active.ord != OrdKind::Tracker || !enemies.get(id).is_ok_and(|(_, e)| e.alive()) {
            continue;
        }
        apply_tracker_mark(&mut commands, &mut st.locks, id);
    }
    for l in &mut st.locks {
        for t in &mut l.1 {
            *t -= dt;
        }
        l.1.retain(|t| *t > 0.0);
    }
    st.locks
        .retain(|l| !l.1.is_empty() && enemies.get(l.0).is_ok_and(|(_, e)| e.alive()));

    // --- Ordnance (Q): press to fire ---
    let pressed = input.ordnance && !st.prev_ord && !frozen;
    st.prev_ord = input.ordnance;
    let spec = &defs.kits[active.index.min(defs.kits.len() - 1)];
    // An offhand fires once it has deployed (the script's deploy_time; its viewmodel plays the
    // draw meanwhile). The Multi-Target Missiles lock on while held, and Tracker Rockets check
    // for a full lock when pressed.
    let deferred = !matches!(active.ord, OrdKind::Missiles | OrdKind::Tracker);
    let mut fire_now = pressed && st.ord_cd <= 0.0 && !deferred;
    if pressed
        && st.ord_cd <= 0.0
        && st.ord_pending.is_none()
        && deferred
        && (active.ord != OrdKind::LaserShot || st.energy >= n.laser_cost)
    {
        st.ord_pending = Some(spec.ord_deploy);
    }
    if let Some(t) = st.ord_pending {
        let t = t - dt;
        st.ord_pending = (t > 0.0).then_some(t);
        fire_now |= t <= 0.0 && !frozen;
    }
    if fire_now {
        let fired = match active.ord {
            OrdKind::Missiles => false,
            OrdKind::LaserShot if st.energy >= n.laser_cost => {
                st.energy -= n.laser_cost;
                st.energy_delay = n.energy_delay;
                st.ord_cd_max = n.laser_interval;
                st.ord_cd = n.laser_interval;
                let (t, who) = trace(&world, &enemies, eye, dir, 16000.0, 20.0);
                if let Some(id) = who {
                    if let Ok((_, mut e)) = enemies.get_mut(id) {
                        hurt(&mut e, n.laser_titan, n.laser_pilot, &mut core);
                    }
                }
                // From the laser pod's muzzle_flash at the end of its draw (atpov_titan_bt_laser_pod:
                // 62.5 ahead, 17.3 left and 11.8 above the camera bone): the script's
                // P_wpn_hand_laser_beam, and its exp_laser_lite impact table.
                let (a, b) = (
                    to_bevy(eye + dir * 62.5 - right * 17.3 + Vec3::Z * 11.8),
                    to_bevy(eye + dir * t),
                );
                if std::env::var_os("TF_OLD_FX").is_none() {
                    crate::pfx::emit_beam(&mut commands, "P_wpn_hand_laser_beam", a, b);
                    let surface = if who.is_some() {
                        crate::particles::Surface::Titan
                    } else {
                        crate::particles::Surface::World
                    };
                    emit(
                        &mut commands,
                        Effect::Hit {
                            at: b,
                            normal: (a - b).normalize_or(Vec3::Y),
                            table: "exp_laser_lite",
                            surface,
                            victim: false,
                            scale: 2.5,
                        },
                    );
                } else {
                    emit(
                        &mut commands,
                        Effect::Tracer {
                            from: a,
                            to: b,
                            width: 0.5,
                            color: Vec3::new(8.0, 1.5, 1.0),
                        },
                    );
                    emit(
                        &mut commands,
                        Effect::Impact {
                            at: b,
                            normal: (a - b).normalize_or(Vec3::Y),
                            scale: 2.5,
                            energy: true,
                        },
                    );
                }
                true
            }
            OrdKind::LaserShot => false,
            OrdKind::Firewall => {
                st.ord_cd_max = n.firewall_cd;
                st.ord_cd = n.firewall_cd;
                let group = next_group();
                commands.spawn(Wave {
                    kind: WaveKind::Firewall,
                    pos: Vec3::from(s.pos.to_array()) + fwd * 25.0 + Vec3::Z * 40.0,
                    head: Vec3::from(s.pos.to_array()) + fwd * 25.0 + Vec3::Z * 40.0,
                    first: true,
                    dir: fwd,
                    step: n.firewall_step,
                    left: n.firewall_count,
                    timer: 0.0,
                    radius: n.thermite_radius,
                    titan: 0.0,
                    pilot: 0.0,
                    group,
                    enemy: false,
                    trap: None,
                    row: false,
                });
                crate::audio::event(&mut commands, "flamewall_flame_start");
                true
            }
            OrdKind::Cluster => {
                st.ord_cd_max = n.cluster_cd;
                st.ord_cd = n.cluster_cd;
                let from = eye + right * 60.0 + dir * 80.0;
                let rocket = ClusterRocket {
                    pos: from,
                    vel: dir * n.cluster_speed,
                    age: 0.0,
                };
                let at = Transform::from_translation(to_bevy(from)).with_rotation(
                    Quat::from_rotation_arc(Vec3::Y, to_bevy(dir).normalize_or(Vec3::Y)),
                );
                if std::env::var_os("TF_OLD_FX").is_some() {
                    commands.spawn((at, Glow::new(Vec3::new(5.0, 3.0, 1.2), 0.5, true), rocket));
                } else {
                    // The script's projectile: projectilemodel and projectile_trail_effect_0.
                    commands.spawn((
                        at,
                        Visibility::default(),
                        rocket,
                        crate::pfx::PfxTrail::new("Rocket_Smoke_SMALL_Titan_2"),
                        crate::pilotweapon::ProjectileModel(
                            "models/weapons/bullets/projectile_rocket_largest.mdl",
                        ),
                    ));
                }
                true
            }
            OrdKind::ArcWave => {
                st.ord_cd_max = n.arc_cd;
                st.ord_cd = n.arc_cd;
                let group = next_group();
                commands.spawn(Wave {
                    kind: WaveKind::Arc,
                    pos: Vec3::from(s.pos.to_array()) + fwd * 25.0 + Vec3::Z * 40.0,
                    head: Vec3::from(s.pos.to_array()) + fwd * 25.0 + Vec3::Z * 40.0,
                    first: true,
                    dir: fwd,
                    step: n.arc_step,
                    left: n.arc_count,
                    timer: 0.0,
                    radius: 112.0,
                    titan: n.arc_titan,
                    pilot: n.arc_pilot,
                    group,
                    enemy: false,
                    trap: None,
                    row: false,
                });
                true
            }
            OrdKind::Tracker => {
                // Fire at the most-locked target near the crosshair; needs a full 3-stack lock.
                let target = st
                    .locks
                    .iter()
                    .filter(|l| l.1.len() >= TRACKER_MAX)
                    .filter_map(|l| {
                        enemies.get(l.0).ok().map(|(id, e)| {
                            (
                                id,
                                ((e.pos + Vec3::Z * e.height * 0.5) - eye)
                                    .normalize()
                                    .dot(dir),
                            )
                        })
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|t| t.0);
                if let Some(t) = target {
                    st.locks.retain(|l| l.0 != t);
                    st.ord_cd_max = n.tracker_delay;
                    st.ord_cd = n.tracker_delay;
                    st.queue_rate = n.tracker_rate;
                    let spec = MissileSpec {
                        speed: TRACKER_ROCKET_SPEED,
                        homing: crate::abilities::MTMS.homing * 2.0,
                        direct: n.tracker_damage,
                        splash: 0.0,
                        radius: 1.0,
                        out_time: 0.15,
                    };
                    for _ in 0..n.tracker_count {
                        st.queue.push((Some(t), spec));
                    }
                    log::info!("tracker rockets: {} at a locked target", n.tracker_count);
                    true
                } else {
                    false
                }
            }
            OrdKind::PowerShot => {
                // OnWeaponPrimaryAttack_power_shot: not while reloading or empty. The gun is zoomed
                // in by force (SetForcedADS) and its next round is the Power Shot, charged for the
                // mod's charge_time (charge_require_input 0, charge_end_forces_fire); see below.
                if weapon.reloading() || weapon.ammo == 0 || st.power.is_some() {
                    false
                } else {
                    st.ord_cd_max = n.power_cd;
                    st.ord_cd = n.power_cd;
                    st.power = Some(0.0);
                    weapon.set_alt_shot(true);
                    true
                }
            }
        };
        if fired {
            if !spec.ord_sound.is_empty() {
                crate::audio::event(&mut commands, &spec.ord_sound);
            }
            log::info!("ordnance: {:?}", active.ord);
        }
    }

    // --- Power Shot: zoomed in by force, then the charge (charge_seq, the mod's charge_effect
    // P_predator_barrel_blur_glow_FP on fx_barrel), then the shot (power_shot_fire_seq) ---
    if let Some(mut t) = st.power {
        if frozen {
            st.power = None;
            weapon.set_alt_shot(false);
            if let Some(Ok(mut c)) = st.power_fx.take().map(|e| commands.get_entity(e)) {
                c.despawn();
            }
        } else {
            if weapon.ads >= 0.999 {
                t += dt;
                weapon.play_charge();
                if st.power_fx.is_none() && std::env::var_os("TF_OLD_FX").is_none() {
                    st.power_fx = Some(
                        commands
                            .spawn((
                                Transform::default(),
                                crate::pfx::PfxTrail::oriented(if gun_muzzle.cockpit {
                                    "P_predator_barrel_blur_glow_FP"
                                } else {
                                    "P_predator_barrel_blur_glow"
                                }),
                            ))
                            .id(),
                    );
                    if gun_muzzle.cockpit {
                        commands
                            .entity(st.power_fx.unwrap())
                            .insert((crate::pfx::PfxVmSpace, crate::pfx::PfxOverlay));
                    }
                }
            }
            if let (Some(e), Some(j)) = (st.power_fx, gun_muzzle.barrel) {
                // On the fx_barrel joint, down the barrels: its Z (see weapon_viewmodel's barrel
                // blur).
                if let Ok(mut c) = commands.get_entity(e) {
                    c.insert((Transform::IDENTITY.looking_to(Vec3::Z, Vec3::Y), ChildOf(j)));
                }
            }
            st.power = Some(t);
            if t >= POWER_SHOT_CHARGE {
                st.power = None;
                weapon.set_alt_shot(false);
                weapon.play_alt_fire();
                if let Some(Ok(mut c)) = st.power_fx.take().map(|e| commands.get_entity(e)) {
                    c.despawn();
                }
                let (t, who) = trace(&world, &enemies, eye, dir, 12000.0, 30.0);
                let at = eye + dir * t;
                if weapon.long_range() {
                    // LongRangePowerShot: one straight round (200 / 2000 to Titans) with a
                    // 150-unit explosion (200 / 1800 heavy) where it lands.
                    if let Some(id) = who {
                        if let Ok((_, mut e)) = enemies.get_mut(id) {
                            hurt(&mut e, n.power_titan, n.power_pilot, &mut core);
                        }
                    }
                    splash(
                        &mut enemies,
                        at,
                        n.power_inner,
                        n.power_radius,
                        POWER_SHOT_SPLASH_HEAVY,
                        n.power_pilot,
                        &mut core,
                    );
                } else {
                    // CloseRangePowerShot: a 16 degree blast - everything in the cone with a
                    // clear line takes 200 (50 by 1600, from 800) / 2000 to Titans.
                    let half = (POWER_SHOT_CLOSE_SPREAD * 0.5f32).to_radians().cos();
                    for (_, mut e) in enemies.iter_mut() {
                        if !e.alive() {
                            continue;
                        }
                        let to = e.pos + Vec3::Z * e.height * 0.5 - eye;
                        let d = to.length();
                        if d > POWER_SHOT_CLOSE_FAR || to.normalize_or_zero().dot(dir) < half {
                            continue;
                        }
                        if world
                            .0
                            .raycast(
                                SVec3::from(eye.to_array()),
                                SVec3::from(to.normalize_or_zero().to_array()),
                                d - e.radius,
                            )
                            .is_some()
                        {
                            continue;
                        }
                        let k = ((d - POWER_SHOT_CLOSE_NEAR)
                            / (POWER_SHOT_CLOSE_FAR - POWER_SHOT_CLOSE_NEAR))
                            .clamp(0.0, 1.0);
                        hurt(
                            &mut e,
                            n.power_titan,
                            n.power_pilot + (POWER_SHOT_CLOSE_FAR_PILOT - n.power_pilot) * k,
                            &mut core,
                        );
                    }
                }
                let b = to_bevy(at);
                match gun_muzzle
                    .frame
                    .filter(|_| std::env::var_os("TF_OLD_FX").is_none())
                {
                    // The Power Shot mods' effects (mp_titanweapon_predator_cannon.txt): their
                    // muzzle flash, P_weapon_tracers_predator, and their impact table.
                    Some((a, rot)) => {
                        let long = weapon.long_range();
                        let flash = match (gun_muzzle.cockpit, long) {
                            (true, false) => "P_muzzleflash_predator_PS_FP",
                            (true, true) => "P_muzzleflash_predator_PS_FP_ALT",
                            (false, false) => "P_muzzleflash_predator_PS",
                            (false, true) => "P_muzzleflash_predator_PS_ALT",
                        };
                        if gun_muzzle.cockpit {
                            crate::pfx::emit_named_vm_frame(&mut commands, flash, a, rot);
                        } else {
                            crate::pfx::emit_named_frame(&mut commands, flash, a, rot);
                        }
                        crate::pfx::emit_beam(&mut commands, "P_weapon_tracers_predator", a, b);
                        let surface = match who.and_then(|id| enemies.get(id).ok()) {
                            Some((_, e)) if e.infantry => crate::particles::Surface::Flesh,
                            Some(_) => crate::particles::Surface::Titan,
                            None => crate::particles::Surface::World,
                        };
                        emit(
                            &mut commands,
                            Effect::Hit {
                                at: b,
                                normal: (a - b).normalize_or(Vec3::Y),
                                table: if long {
                                    "predator_bullet_ps_alt"
                                } else {
                                    "predator_bullet_ps"
                                },
                                surface,
                                victim: false,
                                scale: 2.5,
                            },
                        );
                    }
                    None => {
                        let a = to_bevy(eye + right * 50.0 - Vec3::Z * 40.0);
                        emit(
                            &mut commands,
                            Effect::Tracer {
                                from: a,
                                to: b,
                                width: 0.7,
                                color: Vec3::new(6.0, 4.0, 1.5),
                            },
                        );
                        emit(&mut commands, Effect::Explosion { at: b, scale: 1.4 });
                        crate::audio::cue(
                            &mut commands,
                            crate::audio::Cue::MissileExplode,
                            Some(at),
                        );
                    }
                }
            }
        }
    }

    // --- Defensive (E) other than the Vortex family (combat.rs) ---
    let held = input.vortex && !frozen;
    let def_pressed = held && !st.prev_def;
    st.prev_def = held;
    st.blocking = active.def == DefKind::SwordBlock && held;
    let sword_core = core.kind == CoreKind::Sword && core.active();
    health.block = if st.blocking {
        if sword_core {
            n.block_scale_core
        } else {
            n.block_scale
        }
    } else {
        1.0
    };
    health.block_cone = st
        .blocking
        .then(|| (eye, Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0)));
    if def_pressed && st.def_cd <= 0.0 {
        let used = match active.def {
            DefKind::ParticleWall => {
                st.def_cd_max = n.wall_cd;
                st.def_cd = n.wall_cd;
                // Around BT, facing the view's heading (GetParticleWallAttackAnglesFromOwner).
                let origin = Vec3::from(s.pos.to_array()) - Vec3::Z * 64.0;
                let normal = Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0);
                log::debug!("particle wall up at {origin:.0} facing {normal:.2}");
                // The other players get it too (net.rs).
                if let Ok(mut w) = crate::net::WALLS.lock() {
                    w.push((origin, normal, n.wall_life, n.wall_hp));
                }
                commands.spawn((
                    Transform::from_translation(to_bevy(origin)),
                    Visibility::default(),
                    ParticleWall {
                        origin,
                        normal,
                        life: n.wall_life,
                        hp: n.wall_hp,
                        enemy: false,
                    },
                ));
                true
            }
            DefKind::Tether => {
                st.def_cd_max = n.tether_cd;
                st.def_cd = n.tether_cd;
                // Two tethers (SP), thrown up 0.3 and right ±0.2 of the view at 1000 u/s.
                let right = Vec3::new(dir.y, -dir.x, 0.0).normalize_or(Vec3::Y);
                for side in [-1.0, 1.0] {
                    let d = (dir + Vec3::Z * TETHER_OFFSET_UP + right * TETHER_OFFSET_RIGHT * side)
                        .normalize();
                    let r = |k: f32| {
                        ((eye.x + eye.y * k + side * 31.0).sin() * 360.0).to_radians() * 2.0
                    };
                    spawn_tether(
                        &mut commands,
                        tether_assets.as_deref(),
                        eye,
                        TetherShot {
                            pos: eye,
                            vel: d * TETHER_SPEED,
                            life: 10.0,
                            spin: Vec3::new(r(1.7), r(2.3), r(3.1)),
                        },
                    );
                }
                true
            }
            DefKind::GunShield => {
                st.def_cd_max = n.gun_shield_cd;
                st.def_cd = n.gun_shield_cd;
                st.gun_shield = n.gun_shield_life;
                st.gun_shield_hp = n.gun_shield_hp;
                true
            }
            _ => false,
        };
        if used {
            if !spec.def_sound.is_empty() {
                crate::audio::event(&mut commands, &spec.def_sound);
            }
            log::info!("defensive: {:?}", active.def);
        }
    }
    st.gun_shield = (st.gun_shield - dt).max(0.0);
    if st.gun_shield_hp <= 0.0 {
        st.gun_shield = 0.0;
    }
    // GunShieldThink: the gun is zoomed in by force while the shield stands.
    weapon.forced_ads = st.gun_shield > 0.0;

    // --- Utility (E): the antirodeo slot ---
    let un = n
        .util
        .get(
            UTILITY
                .iter()
                .find(|u| u.1 == active.util)
                .map(|u| u.0)
                .unwrap_or(""),
        )
        .copied()
        .unwrap_or(UtilNumbers {
            cooldown: 20.0,
            charges: 1,
            duration: 0.0,
        });
    if st.util_charges < un.charges {
        st.util_cd = (st.util_cd - dt).max(0.0);
        if st.util_cd <= 0.0 {
            st.util_charges += 1;
            st.util_cd = if st.util_charges < un.charges {
                un.cooldown
            } else {
                0.0
            };
        }
    }
    let util_held = input.utility && !frozen;
    let util_pressed = util_held && !st.prev_util;
    st.prev_util = util_held;
    let bt_pos = Vec3::from(s.pos.to_array());
    if util_pressed && st.util_charges > 0 && st.phase <= 0.0 && st.hover_len <= 0.0 {
        let used = match active.util {
            UtilKind::Smoke => {
                // TitanSmokescreen: 240 units ahead when the line there is clear, else on BT.
                let ahead = bt_pos + fwd * 240.0;
                let to = ahead + Vec3::Z * 60.0 - eye;
                let clear = world
                    .0
                    .raycast(
                        SVec3::from(eye.to_array()),
                        SVec3::from(to.normalize_or_zero().to_array()),
                        to.length(),
                    )
                    .is_none();
                let at = if clear { ahead } else { bt_pos };
                spawn_smoke(&mut commands, at, fwd, false);
                true
            }
            UtilKind::SonarPulse => {
                // FireSonarPulse: FireGenericBoltWithDrop, the echo pulsing where it lands
                // (`sonar_shots`). It leaves the shoulder pod (this spot right of the view stands
                // in for its muzzle) aimed at what the crosshair is on (projectile_adjust_to_gun_
                // barrel), 1 degree higher (projectile_launch_pitch_offset).
                let from = eye + right * 60.0 + dir * 80.0;
                let reach = world
                    .0
                    .raycast(
                        SVec3::from(eye.to_array()),
                        SVec3::from(dir.to_array()),
                        20000.0,
                    )
                    .map_or(20000.0, |h| h.t);
                let aim = (eye + dir * reach - from).normalize_or(dir);
                let d = view_dir(aim.y.atan2(aim.x), (-aim.z).asin() - 1f32.to_radians());
                let shot = SonarShot {
                    pos: from,
                    vel: d * SONAR_SHOT_SPEED,
                    age: 0.0,
                };
                let at = Transform::from_translation(to_bevy(from)).with_rotation(
                    Quat::from_rotation_arc(Vec3::Y, to_bevy(d).normalize_or(Vec3::Y)),
                );
                commands.spawn((
                    at,
                    Visibility::default(),
                    shot,
                    crate::pfx::PfxTrail::new("wpn_grenade_sonar_titan"),
                    crate::pilotweapon::ProjectileModel(
                        "models/weapons/kunai/w_kunai_projectile.mdl",
                    ),
                ));
                crate::audio::event(&mut commands, "Titan_Tone_SonarLock_Fired_ShoulderPod_1P");
                true
            }
            UtilKind::PhaseDash => {
                // PhaseShift for 1 s, launched along the movement input at PHASE_DASH_SPEED (+200 up).
                st.phase = PHASE_DASH_TIME;
                let wish = fwd * input.forward + right * input.right;
                let launch = if wish.length() > 0.1 {
                    wish.normalize()
                } else {
                    fwd
                };
                mods.launch = Some(launch * PHASE_DASH_SPEED + Vec3::Z * 200.0);
                crate::audio::event(&mut commands, "Stryder.Dash");
                true
            }
            UtilKind::Hover => {
                st.hover_t = 0.0;
                st.hover_len = un.duration.max(0.1);
                st.hover_horiz = HOVER_HORIZ;
                emit(
                    &mut commands,
                    Effect::DustRing {
                        at: to_bevy(bt_pos),
                        radius: 4.0,
                    },
                );
                true
            }
            UtilKind::SlowTrap => {
                // ThrowDeployable at 1500 u/s; the canister builds for 1 s and gasses 240 units for 12 s.
                let p = eye + dir * 40.0;
                commands.spawn((
                    Transform::from_translation(to_bevy(p)),
                    Glow::new(Vec3::new(2.0, 1.2, 0.3), 0.3, true),
                    SlowTrap {
                        pos: p,
                        vel: dir * 1500.0,
                        landed: false,
                        build: SLOW_TRAP_BUILD_TIME,
                        life: SLOW_TRAP_LIFETIME,
                        puff: 0.0,
                    },
                ));
                true
            }
            UtilKind::LaserTrip => {
                // OnWeaponPrimaryAttack_titanweapon_laser_trip: three deployables along the view
                // minus, along and plus right = dir x up (as long as the view's level part, so
                // the sides are always 45 degrees out), the view tipped down to at least z -0.2;
                // 1200 u/s to the sides, 900 straight. ThrowDeployable aims each 8 degrees higher
                // and throws up to three times harder looking down (GetDeployableThrowVelocity).
                st.trip_group += 1;
                let group = st.trip_group;
                let side = dir.cross(Vec3::Z);
                let mut d = dir;
                d.z = d.z.min(-0.2);
                for (id, (k, power)) in [
                    (d - side, LASER_TRIP_DEPLOY_SIDE_POWER),
                    (d, LASER_TRIP_DEPLOY_POWER),
                    (d + side, LASER_TRIP_DEPLOY_SIDE_POWER),
                ]
                .into_iter()
                .enumerate()
                {
                    let yaw = k.y.atan2(k.x);
                    let pitch = (-k.z).atan2(k.truncate().length()).to_degrees() - 8.0;
                    let power = if pitch < 80.0 {
                        power * (1.0 + 2.0 * (pitch / 80.0).clamp(0.0, 1.0))
                    } else {
                        power
                    };
                    // The game throws from BT's TITAN_GRENADE attachment plus 50 along it; this
                    // spot below and right of the eye stands in for it.
                    let p = eye + Vec3::Z * -15.0 + right * 25.0;
                    let vel = view_dir(yaw, pitch.to_radians()) * power;
                    commands.spawn((
                        Transform::from_translation(to_bevy(p)),
                        Visibility::default(),
                        Glow::new(Vec3::new(1.5, 3.0, 6.0), 0.3, false),
                        LaserPylon {
                            pos: p,
                            vel,
                            landed: false,
                            build: LASER_TRIP_BUILD_TIME,
                            life: LASER_TRIP_LIFETIME,
                            group,
                            id: id as u8,
                            yaw,
                            link: None,
                        },
                    ));
                }
                true
            }
            UtilKind::AmmoSwap => {
                // ToggleAmmoMods: the Predator Cannon swaps its LongRangeAmmo mod in or out
                // (ADS spread 0.4, 3000/3250 falloff, zoom 40 against 3.4 / 1200/1800) with a
                // full clip, and the viewmodel plays ammo_swap_seq.
                let long = weapon.long_range();
                if weapon.def.name == "mp_titanweapon_predator_cannon" {
                    weapon.def = defs.predator[if long { 0 } else { 1 }].clone();
                }
                weapon.ammo = weapon.def.clip;
                weapon.reload_left = 0.0;
                weapon.play_ammo_swap();
                crate::audio::event(
                    &mut commands,
                    if long {
                        "weapon_predator_rangeswitch_toshort_1p"
                    } else {
                        "weapon_predator_rangeswitch_tolong_1p"
                    },
                );
                log::info!("ammo swap: {} range", if long { "close" } else { "long" });
                true
            }
        };
        if used {
            // TF_NO_COOLDOWN (testing): the utility keeps its charges.
            if std::env::var_os("TF_NO_COOLDOWN").is_none() {
                st.util_charges -= 1;
            }
            if st.util_cd <= 0.0 {
                st.util_cd = un.cooldown;
            }
            if !spec.util_sound.is_empty() {
                crate::audio::event(&mut commands, &spec.util_sound);
            }
            log::info!(
                "utility: {:?} ({} charges left)",
                active.util,
                st.util_charges
            );
        }
    }
    // Phase Dash: phased BT takes no damage and isn't drawn; move_slow 0.6 fades over 0.5 s after.
    if st.phase > 0.0 {
        st.phase -= dt;
        health.block = 0.0;
        if st.phase <= 0.0 {
            st.phase_slow = PHASE_FADE;
        }
    }
    mods.phased = st.phase > 0.0;
    if !mods.phased && st.phase_hidden {
        // Back from a phase in third person: show BT again (the cockpit view keeps it hidden).
        if let Ok(mut v) = vis.get_mut(titan.actor) {
            v.set_if_neq(if *mode == CameraMode::Cockpit {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    }
    st.phase_hidden = mods.phased;
    st.phase_slow = (st.phase_slow - dt).max(0.0);
    // Hover (FlyerHovers): rise at 225..450 u/s for 0.5 s, ease to 70 by 1.25 s, hold until the
    // flight ends; horizontal speed capped at 250; dodge_speed_slow 0.65 fading 0.75 s after.
    mods.fly = None;
    if st.hover_len > 0.0 {
        st.hover_t += dt;
        let t = st.hover_t;
        let vz = if t < HOVER_LERP_IN {
            HOVER_RISE * 0.5 + (HOVER_RISE - HOVER_RISE * 0.5) * (t / HOVER_LERP_IN)
        } else if t < HOVER_LERP_IN + 0.75 {
            HOVER_RISE + (70.0 - HOVER_RISE) * ((t - HOVER_LERP_IN) / 0.75)
        } else {
            70.0
        };
        if t >= st.hover_len
            || health.dead_for.is_some()
            || *control != crate::pilotctl::Control::Titan
        {
            st.hover_len = 0.0;
            st.hover_slow = HOVER_FADE;
        } else {
            mods.fly = Some((vz, st.hover_horiz + 50.0));
        }
    }
    st.hover_slow = (st.hover_slow - dt).max(0.0);
    let phase_k = if st.phase > 0.0 {
        1.0
    } else {
        st.phase_slow / PHASE_FADE
    };
    let hover_k = if st.hover_len > 0.0 {
        1.0
    } else {
        st.hover_slow / HOVER_FADE
    };
    mods.arc_slow = (mods.arc_slow - dt).max(0.0);
    mods.speed_scale = (1.0 - 0.6 * phase_k)
        .min(1.0 - 0.65 * hover_k)
        .min(if mods.arc_slow > 0.0 { ARC_SLOW } else { 1.0 });

    // --- Cores ---
    // TF_CORE_FULL=1 keeps the meter full (testing cores in scripted runs).
    if core.state == CoreState::Building && std::env::var_os("TF_CORE_FULL").is_some() {
        core.meter = 1.0;
    }
    let core_on = core.active();
    if core_on && !st.core_was_active {
        st.core_tick = 0.0;
        match core.kind {
            CoreKind::FlameWave => {
                let group = next_group();
                let base = Vec3::from(s.pos.to_array()) + fwd * 25.0;
                for (off, sfx) in [
                    (-1.0, "flamewave_blast_right"),
                    (0.0, "flamewave_blast_middle"),
                    (1.0, "flamewave_blast_left"),
                ] {
                    commands.spawn(Wave {
                        kind: WaveKind::Flame,
                        pos: base + right * off * 128.0,
                        head: base + right * off * 128.0,
                        first: true,
                        dir: fwd,
                        step: n.flame_step,
                        left: n.flame_count,
                        timer: 0.0,
                        radius: 180.0,
                        titan: n.flame_titan,
                        pilot: n.flame_pilot,
                        group,
                        enemy: false,
                        trap: None,
                        row: off == 0.0,
                    });
                    crate::audio::event(&mut commands, sfx);
                }
            }
            CoreKind::Salvo => {
                // Salvo Core: burst_fire_count homing rockets, spread over what's in front.
                let targets: Vec<Entity> = enemies
                    .iter()
                    .filter(|(_, e)| e.alive())
                    .filter(|(_, e)| {
                        ((e.pos + Vec3::Z * e.height * 0.5) - eye)
                            .normalize()
                            .dot(dir)
                            > 0.6
                    })
                    .map(|(id, _)| id)
                    .collect();
                st.queue_rate = n.salvo_rate;
                for i in 0..n.salvo_count as usize {
                    let t = if targets.is_empty() {
                        None
                    } else {
                        Some(targets[i % targets.len()])
                    };
                    st.queue.push((t, n.salvo_rocket));
                }
                log::info!(
                    "salvo core: {} rockets at {} targets",
                    n.salvo_count,
                    targets.len()
                );
            }
            CoreKind::Laser => crate::audio::event(&mut commands, "Titan_Core_Laser_FireBeam_1P"),
            CoreKind::Flight => {
                // PROTO_FlightCore: FlyerHovers for the core's duration, rockets after takeoff.
                st.hover_t = 0.0;
                st.hover_len = core.duration.max(0.1);
                st.hover_horiz = FLIGHT_CORE_HORIZ;
                emit(
                    &mut commands,
                    Effect::DustRing {
                        at: to_bevy(bt_pos),
                        radius: 4.0,
                    },
                );
                crate::audio::event(&mut commands, "titan_core_flight_liftoff_1p");
                log::info!("flight core: {:.1} s of flight", core.duration);
            }
            _ => {}
        }
    }
    st.core_was_active = core_on;
    if !(core_on && core.kind == CoreKind::Laser) {
        if let Some((beam, glow)) = st.laser_fx.take() {
            commands.entity(beam).despawn();
            commands.entity(glow).despawn();
        }
    }
    st.melee_damage = if active.def == DefKind::SwordBlock {
        n.sword_damage + if sword_core { n.sword_core_bonus } else { 0.0 }
    } else {
        500.0
    };
    if core_on && !frozen {
        match core.kind {
            CoreKind::Flight if matches!(core.state, CoreState::Active(t) if t < FLIGHT_CORE_TAKEOFF) =>
                {}
            CoreKind::Flight => {
                // Flight Core rockets at fire_rate from the shoulder pods, straight at the crosshair.
                st.core_tick -= dt;
                while st.core_tick <= 0.0 {
                    st.core_tick += 1.0 / n.flight_rate.max(1.0);
                    let jitter =
                        Vec3::new(st.rand() - 0.5, st.rand() - 0.5, st.rand() - 0.5) * 0.06;
                    st.queue_rate = n.flight_rate;
                    if let Some(a) = assets.as_ref() {
                        st.pod_right = !st.pod_right;
                        let side = right * if st.pod_right { 70.0 } else { -70.0 };
                        spawn_missile(
                            &mut commands,
                            a,
                            eye + side + Vec3::Z * 40.0,
                            (dir + jitter).normalize(),
                            None,
                            n.flight_rocket,
                        );
                    }
                    crate::audio::cue(&mut commands, crate::audio::Cue::MissileFire, None);
                }
            }
            CoreKind::Laser => {
                // A continuous beam; each server frame it damages what it touches.
                let (t, who) = trace(
                    &world,
                    &enemies,
                    eye,
                    dir,
                    n.laser_core_range,
                    n.laser_core_radius,
                );
                // From the chest: the Laser Core viewmodel's muzzle_flash (atpov_laser_cannon:
                // 32 ahead of and 44 below the camera bone; its arms hang out of view).
                let (a, b) = (
                    to_bevy(eye + dir * 32.0 - Vec3::Z * 44.0),
                    to_bevy(eye + dir * t),
                );
                if std::env::var_os("TF_OLD_FX").is_none() {
                    // The script's sustained laser: P_wpn_lasercannon_FP from the muzzle to the
                    // hit point, P_lasercannon_endglow on the hit point, both kept running.
                    let (beam, glow) = *st.laser_fx.get_or_insert_with(|| {
                        (
                            commands
                                .spawn((
                                    Transform::from_translation(a),
                                    crate::pfx::PfxTrail::new("P_wpn_lasercannon_FP"),
                                    crate::pfx::PfxBeamEnd(b),
                                ))
                                .id(),
                            commands
                                .spawn((
                                    Transform::from_translation(b),
                                    crate::pfx::PfxTrail::new("P_lasercannon_endglow"),
                                ))
                                .id(),
                        )
                    });
                    commands.entity(beam).insert((
                        Transform::from_translation(a).looking_at(b, Vec3::Y),
                        crate::pfx::PfxBeamEnd(b),
                    ));
                    commands.entity(glow).insert(Transform::from_translation(b));
                } else {
                    emit(
                        &mut commands,
                        Effect::Tracer {
                            from: a,
                            to: b,
                            width: 1.2,
                            color: Vec3::new(2.0, 4.0, 8.0),
                        },
                    );
                    emit(
                        &mut commands,
                        Effect::Impact {
                            at: b,
                            normal: (a - b).normalize_or(Vec3::Y),
                            scale: 2.0,
                            energy: true,
                        },
                    );
                }
                st.core_tick -= dt;
                if st.core_tick <= 0.0 {
                    st.core_tick += SERVER_FRAME;
                    if let Some(id) = who {
                        if let Ok((_, mut e)) = enemies.get_mut(id) {
                            hurt(&mut e, n.laser_core_tick, n.laser_core_tick, &mut core);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // --- Rocket queues (Tracker Rockets, Salvo Core) ---
    if let Some(a) = assets.as_ref() {
        if !st.queue.is_empty() {
            st.queue_timer -= dt;
            while st.queue_timer <= 0.0 && !st.queue.is_empty() {
                st.queue_timer += 1.0 / st.queue_rate.max(1.0);
                let (target, spec) = st.queue.remove(0);
                st.pod_right = !st.pod_right;
                let side = right * if st.pod_right { 70.0 } else { -70.0 };
                let launch = eye + side + Vec3::Z * 40.0;
                // Out of the pods toward the target, fanned less the closer it is: at close
                // range the full fan carried the rockets past both sides of it.
                let (aim, fan) = match target.and_then(|t| enemies.get(t).ok()) {
                    Some((_, e)) => {
                        let to = e.pos + Vec3::Z * e.height * 0.6 - launch;
                        (
                            to.normalize_or(dir),
                            0.2 * (to.length() / 1500.0).clamp(0.0, 1.0),
                        )
                    }
                    None => (dir, 0.2),
                };
                let out = (aim + side.normalize() * fan + Vec3::Z * fan).normalize();
                spawn_missile(&mut commands, a, launch, out, target, spec);
                crate::audio::cue(&mut commands, crate::audio::Cue::MissileFire, None);
            }
        } else {
            st.queue_timer = 0.0;
        }
    }
}

/// Waves, thermite, cluster rockets and bursts, walls, tethers and the gun shield.
#[allow(clippy::too_many_arguments)]
pub fn kit_world(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    defs: Option<Res<KitDefs>>,
    mut shields: ResMut<BoltShields>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut waves: Query<(Entity, &mut Wave)>,
    mut thermite: Query<(Entity, &mut Thermite)>,
    mut rockets: Query<(Entity, &mut ClusterRocket, &mut Transform), Without<ParticleWall>>,
    mut bursts: Query<(Entity, &mut ClusterBursts)>,
    mut walls: Query<
        (Entity, &mut ParticleWall, &mut Transform, Option<&Children>),
        Without<ClusterRocket>,
    >,
    mut traps: Query<(Entity, &mut TetherTrap)>,
    mut titans: Query<(&PlayerTitan, &mut KitState, &mut TitanCore)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    (
        mut tick,
        mut hit_groups,
        gas,
        vortex,
        mut tethers,
        tether_assets,
        (
            control,
            difficulty,
            mut pilots,
            mut player,
            mut mods,
            mut enemy_walls,
            mode,
            gun_muzzle,
            mut spots,
        ),
    ): (
        Local<f32>,
        Local<HashMap<u32, Vec<Entity>>>,
        Query<(Entity, &SlowTrap)>,
        Query<&Vortex, With<PlayerTitan>>,
        Query<
            (Entity, &mut TetherShot, &mut Transform),
            (Without<ParticleWall>, Without<ClusterRocket>),
        >,
        Option<Res<TetherAssets>>,
        (
            Res<Control>,
            Res<crate::vitals::Difficulty>,
            Query<(
                Entity,
                &crate::pilotctl::PlayerPilot,
                &mut crate::pilotctl::PilotHealth,
            )>,
            Query<(Entity, &PlayerTitan, &mut TitanHealth)>,
            ResMut<TitanMoveMods>,
            ResMut<EnemyWalls>,
            Res<CameraMode>,
            Res<crate::weapons::GunMuzzle>,
            ResMut<FireSpots>,
        ),
    ),
) {
    let Some(defs) = defs else { return };
    let n = &defs.n;
    let dt = time.delta_secs().min(0.1);
    let Ok((titan, mut st, mut core)) = titans.single_mut() else {
        return;
    };

    // Damage the shields soaked up last frame.
    let owners: Vec<Option<Entity>> = shields.shields.iter().map(|s| s.owner).collect();
    for (i, dmg) in std::mem::take(&mut shields.absorbed) {
        match owners.get(i).copied().flatten() {
            Some(wall) => {
                if let Ok((_, mut w, _, _)) = walls.get_mut(wall) {
                    w.hp -= dmg;
                }
            }
            None => st.gun_shield_hp -= dmg,
        }
    }
    shields.shields.clear();

    // --- Waves ---
    let mut live_groups = Vec::new();
    for (id, mut w) in &mut waves {
        live_groups.push(w.group);
        w.timer -= dt;
        while w.timer <= 0.0 && w.left > 0 {
            w.timer += SERVER_FRAME;
            w.left -= 1;
            // WeaponAttackWave: trace forward (and 40 down after the first step) from 80 above
            // the last segment; if that's blocked, try climbing (up to step * 0.577) instead.
            // Then the ground is 1000 below at most, or the wave ends.
            let start = if w.first {
                w.head
            } else {
                w.pos + Vec3::Z * 80.0
            };
            let new = w.head + w.dir * w.step;
            let under = Vec3::new(new.x, new.y, if w.first { start.z } else { start.z - 40.0 });
            let over = Vec3::new(
                new.x,
                new.y,
                if w.first {
                    start.z
                } else {
                    start.z + w.step * 0.577_350_3
                },
            );
            let trace = |a: Vec3, b: Vec3| {
                let d = b - a;
                world
                    .0
                    .raycast(
                        SVec3::from(a.to_array()),
                        SVec3::from(d.normalize_or_zero().to_array()),
                        d.length(),
                    )
                    .map(|h| a + d.normalize_or_zero() * h.t)
            };
            let ground = |p: Vec3| {
                world
                    .0
                    .raycast(SVec3::from(p.to_array()), -SVec3::Z, 1000.0)
                    .map(|h| p - Vec3::Z * h.t)
            };
            let (head, seg) = match trace(start, under) {
                None => (under, ground(under)),
                Some(hit) => (
                    hit,
                    if trace(start, over).is_some() {
                        None
                    } else {
                        ground(over)
                    },
                ),
            };
            let Some(seg) = seg else {
                log::debug!(
                    "{:?} wave ended at {:?} ({} steps left)",
                    w.kind,
                    w.pos.round(),
                    w.left
                );
                w.left = 0;
                break;
            };
            // The last segment (the Firewall's savedOrigin), None at the first.
            let prev = (!w.first).then_some(w.pos);
            w.first = false;
            w.head = head;
            w.pos = seg;
            let at = to_bevy(w.pos + Vec3::Z * 20.0);
            let old_fx = std::env::var_os("TF_OLD_FX").is_some();
            // The scripts' effects stand on the segment (downTrace.endPos), facing the wave.
            let ground = Cp::facing(w.pos, w.dir);
            match w.kind {
                WaveKind::Arc if old_fx => emit(
                    &mut commands,
                    Effect::Impact {
                        at,
                        normal: Vec3::Y,
                        scale: 2.4,
                        energy: true,
                    },
                ),
                // CreateEmpWaveSegment.
                WaveKind::Arc => crate::pfx::emit_named_cp(&mut commands, "P_arcwave_exp", ground),
                WaveKind::Flame if old_fx => {
                    emit(&mut commands, Effect::Explosion { at, scale: 0.9 })
                }
                WaveKind::Flame => {
                    // CreateFlameWaveSegment: two P_wpn_meteor_waveCP per row, from the middle
                    // wave out to either side (control point 1), killed after 3 s.
                    if w.row {
                        let side =
                            w.dir.cross(Vec3::Z).normalize_or(Vec3::X) * FLAME_ROW_HALF_WIDTH;
                        for s in [side, -side] {
                            crate::pfx::emit_named_cps(
                                &mut commands,
                                "P_wpn_meteor_waveCP",
                                &[
                                    ground,
                                    Cp {
                                        pos: w.pos + s,
                                        ..ground
                                    },
                                ],
                                Some(3.0),
                            );
                        }
                    }
                }
                // CreateThermiteWallSegment / CreateSlowTrapSegment: the first step only notes
                // where the wave is.
                WaveKind::Firewall => {
                    if let Some(prev) = prev {
                        let life = if w.enemy {
                            ENEMY_FIREWALL_THERMITE_SECS
                        } else {
                            n.thermite_life
                        };
                        let patch = commands
                            .spawn((
                                Transform::from_translation(at),
                                Thermite {
                                    pos: w.pos,
                                    life,
                                    group: w.group,
                                    radius: w.radius,
                                    flicker: w.left as f32 * 0.13,
                                    enemy: w.enemy,
                                    trap: w.trap.map(|o| (o, 0.0)),
                                },
                            ))
                            .id();
                        // The effect runs from this segment back to the last one (control point
                        // 1); a Slow Trap line's runs from its end back to the trap once the line
                        // is laid (FlameOn). It goes with the patch.
                        log::debug!(
                            "thermite patch at {:?} (wave {:?} left {})",
                            w.pos.round(),
                            w.kind,
                            w.left
                        );
                        let fx = match w.trap {
                            None => Some(("P_wpn_meteor_wall", prev)),
                            Some(origin) if w.left == 0 => Some(("P_meteor_trap_burn", origin)),
                            Some(_) => None,
                        };
                        if old_fx {
                            emit(&mut commands, Effect::Explosion { at, scale: 0.45 });
                            commands.entity(patch).insert(Glow::new(
                                Vec3::new(6.0, 2.2, 0.4),
                                2.2,
                                true,
                            ));
                        } else if let Some((name, end)) = fx {
                            let local = to_bevy(w.pos) - at;
                            commands.entity(patch).with_child((
                                Transform::from_translation(local),
                                crate::pfx::PfxTrail::new(name),
                                crate::pfx::PfxBeamEnd(to_bevy(end)),
                            ));
                        }
                    }
                }
            }
            if w.enemy && w.kind != WaveKind::Firewall {
                // An enemy's Arc Wave: BT (unless phased) and the Pilot on foot, once each.
                let hit = hit_groups.entry(w.group).or_default();
                let scale = difficulty.damage_to_player();
                if let Ok((bid, bt, mut health)) = player.single_mut() {
                    let bp = Vec3::from(bt.state.pos.to_array());
                    let d = (bp - w.pos).truncate().length() - settings.0.radius;
                    if !hit.contains(&bid)
                        && !mods.phased
                        && health.dead_for.is_none()
                        && d < w.radius
                        && (w.pos.z - bp.z) < settings.0.height
                        && (bp.z - w.pos.z) < w.radius
                    {
                        hit.push(bid);
                        health.damage_from(w.titan * scale, true, w.pos);
                        mods.arc_slow = ARC_SLOW_SECS;
                        log::info!("enemy arc wave hit BT for {:.0}", w.titan * scale);
                    }
                }
                if *control == Control::Pilot {
                    if let Ok((pid, pilot, mut health)) = pilots.single_mut() {
                        let pp = Vec3::from(pilot.state.pos.to_array());
                        let d = (pp - w.pos).truncate().length() - 16.0;
                        if !hit.contains(&pid)
                            && d < w.radius
                            && (w.pos.z - pp.z) < 72.0
                            && (pp.z - w.pos.z) < w.radius
                        {
                            hit.push(pid);
                            health.damage_capped(
                                w.pilot * scale,
                                difficulty.max_pilot_damage_per_hit(),
                            );
                            log::info!("enemy arc wave hit the Pilot for {:.0}", w.pilot * scale);
                        }
                    }
                }
            } else if w.kind != WaveKind::Firewall {
                let hit = hit_groups.entry(w.group).or_default();
                for (eid, mut e) in enemies.iter_mut() {
                    if !e.alive() || hit.contains(&eid) {
                        continue;
                    }
                    let d = (e.pos - w.pos).truncate().length() - e.radius;
                    if d < w.radius
                        && (w.pos.z - e.pos.z) < e.height
                        && (e.pos.z - w.pos.z) < w.radius
                    {
                        hit.push(eid);
                        hurt(&mut e, w.titan, w.pilot, &mut core);
                    }
                }
            }
        }
        if w.left == 0 {
            commands.entity(id).despawn();
        }
    }

    // --- Thermite: each server frame, everything in the fire takes one tick per firewall ---
    *tick -= dt;
    let burn = *tick <= 0.0;
    if burn {
        *tick += SERVER_FRAME;
    }
    let mut burned: Vec<(u32, Entity)> = Vec::new();
    // Burning patches this frame, for igniting Slow Trap gas.
    let mut fire: Vec<(Vec3, f32)> = Vec::new();
    for (id, mut t) in &mut thermite {
        live_groups.push(t.group);
        t.life -= dt;
        if t.life <= 0.0 {
            commands.entity(id).despawn();
            continue;
        }
        fire.push((t.pos, t.radius));
        // The burning wall (TF_OLD_FX): flame puffs rising off each patch.
        t.flicker -= dt;
        if t.flicker <= 0.0 && std::env::var_os("TF_OLD_FX").is_some() {
            t.flicker += 0.35;
            emit(
                &mut commands,
                Effect::Explosion {
                    at: to_bevy(t.pos + Vec3::Z * 40.0),
                    scale: 0.4,
                },
            );
        }
        // A trap segment ticks every 0.2 s from when it lands; all of a trap's segments share
        // one CreateOncePerTickDamageInflictorHelper, so a target takes one tick per server
        // frame from the whole trap (keyed by its centre), and FireTrap_DamagedPlayerOrNPC
        // zeroes damage farther than SLOW_TRAP_RADIUS (2D) from the trap.
        if !burn {
            continue;
        }
        let mut key = t.group;
        let mut trap_center = None;
        if let Some((center, wait)) = t.trap.as_mut() {
            *wait -= SERVER_FRAME;
            if *wait > -1e-3 {
                continue;
            }
            *wait += FIRE_TRAP_TICK;
            key = center.x.to_bits() ^ center.y.to_bits().rotate_left(13);
            trap_center = Some(*center);
        }
        if t.enemy {
            // An enemy Scorch's fire: BT and the Pilot on foot take the thermite tick.
            let scale = difficulty.damage_to_player();
            if let Ok((bid, bt, mut health)) = player.single_mut() {
                let bp = Vec3::from(bt.state.pos.to_array());
                let d = (bp - t.pos).truncate().length() - settings.0.radius;
                if !burned.contains(&(t.group, bid))
                    && !mods.phased
                    && health.dead_for.is_none()
                    && d < t.radius
                    && (t.pos.z - bp.z) < settings.0.height
                    && (bp.z - t.pos.z) < 100.0
                {
                    burned.push((t.group, bid));
                    health.damage_from(n.thermite_tick * scale, true, t.pos);
                }
            }
            if *control == Control::Pilot {
                if let Ok((pid, pilot, mut health)) = pilots.single_mut() {
                    let pp = Vec3::from(pilot.state.pos.to_array());
                    let d = (pp - t.pos).truncate().length() - 16.0;
                    if !burned.contains(&(t.group, pid))
                        && d < t.radius
                        && (t.pos.z - pp.z) < 72.0
                        && (pp.z - t.pos.z) < 100.0
                    {
                        burned.push((t.group, pid));
                        health.damage_capped(
                            n.thermite_tick_pilot * scale,
                            difficulty.max_pilot_damage_per_hit(),
                        );
                    }
                }
            }
            continue;
        }
        for (eid, mut e) in enemies.iter_mut() {
            if !e.alive() || burned.contains(&(key, eid)) {
                continue;
            }
            if trap_center.is_some_and(|c| (e.pos - c).truncate().length() > SLOW_TRAP_RADIUS) {
                continue;
            }
            let d = (e.pos - t.pos).truncate().length() - e.radius;
            if d < t.radius && (t.pos.z - e.pos.z) < e.height && (e.pos.z - t.pos.z) < 100.0 {
                burned.push((key, eid));
                // RadiusDamage: shields that catch rounds don't stop it.
                let amount = if e.infantry {
                    n.thermite_tick_pilot
                } else {
                    n.thermite_tick
                };
                let hit = e.damage_radius(amount, false);
                core.credit_inflicted(hit);
            }
        }
    }
    hit_groups.retain(|g, _| live_groups.contains(g));

    // --- Slow Trap gas touched by fire (OnSlowTrapDamaged by thermite, the Firewall, the Flame
    // Core or the Thermal Shield): IgniteTrap - the cloud goes up and 12 lines of thermite run
    // out from it (FireWeaponGrenade at yaw 30 x i, the last six reversed, each from 75 out),
    // every patch a FIRE_TRAP_MINI_EXPLOSION_RADIUS 75 meteor tick for the Firewall's lifetime.
    // The T-203's impact blasts and burning thermite (mp_titanweapon_meteor and
    // mp_titanweapon_meteor_thermite damage both ignite it).
    fire.extend(std::mem::take(&mut spots.blasts));
    fire.extend(spots.burning.iter().copied());
    for (id, trap) in &gas {
        if !trap.landed || trap.build > 0.0 {
            continue;
        }
        let lit = fire.iter().any(|(p, r)| {
            (*p - trap.pos).truncate().length() < SLOW_TRAP_RADIUS + r
                && (p.z - trap.pos.z).abs() < 150.0
        }) || waves.iter().any(|(_, w)| {
            w.kind == WaveKind::Flame
                && (w.pos - trap.pos).truncate().length() < SLOW_TRAP_RADIUS + w.radius
                && (w.pos.z - trap.pos.z).abs() < 150.0
        }) || (vortex
            .single()
            .is_ok_and(|v| v.active && v.mode == VortexMode::Heat)
            && (Vec3::from(titan.state.pos.to_array()) - trap.pos)
                .truncate()
                .length()
                < SLOW_TRAP_RADIUS + 250.0);
        if !lit {
            continue;
        }
        commands.entity(id).despawn();
        let origin = trap.pos + Vec3::Z * SLOW_TRAP_GAS_HEIGHT;
        // IgniteTrap: P_meteor_trap_center at the gas, killed after 3 s.
        if std::env::var_os("TF_OLD_FX").is_some() {
            emit(
                &mut commands,
                Effect::Explosion {
                    at: to_bevy(origin),
                    scale: 1.6,
                },
            );
        } else {
            crate::pfx::emit_named_cps(
                &mut commands,
                "P_meteor_trap_center",
                &[Cp::facing(origin, Vec3::X)],
                Some(3.0),
            );
        }
        crate::audio::event_at(&mut commands, "incendiary_trap_explode", origin);
        crate::audio::event_at(&mut commands, "incendiary_trap_burn", origin);
        let group = next_group();
        for i in 0..12u32 {
            let yaw = (30.0 * i as f32).to_radians();
            let fwd = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
            let dir = if i > 5 { -fwd } else { fwd };
            commands.spawn(Wave {
                kind: WaveKind::Firewall,
                pos: trap.pos + fwd * FIRE_TRAP_MINI_EXPLOSION_RADIUS,
                head: trap.pos + fwd * FIRE_TRAP_MINI_EXPLOSION_RADIUS,
                first: true,
                dir,
                step: n.trap_step,
                left: n.trap_count,
                timer: i as f32 * 0.008,
                radius: FIRE_TRAP_MINI_EXPLOSION_RADIUS,
                titan: 0.0,
                pilot: 0.0,
                group: group + i,
                enemy: false,
                trap: Some(origin),
                row: false,
            });
        }
        log::info!(
            "slow trap ignited ({} lines of {} x {:.0})",
            12,
            n.trap_count,
            n.trap_step
        );
    }

    // --- Cluster Missile ---
    let old_fx = std::env::var_os("TF_OLD_FX").is_some();
    for (id, mut r, mut tf) in &mut rockets {
        r.age += dt;
        let step = r.vel * dt;
        let len = step.length().max(1e-4);
        let dir = step / len;
        let mut t_end = len;
        let mut normal = -dir;
        let mut hit = world
            .0
            .raycast(
                SVec3::from(r.pos.to_array()),
                SVec3::from(dir.to_array()),
                len,
            )
            .map(|h| {
                normal = Vec3::from(h.normal.to_array());
                h.t
            });
        if let Some(t) = hit {
            t_end = t;
        }
        for (_, e) in enemies.iter() {
            if e.alive() {
                if let Some(t) = ray_cylinder(r.pos, dir, e.pos, e.radius, e.height) {
                    if t < t_end {
                        t_end = t;
                        hit = Some(t);
                        normal = -dir;
                    }
                }
            }
        }
        r.pos += dir * t_end;
        tf.translation = to_bevy(r.pos);
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, to_bevy(dir).normalize_or(Vec3::Y));
        if hit.is_none() && r.age < 6.0 {
            continue;
        }
        commands.entity(id).despawn();
        let at = r.pos - dir * 10.0;
        splash(
            &mut enemies,
            at,
            n.cluster_inner,
            n.cluster_radius,
            n.cluster_heavy,
            n.cluster_pilot,
            &mut core,
        );
        let base = if old_fx {
            emit(
                &mut commands,
                Effect::Explosion {
                    at: to_bevy(at),
                    scale: 1.3,
                },
            );
            crate::audio::cue(&mut commands, crate::audio::Cue::MissileExplode, Some(at));
            None
        } else {
            emit(
                &mut commands,
                Effect::Hit {
                    at: to_bevy(r.pos),
                    normal: to_bevy(normal).normalize_or(Vec3::Y),
                    table: "exp_rocket_dumbfire",
                    surface: crate::particles::Surface::World,
                    victim: false,
                    scale: 1.0,
                },
            );
            // exp_rocket_dumbfire's Sound_attacker, and the base's sound.
            crate::audio::event_at(
                &mut commands,
                "Default.ClusterRocket_Primary_Explosion_1P_vs_3P",
                at,
            );
            crate::audio::event_at(&mut commands, "Explo_ThermiteGrenade_Impact_3P", at);
            // CLUSTER_BASE_FX on a mover facing along the surface normal.
            let rot = Quat::from_rotation_arc(Vec3::NEG_Z, to_bevy(normal).normalize_or(Vec3::Y));
            Some(
                commands
                    .spawn((
                        Transform::from_translation(to_bevy(r.pos)).with_rotation(rot),
                        crate::pfx::PfxTrail::oriented("P_wpn_meteor_exp"),
                    ))
                    .id(),
            )
        };
        commands.spawn(ClusterBursts {
            center: r.pos,
            normal,
            delay: CLUSTER_EXPLOSION_DELAY,
            life: CLUSTER_EXPLOSION_DELAY + n.cluster_duration,
            base,
            hops: Vec::new(),
            rng: (r.age.to_bits() as u64) | 1,
        });
    }
    for (id, mut b) in &mut bursts {
        let b = &mut *b;
        let mut rand = {
            let rng = &mut b.rng;
            move || {
                *rng ^= *rng << 13;
                *rng ^= *rng >> 7;
                *rng ^= *rng << 17;
                (*rng >> 40) as f32 / (1u64 << 24) as f32
            }
        };
        b.life -= dt;
        if b.life <= 0.0 {
            for h in &b.hops {
                commands.entity(h.ent).despawn();
            }
            if let Some(e) = b.base {
                commands.entity(e).despawn();
            }
            commands.entity(id).despawn();
            continue;
        }
        let burst_delay =
            n.cluster_duration / (n.cluster_bursts / CLUSTER_GROUP_SIZE).max(1) as f32;
        // The next pop: a random point in the dome over the hit, out to the burst range, cut
        // short by the world (ClusterRocketBurst).
        let next_pop = |from: Vec3, rand: &mut dyn FnMut() -> f32| -> (Vec3, Vec3, f32) {
            let v = loop {
                let v = Vec3::new(rand() * 2.0 - 1.0, rand() * 2.0 - 1.0, rand() * 2.0 - 1.0);
                if v.length_squared() > 1e-4 && v.length_squared() <= 1.0 {
                    let v = v.normalize();
                    break if v.dot(b.normal) < 0.0 { -v } else { v };
                }
            };
            let pop_vec = v * n.cluster_range * rand();
            let len = pop_vec.length();
            let frac = world
                .0
                .raycast(
                    SVec3::from(b.center.to_array()),
                    SVec3::from((pop_vec / len.max(1e-4)).to_array()),
                    len,
                )
                .map_or(1.0, |h| h.t / len.max(1e-4));
            let to = b.center + pop_vec * frac;
            let dur =
                (burst_delay - CLUSTER_BURST_DELAY - (rand() * 2.0 - 1.0) * CLUSTER_BURST_OFFSET)
                    .max(0.05);
            let vel = Vec3::new(
                (to.x - from.x) / dur,
                (to.y - from.y) / dur,
                (to.z + 0.5 * 750.0 * dur * dur - from.z) / dur,
            );
            (to, vel, dur)
        };
        if b.delay > 0.0 {
            b.delay -= dt;
            if b.delay <= 0.0 {
                // CLUSTER_ROCKET_BURST_GROUP_SIZE projectiles, a frame apart, from 8 units off
                // the surface; each explodes count / group + 1 times.
                let from = b.center + b.normal * 8.0;
                for k in 0..CLUSTER_GROUP_SIZE {
                    let (to, vel, dur) = next_pop(from, &mut rand);
                    let ent = commands
                        .spawn((
                            Transform::from_translation(to_bevy(from)),
                            Visibility::default(),
                            crate::pfx::PfxTrail::new("P_wpn_dumbfire_burst_trail"),
                            crate::pilotweapon::ProjectileModel(
                                "models/weapons/grenades/m20_f_grenade.mdl",
                            ),
                        ))
                        .id();
                    b.hops.push(ClusterHop {
                        ent,
                        from,
                        to,
                        vel,
                        t: -(k as f32) / 60.0,
                        dur,
                        left: n.cluster_bursts / CLUSTER_GROUP_SIZE,
                    });
                }
            }
            continue;
        }
        let mut pops = Vec::new();
        for h in b.hops.iter_mut() {
            if h.t < 0.0 && h.t + dt < 0.0 {
                h.t += dt;
                continue;
            }
            h.t += dt;
            if h.t < h.dur {
                let p = h.from + h.vel * h.t - Vec3::Z * 0.5 * 750.0 * h.t * h.t;
                commands
                    .entity(h.ent)
                    .insert(Transform::from_translation(to_bevy(p)));
                continue;
            }
            pops.push(h.to);
            if h.left == 0 {
                commands.entity(h.ent).despawn();
                h.dur = f32::INFINITY;
                continue;
            }
            h.left -= 1;
            h.from = h.to;
            h.t = 0.0;
        }
        b.hops.retain(|h| h.dur.is_finite() || h.left > 0);
        let hops: Vec<usize> = (0..b.hops.len()).filter(|&i| b.hops[i].t == 0.0).collect();
        for i in hops {
            let from = b.hops[i].from;
            let (to, vel, dur) = next_pop(from, &mut rand);
            let h = &mut b.hops[i];
            (h.to, h.vel, h.dur) = (to, vel, dur);
        }
        for at in pops {
            splash(
                &mut enemies,
                at,
                n.cluster_inner,
                n.cluster_radius,
                n.cluster_heavy,
                n.cluster_pilot,
                &mut core,
            );
            if old_fx {
                emit(
                    &mut commands,
                    Effect::Explosion {
                        at: to_bevy(at),
                        scale: 0.6,
                    },
                );
            } else {
                emit(
                    &mut commands,
                    Effect::Hit {
                        at: to_bevy(at),
                        normal: to_bevy(b.normal).normalize_or(Vec3::Y),
                        table: "exp_rocket_cluster_secondary",
                        surface: crate::particles::Surface::World,
                        victim: false,
                        scale: 1.0,
                    },
                );
            }
            crate::audio::event_at(
                &mut commands,
                "Default.ClusterRocket_Explosion_1P_vs_3P",
                at,
            );
        }
    }

    // --- Particle Wall ---
    let disc = |meshes: &mut Assets<Mesh>,
                materials: &mut Assets<StandardMaterial>,
                r: f32,
                color: Color,
                glow: LinearRgba| {
        (
            meshes.add(Circle::new(r * crate::player::UNIT)),
            materials.add(StandardMaterial {
                base_color: color,
                emissive: glow,
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
        )
    };
    for (ent, dmg) in std::mem::take(&mut enemy_walls.absorbed) {
        if let Ok((_, mut w, _, _)) = walls.get_mut(ent) {
            w.hp -= dmg;
            log::debug!("enemy particle wall took {dmg:.0}, {:.0} left", w.hp);
            if w.hp <= 0.0 {
                log::info!("enemy particle wall broken");
            }
        }
    }
    enemy_walls.walls.clear();
    for (id, mut w, mut tf, children) in &mut walls {
        w.life -= dt;
        if w.life <= 0.0 || w.hp <= 0.0 {
            log::debug!(
                "particle wall down ({})",
                if w.hp <= 0.0 { "broken" } else { "expired" }
            );
            commands.entity(id).despawn();
            continue;
        }
        if children.is_none_or(|c| c.is_empty()) {
            if std::env::var_os("TF_OLD_FX").is_none() {
                // SHIELD_WALL_FX at the wall's origin with its angles (control point 0 facing
                // the normal), looping until the wall goes.
                let fx = commands
                    .spawn((
                        Transform::IDENTITY,
                        Visibility::default(),
                        crate::pfx::PfxTrail::oriented("P_xo_shield_wall"),
                    ))
                    .id();
                commands.entity(id).add_child(fx);
            } else {
                // A wide, tall pane facing where BT looked.
                let (m, mat) = disc(
                    &mut meshes,
                    &mut materials,
                    1.0,
                    Color::srgba(0.3, 0.7, 1.0, 0.2),
                    LinearRgba::rgb(0.4, 1.2, 2.4),
                );
                let pane = commands
                    .spawn((
                        Mesh3d(m),
                        MeshMaterial3d(mat),
                        Transform::from_xyz(
                            0.0,
                            194.0 * crate::player::UNIT,
                            -SHIELD_WALL_RADIUS * crate::player::UNIT,
                        )
                        .with_scale(Vec3::new(156.0, 180.0, 1.0)),
                    ))
                    .id();
                commands.entity(id).add_child(pane);
            }
        }
        // Control point 0 takes the entity's rotation, forward (Bevy -Z) along the normal.
        *tf = Transform::from_translation(to_bevy(w.origin))
            .looking_to(to_bevy(w.normal).normalize(), Vec3::Y);
        // UpdateShieldWallColorForFrac( shieldWallFX, GetHealthFrac( vortexSphere ) ): control
        // point 1 carries GetShieldTriLerpColor( 1 - health fraction ), so the wall goes from
        // blue through orange to red as it takes damage.
        for &c in children.into_iter().flatten() {
            commands
                .entity(c)
                .insert(crate::pfx::PfxColor(crate::fxuber::shield_color(
                    1.0 - w.hp / 1750.0,
                )));
        }
        if w.enemy {
            enemy_walls
                .walls
                .push(BoltShield::wall(w.origin, w.normal, Some(id)));
        } else {
            shields
                .shields
                .push(BoltShield::wall(w.origin, w.normal, Some(id)));
        }
    }

    // --- Gun Shield: in front of BT's gun while it lasts ---
    let s = &titan.state;
    let eye = Vec3::from(s.eye(&settings.0).to_array());
    let view = view_dir(s.yaw, s.pitch);
    if st.gun_shield > 0.0 && st.gun_shield_hp > 0.0 {
        // TITAN_GUN_SHIELD_RADIUS around the Predator's muzzle, out in front of BT's right hand.
        let side = Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0);
        let center = eye + view * 220.0 + side * 45.0 - Vec3::Z * 45.0;
        shields
            .shields
            .push(BoltShield::disc(center, view, 105.0, None));
        // The game plays P_titan_gun_shield_FP on the owner's viewmodel and the 3P system on
        // everyone else's screen; swap when the camera changes.
        let fp = *mode == CameraMode::Cockpit;
        if let Some(e) = st.gun_vis {
            if st.gun_vis_fp != fp {
                commands.entity(e).despawn();
                st.gun_vis = None;
            }
        }
        let e = match st.gun_vis {
            Some(e) => e,
            None => {
                let (m, mat) = disc(
                    &mut meshes,
                    &mut materials,
                    105.0,
                    Color::srgba(1.0, 0.6, 0.2, 0.12),
                    LinearRgba::rgb(1.6, 0.8, 0.2),
                );
                let e = commands
                    .spawn((
                        Transform::from_translation(to_bevy(center + Vec3::Z * 80.0)),
                        Visibility::default(),
                    ))
                    .id();
                if std::env::var_os("TF_OLD_FX").is_some() {
                    let d = commands
                        .spawn((Mesh3d(m), MeshMaterial3d(mat), Transform::IDENTITY))
                        .id();
                    commands.entity(e).add_child(d);
                } else {
                    // The game's hex gun shield (P_titan_gun_shield_FP in the cockpit; the 3P
                    // system is the same models with a different trail).
                    commands
                        .entity(e)
                        .insert(crate::pfx::PfxTrail::oriented(if fp {
                            "P_titan_gun_shield_FP"
                        } else {
                            "P_titan_gun_shield_3P"
                        }));
                    if fp {
                        commands.entity(e).insert(crate::pfx::PfxVmSpace);
                    }
                }
                st.gun_vis = Some(e);
                st.gun_vis_fp = fp;
                e
            }
        };
        // The 3P hex models hang 80 below their control point (see defense_fx_spot in
        // targets.rs); the FP system is built around the muzzle itself.
        // In the cockpit it stands at the viewmodel's gun_shield_fp (FX_TITAN_GUN_SHIELD_VM).
        let old_fx = std::env::var_os("TF_OLD_FX").is_some();
        let fx_center = if old_fx || fp {
            to_bevy(center)
        } else {
            to_bevy(center + Vec3::Z * 80.0)
        };
        let fx_center = gun_muzzle
            .shield_fp
            .filter(|_| fp && !old_fx && gun_muzzle.cockpit)
            .unwrap_or(fx_center);
        // mp_titanability_gun_shield.nut's UpdateShieldColor: control point 1 carries
        // GetShieldTriLerpColor( 1 - health / TITAN_GUN_SHIELD_HEALTH ).
        let color =
            crate::pfx::PfxColor(crate::fxuber::shield_color(1.0 - st.gun_shield_hp / 2500.0));
        commands.entity(e).insert((
            Transform::from_translation(fx_center).looking_to(to_bevy(view).normalize(), Vec3::Y),
            color,
        ));
    } else if let Some(e) = st.gun_vis.take() {
        commands.entity(e).despawn();
    }

    // --- Tether grenades: fly, bounce off walls, plant on floors ---
    for (id, mut g, mut tf) in &mut tethers {
        g.life -= dt;
        g.vel.z -= DEPLOYABLE_GRAVITY * dt;
        let step = g.vel * dt;
        let len = step.length();
        if len > 1e-4 {
            let dir = step / len;
            let world_hit = world.0.raycast(
                SVec3::from(g.pos.to_array()),
                SVec3::from(dir.to_array()),
                len + 4.0,
            );
            // A tether that strikes an enemy Titan tethers it on the spot: the anchor goes on
            // the ground up to 256 below the hit (OnProjectileCollision_weapon_tether).
            let limit = world_hit.as_ref().map_or(len + 4.0, |h| h.t);
            let direct = enemies
                .iter()
                .filter(|(_, e)| e.alive() && !e.infantry)
                .filter_map(|(eid, e)| {
                    ray_cylinder(g.pos, dir, e.pos, e.radius + 4.0, e.height)
                        .filter(|t| *t < limit)
                        .map(|t| (eid, e.pos, t))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((eid, epos, t)) = direct {
                let hit = g.pos + dir * t;
                let nrm = (hit - epos).with_z(0.0).normalize_or(Vec3::X);
                let probe = Vec3::new(nrm.x * 100.0, nrm.y * 100.0, -256.0);
                commands.entity(id).despawn();
                if let Some(h) = world.0.raycast(
                    SVec3::from(hit.to_array()),
                    SVec3::from(probe.normalize().to_array()),
                    probe.length(),
                ) {
                    let ground = Vec3::from(h.normal.to_array());
                    let pos = hit + probe.normalize() * h.t + ground * 2.0;
                    crate::audio::event_at(&mut commands, "Wpn_TetherTrap_PopOpen_3p", pos);
                    log::info!("tether trap struck an enemy Titan");
                    let e = spawn_tether(
                        &mut commands,
                        tether_assets.as_deref(),
                        pos,
                        TetherTrap {
                            pos,
                            arm: 0.0,
                            victim: Some((eid, epos, n.tether_hold)),
                            life: TETHER_LIFETIME,
                        },
                    );
                    if tether_assets.is_some() {
                        commands.entity(e).insert(
                            Transform::from_translation(pos)
                                .with_rotation(Quat::from_rotation_arc(Vec3::Z, ground)),
                        );
                    }
                }
                continue;
            }
            match world_hit {
                Some(h) => {
                    let nrm = Vec3::from(h.normal.to_array());
                    let at = g.pos + dir * (h.t - 4.0).max(0.0);
                    if nrm.z >= TETHER_PLANT_DOT {
                        // PlantStickyEntityOnWorldThatBouncesOffWalls: a floor plants it.
                        let pos = at + nrm * 2.0;
                        crate::audio::event_at(&mut commands, "Wpn_TetherTrap_Land", pos);
                        log::info!(
                            "tether trap landed {:.0} out",
                            (pos - Vec3::from(titan.state.pos.to_array())).length()
                        );
                        commands.entity(id).despawn();
                        let e = spawn_tether(
                            &mut commands,
                            tether_assets.as_deref(),
                            pos,
                            TetherTrap {
                                pos,
                                arm: TETHER_ARM,
                                victim: None,
                                life: TETHER_LIFETIME,
                            },
                        );
                        if tether_assets.is_some() {
                            // Planted flat on the surface (SetAngles(surfaceNormal)).
                            commands.entity(e).insert(
                                Transform::from_translation(pos)
                                    .with_rotation(Quat::from_rotation_arc(Vec3::Z, nrm)),
                            );
                        }
                        // projectile_max_deployed: the oldest trap goes when a fifth lands.
                        let mut planted: Vec<(Entity, f32)> =
                            traps.iter().map(|(e, t)| (e, t.life)).collect();
                        if planted.len() >= TETHERS_MAX {
                            planted.sort_by(|a, b| a.1.total_cmp(&b.1));
                            for (e, _) in planted.iter().take(planted.len() + 1 - TETHERS_MAX) {
                                commands.entity(*e).despawn();
                            }
                        }
                        continue;
                    }
                    let into = -dir.dot(nrm);
                    let keep = TETHER_BOUNCE_SHALLOW
                        + (TETHER_BOUNCE_SHARP - TETHER_BOUNCE_SHALLOW) * into.clamp(0.0, 1.0);
                    g.vel = (g.vel - 2.0 * g.vel.dot(nrm) * nrm) * keep;
                    g.pos = at + nrm * 0.5;
                }
                None => g.pos += step,
            }
        }
        // With the model the entity lives under the world root (game units); the glow doesn't.
        tf.translation = if tether_assets.is_some() {
            g.pos
        } else {
            to_bevy(g.pos)
        };
        if tether_assets.is_some() {
            tf.rotation *=
                Quat::from_euler(EulerRot::XYZ, g.spin.x * dt, g.spin.y * dt, g.spin.z * dt);
        }
        if g.life <= 0.0 {
            commands.entity(id).despawn();
        }
    }

    // --- Tether Trap: holds the first enemy Titan it sees within 450 units ---
    for (id, mut t) in &mut traps {
        t.life -= dt;
        t.arm -= dt;
        match t.victim {
            None if t.arm <= 0.0 => {
                let sees = |e: &Enemy| {
                    let eye = e.pos + Vec3::Z * (e.height - 40.0);
                    let from = t.pos + Vec3::Z;
                    let d = eye - from;
                    world
                        .0
                        .raycast(
                            SVec3::from(from.to_array()),
                            SVec3::from(d.normalize_or_zero().to_array()),
                            d.length() - e.radius * 0.5,
                        )
                        .is_none()
                };
                let catch = enemies
                    .iter()
                    .filter(|(_, e)| e.alive() && !e.infantry)
                    .find(|(_, e)| e.pos.distance(t.pos) < TETHER_CATCH_RADIUS_NPC && sees(e))
                    .map(|(id, e)| (id, e.pos));
                if let Some((eid, p)) = catch {
                    t.victim = Some((eid, p, n.tether_hold));
                    crate::audio::event_at(&mut commands, "Wpn_TetherTrap_PopOpen_3p", t.pos);
                    log::info!("tether trap caught an enemy Titan");
                }
            }
            Some((eid, anchor, left)) => {
                let left = left - dt;
                match enemies.get_mut(eid) {
                    Ok((_, mut e)) if left > 0.0 && e.alive() => {
                        // Pinned: undo whatever the AI moved this frame.
                        e.pos = anchor;
                        e.state.pos = SVec3::from(anchor.to_array());
                        e.state.vel = SVec3::ZERO;
                        t.victim = Some((eid, anchor, left));
                        emit(
                            &mut commands,
                            Effect::Tracer {
                                from: to_bevy(t.pos),
                                to: to_bevy(anchor + Vec3::Z * 120.0),
                                width: 0.08,
                                color: Vec3::new(1.5, 3.0, 6.0),
                            },
                        );
                    }
                    _ => {
                        commands.entity(id).despawn();
                        continue;
                    }
                }
            }
            None => {}
        }
        if t.life <= 0.0 {
            commands.entity(id).despawn();
        }
    }
}

// --- HUD hookup -----------------------------------------------------------------------------

/// The Q/E slot shades and numbers for the current kit, and the icons and labels on change.
#[allow(clippy::too_many_arguments)]
pub fn kit_view(
    defs: Option<Res<KitDefs>>,
    active: Res<ActiveKit>,
    ui: Res<crate::ui::UiAssets>,
    mut view: ResMut<KitView>,
    titans: Query<(&KitState, &Ordnance, &Vortex)>,
    mut icons: Query<(&crate::hud::SlotIcon, &mut ImageNode), Without<crate::hud::CoreIcon>>,
    mut labels: Query<(
        Option<&crate::hud::SlotLabel>,
        Option<&crate::hud::CoreLabel>,
        &mut Text,
    )>,
    mut shown: Local<Option<usize>>,
) {
    let Some(defs) = defs else { return };
    let Ok((st, ord, vortex)) = titans.single() else {
        return;
    };
    let n = &defs.n;
    let spec = &defs.kits[active.index.min(defs.kits.len() - 1)];
    let frac = |cd: f32, max: f32| {
        if cd > 0.0 {
            (cd / max.max(0.01)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    (view.ord_left, view.ord_text) = match active.ord {
        OrdKind::Missiles if ord.charging => (0.0, format!("{}", ord.locks.len())),
        OrdKind::Missiles if !ord.ready() => (
            1.0 - ord.fraction(),
            format!("{:.0}", (ord.cooldown + ord.delay.max(0.0)).ceil()),
        ),
        OrdKind::Missiles => (0.0, String::new()),
        OrdKind::LaserShot => {
            let need = (n.laser_cost - st.energy).max(0.0);
            (
                need / n.laser_cost,
                if need > 0.0 {
                    format!("{:.0}%", st.energy / n.energy_max * 100.0)
                } else {
                    String::new()
                },
            )
        }
        OrdKind::Tracker => {
            let best = st.locks.iter().map(|l| l.1.len()).max().unwrap_or(0);
            (
                if best >= 3 {
                    frac(st.ord_cd, st.ord_cd_max)
                } else {
                    1.0 - best as f32 / 3.0
                },
                format!("LOCK {best}/3"),
            )
        }
        _ => (
            frac(st.ord_cd, st.ord_cd_max),
            if st.ord_cd > 0.0 {
                format!("{:.0}", st.ord_cd.ceil())
            } else {
                String::new()
            },
        ),
    };
    (view.util_left, view.util_text) = {
        let un = n
            .util
            .get(
                UTILITY
                    .iter()
                    .find(|u| u.1 == active.util)
                    .map(|u| u.0)
                    .unwrap_or(""),
            )
            .copied()
            .unwrap_or(UtilNumbers {
                cooldown: 20.0,
                charges: 1,
                duration: 0.0,
            });
        let left = if st.util_charges == 0 {
            frac(st.util_cd, st.util_cd_max)
        } else {
            0.0
        };
        let text = if st.phase > 0.0 {
            "PHASED".into()
        } else if st.hover_len > 0.0 {
            format!("{:.1}", (st.hover_len - st.hover_t).max(0.0))
        } else if st.util_charges == 0 {
            format!("{:.0}", st.util_cd.ceil())
        } else if un.charges > 1 {
            format!("x{}", st.util_charges)
        } else {
            String::new()
        };
        (left, text)
    };
    (view.def_left, view.def_text) = match active.def {
        DefKind::Vortex => (
            1.0 - vortex.fraction(),
            if vortex.active {
                format!("{}", vortex.caught + vortex.absorbed)
            } else {
                String::new()
            },
        ),
        // The Thermal Shield keeps nothing to count.
        DefKind::HeatShield => (1.0 - vortex.fraction(), String::new()),
        DefKind::IonVortex => (
            1.0 - st.energy / n.energy_max,
            if vortex.active {
                format!("{}", vortex.caught + vortex.absorbed)
            } else {
                String::new()
            },
        ),
        DefKind::SwordBlock => (
            0.0,
            if st.blocking {
                "BLOCK".into()
            } else {
                String::new()
            },
        ),
        DefKind::GunShield if st.gun_shield > 0.0 => {
            (0.0, format!("{:.0}", st.gun_shield_hp.max(0.0)))
        }
        _ => (
            frac(st.def_cd, st.def_cd_max),
            if st.def_cd > 0.0 {
                format!("{:.0}", st.def_cd.ceil())
            } else {
                String::new()
            },
        ),
    };
    if *shown == Some(active.index) && icons.iter().count() > 0 {
        return;
    }
    *shown = Some(active.index);
    for (slot, mut img) in &mut icons {
        let path = match slot.0 {
            crate::hud::Slot::Ordnance => &spec.ord_icon,
            crate::hud::Slot::Defensive => &spec.def_icon,
            crate::hud::Slot::Utility => &spec.util_icon,
        };
        match ui.image(path) {
            Some(i) => *img = i,
            None => log::warn!("HUD icon {path} is not in the UI atlas"),
        }
    }
    if view.core_icon != spec.core_icon {
        view.core_icon = spec.core_icon.clone();
    }
    for (slot, core, mut t) in &mut labels {
        if let Some(slot) = slot {
            t.0 = match slot.0 {
                crate::hud::Slot::Ordnance => spec.ord_label.clone(),
                crate::hud::Slot::Defensive => spec.def_label.clone(),
                crate::hud::Slot::Utility => spec.util_label.clone(),
            };
        } else if core.is_some() {
            t.0 = spec.core_label.clone();
        }
    }
}

/// Tone's Sonar Pulse in flight (the kunai model with wpn_grenade_sonar_titan).
#[derive(Component)]
pub struct SonarShot {
    pos: Vec3,
    vel: Vec3,
    age: f32,
}

/// Fly Sonar Pulse echoes; where one hits the world or a Titan it pulses
/// (OnProjectileCollision_titanability_sonar_pulse -> PulseLocation): enemies within
/// SONAR_PULSE_RADIUS are revealed for SONAR_PULSE_DURATION and each Titan there gets a tracker
/// mark, with the exp_sonar_pulse impact and Titan_Tone_SonarLock_Impact_Pulse_1P.
#[allow(clippy::type_complexity)]
pub fn sonar_shots(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    active: Res<ActiveKit>,
    mut status: ResMut<crate::pilotability::PilotStatus>,
    mut shots: Query<(Entity, &mut SonarShot, &mut Transform)>,
    enemies: Query<(Entity, &Enemy)>,
    mut kits: Query<&mut KitState, With<PlayerTitan>>,
) {
    let dt = time.delta_secs().min(0.1);
    for (id, mut sh, mut tf) in &mut shots {
        sh.age += dt;
        sh.vel.z -= SONAR_SHOT_GRAVITY * dt;
        let step = sh.vel * dt;
        let len = step.length().max(1e-4);
        let dir = step / len;
        let mut t_end = len;
        let mut normal = -dir;
        let mut hit = world
            .0
            .raycast(
                SVec3::from(sh.pos.to_array()),
                SVec3::from(dir.to_array()),
                len,
            )
            .map(|h| {
                normal = Vec3::from(h.normal.to_array());
                h.t
            });
        if let Some(t) = hit {
            t_end = t;
        }
        for (_, e) in enemies.iter() {
            if e.alive() && !e.infantry {
                if let Some(t) = ray_cylinder(sh.pos, dir, e.pos, e.radius, e.height) {
                    if t < t_end {
                        t_end = t;
                        hit = Some(t);
                        normal = -dir;
                    }
                }
            }
        }
        sh.pos += dir * t_end;
        tf.translation = to_bevy(sh.pos);
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, to_bevy(dir).normalize_or(Vec3::Y));
        if hit.is_none() && sh.age < 6.0 {
            continue;
        }
        commands.entity(id).despawn();
        let at = sh.pos;
        log::debug!("sonar pulse at ({:.0}, {:.0}, {:.0})", at.x, at.y, at.z);
        status.pulse = Some((at, SONAR_PULSE_DURATION));
        emit(
            &mut commands,
            Effect::Hit {
                at: to_bevy(at),
                normal: to_bevy(normal).normalize_or(Vec3::Y),
                table: "exp_sonar_pulse",
                surface: crate::particles::Surface::World,
                victim: false,
                scale: 1.0,
            },
        );
        crate::audio::event_at(&mut commands, "Titan_Tone_SonarLock_Impact_Pulse_1P", at);
        if active.ord == OrdKind::Tracker {
            if let Ok(mut kit) = kits.single_mut() {
                for (e_id, e) in enemies.iter() {
                    if e.alive() && !e.infantry && e.pos.distance(at) < SONAR_PULSE_RADIUS {
                        apply_tracker_mark(&mut commands, &mut kit.locks, e_id);
                    }
                }
            }
        }
    }
}

/// Spawn the tower model under each newly planted Laser Tripwire pylon and play its opening
/// ("trip_wire_closed_to_open", held on its last frame).
#[allow(clippy::too_many_arguments)]
pub fn laser_towers(
    mut commands: Commands,
    time: Res<Time>,
    gd: Res<GameData>,
    mut cache: ResMut<crate::convert::Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    mut towers: Query<(Entity, &mut LaserTower)>,
    mut actors: Query<&mut crate::actor::Actor>,
) {
    for (id, mut t) in &mut towers {
        t.age += time.delta_secs();
        let Some(a) = t.actor else {
            let spec = crate::actor::ActorSpec {
                path: LASER_TRIP_MODEL,
                sequences: &["trip_wire_closed_to_open"],
                grids: &[],
                body: &[],
            };
            match crate::actor::spawn_actor(
                &mut commands,
                id,
                &gd,
                &spec,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut bindposes,
            ) {
                Ok(sp) => t.actor = Some(sp.entity),
                Err(e) => {
                    log::warn!("laser tripwire tower: {e:#}");
                    commands.entity(id).remove::<LaserTower>();
                }
            }
            continue;
        };
        let Ok(mut actor) = actors.get_mut(a) else {
            continue;
        };
        actor.autoplay = false;
        if let Some(clip) = actor.clip("trip_wire_closed_to_open") {
            let cycle = (t.age / actor.clips[clip].duration.max(1e-3)).min(0.999);
            actor.layers = vec![crate::actor::Layer {
                clip,
                cycle,
                weight: 1.0,
            }];
        }
    }
}

/// Utility deployables: Electric Smoke clouds, Slow Trap gas and Laser Tripwire pylons.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn utility_world(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    mut smokes: Query<(Entity, &mut TitanSmoke)>,
    mut traps: Query<(Entity, &mut SlowTrap, &mut Transform), Without<LaserPylon>>,
    mut pylons: Query<(Entity, &mut LaserPylon, &mut Transform), Without<SlowTrap>>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    (control, difficulty, mut pilots, mut player): (
        Res<crate::pilotctl::Control>,
        Res<crate::vitals::Difficulty>,
        Query<(
            &crate::pilotctl::PlayerPilot,
            &mut crate::pilotctl::PilotHealth,
        )>,
        Query<(&PlayerTitan, &mut TitanHealth)>,
    ),
) {
    let dt = time.delta_secs().min(0.1);
    // Electric Smoke: after damageDelay, dpsPilot / dpsTitan per second between the radii.
    for (id, mut sm) in &mut smokes {
        sm.life -= dt;
        sm.delay -= dt;
        sm.puff += dt;
        if sm.puff >= 0.08 && std::env::var_os("TF_OLD_FX").is_some() {
            sm.puff = 0.0;
            let a = sm.life * 9.1;
            let r = TITAN_SMOKE_OUTER * 0.7 * (a * 0.41).sin().abs();
            let p = sm.pos
                + Vec3::new(
                    a.cos() * r,
                    a.sin() * r,
                    40.0 + (a * 0.7).sin().abs() * 120.0,
                );
            emit(
                &mut commands,
                Effect::TrailPuff {
                    at: to_bevy(p),
                    scale: 7.0,
                },
            );
            if sm.puff == 0.0 && (sm.life * 5.0) as u32 % 2 == 0 {
                emit(
                    &mut commands,
                    Effect::Impact {
                        at: to_bevy(p),
                        normal: Vec3::Y,
                        scale: 1.2,
                        energy: true,
                    },
                );
            }
        }
        if sm.delay <= 0.0 {
            sm.tick -= dt;
            while sm.tick <= 0.0 {
                sm.tick += SERVER_FRAME;
                if sm.enemy {
                    // An enemy's smoke: the Pilot on foot (or on the Titan's back) and BT.
                    let centre = sm.pos + Vec3::Z * 60.0;
                    let falloff = |d: f32| {
                        1.0 - ((d - TITAN_SMOKE_INNER) / (TITAN_SMOKE_OUTER - TITAN_SMOKE_INNER))
                            .clamp(0.0, 1.0)
                    };
                    if *control == Control::Pilot {
                        if let Ok((pilot, mut health)) = pilots.single_mut() {
                            let d = (Vec3::from(pilot.state.pos.to_array()) + Vec3::Z * 36.0)
                                .distance(centre);
                            if d < TITAN_SMOKE_OUTER {
                                let dmg = TITAN_SMOKE_DPS_PILOT
                                    * SERVER_FRAME
                                    * falloff(d)
                                    * difficulty.damage_to_player();
                                if !sm.hurt_pilot {
                                    sm.hurt_pilot = true;
                                    log::info!("electric smoke: the Pilot is in an enemy's cloud ({dmg:.1} per tick)");
                                }
                                health.damage(dmg);
                            }
                        }
                    }
                    if let Ok((titan, mut health)) = player.single_mut() {
                        let d = (Vec3::from(titan.state.pos.to_array()) + Vec3::Z * 120.0)
                            .distance(centre)
                            - 60.0;
                        if d < TITAN_SMOKE_OUTER && health.dead_for.is_none() {
                            health.damage(
                                TITAN_SMOKE_DPS_TITAN
                                    * SERVER_FRAME
                                    * falloff(d)
                                    * difficulty.damage_to_player(),
                                true,
                            );
                        }
                    }
                    continue;
                }
                for (_, mut e) in enemies.iter_mut() {
                    if !e.alive() {
                        continue;
                    }
                    let d = (e.pos + Vec3::Z * e.height * 0.5).distance(sm.pos + Vec3::Z * 60.0)
                        - e.radius;
                    if d < TITAN_SMOKE_OUTER {
                        let k = 1.0
                            - ((d - TITAN_SMOKE_INNER) / (TITAN_SMOKE_OUTER - TITAN_SMOKE_INNER))
                                .clamp(0.0, 1.0);
                        let dps = if e.infantry {
                            TITAN_SMOKE_DPS_PILOT
                        } else {
                            TITAN_SMOKE_DPS_TITAN
                        };
                        e.damage(dps * SERVER_FRAME * k, false);
                    }
                }
            }
        }
        if sm.life <= 0.0 {
            commands.entity(id).despawn();
        }
    }
    // Slow Trap: lands, builds for a second, then its gas slows whoever stands in it.
    for (id, mut t, mut tf) in &mut traps {
        if !t.landed {
            let (mut p, mut v) = (t.pos, t.vel);
            t.landed = fly_deployable(&world, &mut p, &mut v, dt);
            t.pos = p;
            t.vel = v;
            if t.landed {
                crate::audio::event_at(&mut commands, "incendiary_trap_land", t.pos);
            }
        } else if t.build > 0.0 {
            t.build -= dt;
            if t.build <= 0.0 {
                crate::audio::event_at(&mut commands, "incendiary_trap_gas", t.pos);
                // The script's looping P_meteor_Trap_start and its gas, P_meteor_trap_gas, on
                // the canister (its "smoke" attachment, taken as the canister's origin).
                if std::env::var_os("TF_OLD_FX").is_none() {
                    commands.entity(id).with_children(|c| {
                        c.spawn((
                            Transform::default(),
                            crate::pfx::PfxTrail::oriented("P_meteor_Trap_start"),
                        ));
                        c.spawn((
                            Transform::default(),
                            crate::pfx::PfxTrail::oriented("P_meteor_trap_gas"),
                        ));
                    });
                }
            }
        } else {
            t.life -= dt;
            t.puff += dt;
            if t.puff >= 0.1 && std::env::var_os("TF_OLD_FX").is_some() {
                t.puff = 0.0;
                let a = t.life * 6.3;
                let r = SLOW_TRAP_RADIUS * 0.8 * (a * 0.53).sin().abs();
                emit(
                    &mut commands,
                    Effect::TrailPuff {
                        at: to_bevy(t.pos + Vec3::new(a.cos() * r, a.sin() * r, 45.0)),
                        scale: 5.0,
                    },
                );
            }
            for (_, mut e) in enemies.iter_mut() {
                if e.alive()
                    && (e.pos - t.pos).truncate().length() - e.radius < SLOW_TRAP_RADIUS
                    && (e.pos.z - t.pos.z).abs() < 200.0
                {
                    e.slow = SLOW_TRAP_SLOW;
                    e.slow_t = 0.3;
                }
            }
            if t.life <= 0.0 {
                commands.entity(id).despawn();
            }
        }
        tf.translation = to_bevy(t.pos);
    }
    // Laser Tripwire (mp_titanability_laser_trip.nut): each deployable plants a tower (the
    // trip wire model, opening) with a hidden pylon rising out of it. Once built, a pylon lights
    // up (P_wpn_lasertrip_base) and beams (P_wpn_lasertrip_beam) to the group's next pylon
    // (projectileID + 1) if that one has planted and the line between them is clear; so the beams
    // run left to middle and middle to right. An enemy touching a beam destroys a pylon, and a
    // destroyed pylon takes its whole group with it (OnThreadEnd destroys the projectile group);
    // so does running out of time. Every pylon destroyed blows up: P_impact_exp_XLG_metal and 200
    // (1500 to heavy armour) within 400.
    let old_fx = std::env::var_os("TF_OLD_FX").is_some();
    let planted: Vec<(Entity, u32, u8, Vec3)> = pylons
        .iter()
        .filter(|(_, p, _)| p.landed)
        .map(|(id, p, _)| (id, p.group, p.id, p.pos))
        .collect();
    let mut blow: Vec<u32> = Vec::new();
    for (id, mut p, mut tf) in &mut pylons {
        if !p.landed {
            let (mut pos, mut v) = (p.pos, p.vel);
            p.landed = fly_deployable(&world, &mut pos, &mut v, dt);
            p.pos = pos;
            p.vel = v;
            if p.landed {
                log::debug!(
                    "laser pylon {} landed at ({:.0}, {:.0}, {:.0})",
                    p.id,
                    p.pos.x,
                    p.pos.y,
                    p.pos.z
                );
                commands.entity(id).remove::<Glow>();
                // The tower: upright, turned to the throw's heading, in game units.
                let root =
                    Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::splat(crate::player::UNIT));
                commands.entity(id).with_child((
                    root.mul_transform(Transform::from_rotation(Quat::from_rotation_z(p.yaw))),
                    Visibility::default(),
                    LaserTower {
                        age: 0.0,
                        actor: None,
                    },
                ));
            }
        } else if p.build > 0.0 {
            p.build -= dt;
            if p.build <= 0.0 {
                let head = p.pos + Vec3::Z * LASER_TRIP_RISE;
                let local = to_bevy(head) - to_bevy(p.pos);
                if !old_fx {
                    // PlayLoopFXOnEntity( LASER_TRIP_FX_ALL, pylon ).
                    commands.entity(id).with_child((
                        Transform::from_translation(local)
                            .with_rotation(Quat::from_rotation_y(p.yaw)),
                        crate::pfx::PfxTrail::oriented("P_wpn_lasertrip_base"),
                    ));
                }
                let next = planted.iter().find(|n| n.1 == p.group && n.2 == p.id + 1);
                if let Some(&(other, _, _, other_pos)) = next {
                    let end = other_pos + Vec3::Z * LASER_TRIP_RISE;
                    let to = end - head;
                    let blocked = world
                        .0
                        .raycast(
                            SVec3::from(head.to_array()),
                            SVec3::from(to.normalize_or_zero().to_array()),
                            to.length() * 0.99,
                        )
                        .is_some();
                    if !blocked {
                        p.link = Some((other, end));
                        if !old_fx {
                            commands.entity(id).with_child((
                                Transform::from_translation(local),
                                crate::pfx::PfxTrail::new("P_wpn_lasertrip_beam"),
                                crate::pfx::PfxBeamEnd(to_bevy(end)),
                            ));
                        }
                        log::debug!("laser pylon {} beams to pylon {}", p.id, p.id + 1);
                    }
                }
            }
        } else {
            p.life -= dt;
        }
        if p.life <= 0.0 && !blow.contains(&p.group) {
            blow.push(p.group);
        }
        tf.translation = to_bevy(p.pos);
    }
    // Keep at most LASER_TRIP_MAX pylons: the oldest group goes first.
    if pylons.iter().count() > LASER_TRIP_MAX {
        let oldest = pylons.iter().map(|(_, p, _)| p.group).min().unwrap_or(0);
        if !blow.contains(&oldest) {
            blow.push(oldest);
        }
    }
    for (_, p, _) in pylons.iter() {
        let Some((_, b)) = p.link else { continue };
        let a = p.pos + Vec3::Z * LASER_TRIP_RISE;
        if old_fx {
            emit(
                &mut commands,
                Effect::Tracer {
                    from: to_bevy(a),
                    to: to_bevy(b),
                    width: 0.08,
                    color: Vec3::new(1.0, 2.5, 6.0),
                },
            );
        }
        let seg = b - a;
        let len = seg.length().max(1e-3);
        let sd = seg / len;
        for (_, e) in enemies.iter() {
            if !e.alive() {
                continue;
            }
            // The enemy's bounds (a cylinder here) against the beam's box.
            let c = e.pos + Vec3::Z * e.height * 0.5;
            let t = (c - a).dot(sd).clamp(0.0, len);
            let q = a + sd * t;
            let horiz = (c - q).truncate().length();
            if horiz < e.radius + LASER_TRIP_BEAM_HALF
                && q.z > e.pos.z - LASER_TRIP_BEAM_HALF
                && q.z < e.pos.z + e.height + LASER_TRIP_BEAM_HALF
            {
                log::info!(
                    "laser tripwire tripped by {}",
                    if e.infantry { "a grunt" } else { "a titan" }
                );
                if !blow.contains(&p.group) {
                    blow.push(p.group);
                }
                break;
            }
        }
    }
    let blasts: Vec<(Entity, Vec3)> = pylons
        .iter()
        .filter(|(_, p, _)| blow.contains(&p.group))
        .map(|(id, p, _)| (id, p.pos))
        .collect();
    for (id, at) in blasts {
        commands.entity(id).despawn();
        if old_fx {
            emit(
                &mut commands,
                Effect::Explosion {
                    at: to_bevy(at),
                    scale: 1.0,
                },
            );
        } else {
            crate::pfx::emit_named_cp(
                &mut commands,
                "P_impact_exp_XLG_metal",
                Cp::facing(at, Vec3::Z),
            );
        }
        crate::audio::event_at(&mut commands, "Wpn_LaserTripMine_MineDestroyed", at);
        for (_, mut e) in enemies.iter_mut() {
            if e.alive()
                && (e.pos + Vec3::Z * e.height * 0.5).distance(at) - e.radius < LASER_TRIP_RADIUS
            {
                let dmg = if e.infantry {
                    LASER_TRIP_DAMAGE
                } else {
                    LASER_TRIP_DAMAGE_HEAVY
                };
                e.damage(dmg, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_wall_arc_stops_shots_from_the_front_only() {
        // A wall around the origin facing +X: 180 out, 120 degrees wide, 360 tall.
        let w = BoltShield::wall(Vec3::ZERO, Vec3::X, None);
        // Straight at the Titan from ahead: stops on the arc, 180 out.
        let t = w
            .hit_t(Vec3::new(1000.0, 0.0, 100.0), -Vec3::X, 2000.0)
            .unwrap();
        assert!((t - 820.0).abs() < 0.5, "{t}");
        // 50 degrees off the facing is still on the arc; 70 is past its edge.
        let at = |deg: f32| {
            Vec3::new(deg.to_radians().cos(), deg.to_radians().sin(), 0.0) * 1000.0
                + Vec3::Z * 100.0
        };
        assert!(w
            .hit_t(at(50.0), -at(50.0).with_z(0.0).normalize(), 2000.0)
            .is_some());
        assert!(w
            .hit_t(at(70.0), -at(70.0).with_z(0.0).normalize(), 2000.0)
            .is_none());
        // From behind, over the top, or out from inside: no hit.
        assert!(w
            .hit_t(Vec3::new(-1000.0, 0.0, 100.0), Vec3::X, 2000.0)
            .is_none());
        assert!(w
            .hit_t(Vec3::new(1000.0, 0.0, 400.0), -Vec3::X, 2000.0)
            .is_none());
        assert!(w
            .hit_t(Vec3::new(0.0, 0.0, 100.0), Vec3::X, 2000.0)
            .is_none());
    }
}
