//! Two-way combat: BT's segmented health and shield, dummy Titans firing back, the Vortex
//! Shield (catch rounds, fling them back) and the Titan punch.
//! Values from titan_buddy.set, mp_titanweapon_vortex_shield.txt and melee_titan_punch.txt.

use crate::audio::{self, Cue};
use crate::player::{to_bevy, CameraMode, Collision, PlayerInput, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use crate::vitals::{Hit, Vitals};
use crate::weapons::{FxAssets, VortexRefire};
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

/// The player's Titan as the HUD draws it, from its chassis settings (set by `setup_world`):
/// the health segment size (healthPerSegment: BT's 1800, 2500 in multiplayer), the number of
/// segments (health / segment: BT 5, light 3, medium 4, heavy 5) and the dash power drain
/// (dodgePowerDrain: the dash pips are 100 / drain).
static SEG_SIZE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1800);
static SEG_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(5);
static DASH_DRAIN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(50);

pub fn set_titan_layout(segment: f32, health: f32, dash_drain: f32) {
    use std::sync::atomic::Ordering::Relaxed;
    let segment = segment.max(1.0);
    SEG_SIZE.store(segment as u32, Relaxed);
    SEG_COUNT.store(((health / segment).ceil() as u32).clamp(1, 8), Relaxed);
    DASH_DRAIN.store(dash_drain.clamp(1.0, 100.0) as u32, Relaxed);
}
/// The on-foot meter's auto-Titan icons (hud_follow_icon, hud_guard_icon of the chassis).
static PET_ICONS: std::sync::RwLock<Option<(String, String)>> = std::sync::RwLock::new(None);

pub fn set_pet_icons(follow: &str, guard: &str) {
    if let Ok(mut i) = PET_ICONS.write() {
        *i = Some((follow.replace('\\', "/"), guard.replace('\\', "/")));
    }
}
/// The auto-Titan's icon for its mode.
pub fn pet_icon(guard: bool) -> String {
    let icons = PET_ICONS.read().ok().and_then(|i| i.clone());
    match (icons, guard) {
        (Some((f, _)), false) => f,
        (Some((_, g)), true) => g,
        (None, false) => "rui/hud/earn_meter/titan_follow".to_string(),
        (None, true) => "rui/hud/earn_meter/titan_guard".to_string(),
    }
}
/// Health per HUD segment.
pub fn segment() -> f32 {
    SEG_SIZE.load(std::sync::atomic::Ordering::Relaxed) as f32
}
/// HUD health segments.
pub fn segments() -> u32 {
    SEG_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}
/// Dash power one dash takes.
pub fn dash_drain() -> f32 {
    DASH_DRAIN.load(std::sync::atomic::Ordering::Relaxed) as f32
}
/// Dash pips on the HUD.
pub fn dash_pips() -> usize {
    ((100.0 / dash_drain()).floor() as usize).clamp(1, 4)
}

// Vortex Shield.
const VORTEX_HOLD: f32 = 3.0; // charge_time
const VORTEX_RECOVER: f32 = 3.0; // charge_cooldown_time
const VORTEX_DELAY: f32 = 1.0; // charge_cooldown_delay
const VORTEX_RETURN_DAMAGE: f32 = 140.0; // damage_near_value_titanarmor
const VORTEX_RETURN_PILOT_DAMAGE: f32 = 35.0; // damage_near_value
/// The returned projectiles' explosion (mp_titanweapon_vortex_shield.txt, SP_BASE).
const VORTEX_RETURN_EXPLOSION: f32 = 300.0;
const VORTEX_RETURN_EXPLOSION_INNER: f32 = 25.0;
const VORTEX_RETURN_EXPLOSION_RADIUS: f32 = 175.0;
/// CreateVortexSphere( weapon, false, false, sphereRadius = 150, bulletFOV = 120 ): a sphere
/// around the Titan that catches fire arriving within 60 degrees of where it faces.
pub const VORTEX_RADIUS: f32 = 150.0;
pub const VORTEX_BULLET_FOV: f32 = 120.0;
/// The shield's visible face sits on the front of the sphere.
const VORTEX_DIST: f32 = VORTEX_RADIUS;
/// From the cockpit the disc is drawn farther out than the catch sphere so its rim rings the
/// middle of the view (about 28 degrees out), as the dome's edge does in the game.
const VORTEX_DIST_FP: f32 = VORTEX_RADIUS * 1.9;

// Thermal Shield (mp_titanweapon_heat_shield, SP_BASE): held up to charge_time, recharging over
// charge_cooldown_time after charge_cooldown_delay; burns what is in front at fire_rate.
const HEAT_HOLD: f32 = 3.0;
const HEAT_RECOVER: f32 = 8.0;
const HEAT_DELAY: f32 = 1.0;
/// HeatShieldDamage_Think: a RadiusDamage every 0.1 s (after a first 0.1 s "letting the weapon
/// raise"), explosionradius 175 around the blocker's `muzzle_flash`, for damage_far_value(_titanarmor).
const HEAT_TICK: f32 = 0.1;
const HEAT_RADIUS: f32 = 175.0;
const HEAT_TITAN: f32 = 200.0;
const HEAT_PILOT: f32 = 25.0;
/// ShouldHeatShieldDamage: only what lies within 70 degrees of the view, from the eye.
const HEAT_CONE: f32 = 70.0;
/// The blocker viewmodel's `muzzle_flash` (atpov_vortexblocker_scorch.mdl, ads_in_seq) from
/// the camera bone: forward, left, up. The server reads the world model's attachment on the
/// Titan's hand; the viewmodel's is used for it.
const HEAT_MUZZLE: Vec3 = Vec3::new(105.0, 24.0, -20.0);
/// StartHeatShield: CreateVortexSphere( weapon, false, false, 120, HEAT_SHIELD_FOV 120 ).
const HEAT_SPHERE_RADIUS: f32 = 120.0;
/// Ion's Vortex needs some energy to raise.
const ION_VORTEX_MIN: f32 = 100.0;

/// Which shield the loadout's defensive slot raises (set by titankit).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VortexMode {
    /// Catch rounds and throw them back on release.
    #[default]
    Vortex,
    /// Ion's: the same, but drains the shared energy pool instead of its own charge.
    Ion,
    /// Scorch's Thermal Shield: burns rounds and everything close in front.
    Heat,
}

// Titan punch.
const MELEE_RANGE: f32 = 280.0;
const MELEE_ANGLE: f32 = 30.0;
const MELEE_DAMAGE: f32 = 500.0;
const MELEE_HIT_TIME: f32 = 0.25;
pub const MELEE_ANIM: f32 = 0.8;
const MELEE_RECOVERY: f32 = 1.0;

#[derive(Component, Clone)]
pub struct TitanHealth {
    pub v: Vitals,
    /// The values a fresh BT starts with (titan_buddy.set).
    pub full: Vitals,
    pub dead_for: Option<f32>,
    /// Seconds of red damage flash left.
    pub hurt: f32,
    /// Damage multiplier while blocking (Ronin's Sword Block), else 1.
    pub block: f32,
    /// While blocking: BT's eye and facing, for the block's 150 degree cone.
    pub block_cone: Option<(Vec3, Vec3)>,
}

/// TITAN_BLOCK_ANGLE (mp_titanability_basic_block.nut): hits whose origin lies within this
/// angle of the sword's facing are blocked; only shots from behind get through.
pub const BLOCK_ANGLE: f32 = 150.0;

impl TitanHealth {
    pub fn new(full: Vitals) -> Self {
        Self {
            v: full.clone(),
            full,
            dead_for: None,
            hurt: 0.0,
            block: 1.0,
            block_cone: None,
        }
    }
    pub fn reset(&mut self) {
        *self = Self::new(self.full.clone());
    }
    /// Damage that a Sword Block can't stop (melee, rodeo, doom, tests: `shouldPassThroughDamage`).
    pub fn damage(&mut self, dmg: f32, stops_regen: bool) -> Hit {
        self.apply(dmg, stops_regen)
    }
    /// Damage from `origin` (game space): scaled by the Sword Block when it comes from inside
    /// the block cone (`BasicBlock_OnDamage`).
    pub fn damage_from(&mut self, dmg: f32, stops_regen: bool, origin: Vec3) -> Hit {
        let blocked = self.block < 1.0
            && self.block_cone.is_none_or(|(eye, fwd)| {
                (origin - eye).normalize_or_zero().dot(fwd) >= BLOCK_ANGLE.to_radians().cos()
            });
        self.apply(if blocked { dmg * self.block } else { dmg }, stops_regen)
    }
    fn apply(&mut self, dmg: f32, stops_regen: bool) -> Hit {
        if self.dead_for.is_some() {
            return Hit::default();
        }
        let hit = self.v.damage(dmg, stops_regen);
        if hit.dealt > 0.0 {
            self.hurt = 0.15;
            log::debug!(
                "BT hit: {dmg:.1} dealt {:.1} -> health {:.0} shield {:.0}",
                hit.dealt,
                self.v.health,
                self.v.shield
            );
        }
        // TF_BT_GOD=1: BT logs the hits but keeps his health (testing enemy weapons).
        static GOD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *GOD.get_or_init(|| std::env::var_os("TF_BT_GOD").is_some_and(|v| v == "1")) {
            self.v.health = self.v.max_health;
            return hit;
        }
        if hit.doomed_now {
            log::info!("BT doomed");
        }
        if hit.killed {
            self.dead_for = Some(0.0);
            log::info!("BT destroyed");
        }
        hit
    }
}

#[derive(Component)]
pub struct Vortex {
    /// Remaining hold, 0..VORTEX_HOLD.
    pub charge: f32,
    pub active: bool,
    pub caught: u32,
    /// What each caught round was (its refire behaviour and the firing kit, `targets::Bolt::kit`)
    /// for the release.
    pub rounds: Vec<(VortexRefire, usize)>,
    /// Rounds swallowed without being returned (absorb-type weapons) this activation.
    pub absorbed: u32,
    delay: f32,
    shield: Option<Entity>,
    /// The spawned shield effect is the first-person (`_FP`) variant.
    shield_fp: bool,
    /// The spawned shield rides BT's `jx_vortex` joint (third person) instead of being placed
    /// each frame.
    shield_arm: bool,
    pub mode: VortexMode,
    burn_timer: f32,
}

impl Default for Vortex {
    fn default() -> Self {
        Self {
            charge: VORTEX_HOLD,
            active: false,
            caught: 0,
            rounds: Vec::new(),
            absorbed: 0,
            delay: 0.0,
            shield: None,
            shield_fp: false,
            shield_arm: false,
            mode: VortexMode::Vortex,
            burn_timer: 0.0,
        }
    }
}

impl Vortex {
    fn hold(&self) -> f32 {
        if self.mode == VortexMode::Heat {
            HEAT_HOLD
        } else {
            VORTEX_HOLD
        }
    }
    pub fn fraction(&self) -> f32 {
        self.charge / self.hold()
    }
    /// The catch sphere's radius around the eye.
    pub fn sphere_radius(&self) -> f32 {
        if self.mode == VortexMode::Heat {
            HEAT_SPHERE_RADIUS
        } else {
            VORTEX_RADIUS
        }
    }
}

/// A caught round's effect in the Vortex (a child of BT), removed when the shield goes down.
#[derive(Component)]
pub struct VortexCaughtFx;

/// GetBulletCollectionOffset for a Titan: (300, -90, -70) from the eye in the view's frame
/// (forward, left, up), where caught rounds swirl to (the absorb effects' control point 1).
pub fn vortex_collection_point(eye: Vec3, yaw: f32, pitch: f32) -> Vec3 {
    let fwd = Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    );
    let left = Vec3::new(-yaw.sin(), yaw.cos(), 0.0);
    let up = fwd.cross(left);
    eye + fwd * 300.0 - left * 90.0 - up * 70.0
}

/// Keep the caught rounds' collection point in front of BT's view (the game parents it to the
/// Titan), and remove their effects when the Vortex goes down (released, broken or stowed).
pub fn update_vortex_caught_fx(
    mut commands: Commands,
    settings: Res<crate::player::TitanSettings>,
    titans: Query<(&crate::player::PlayerTitan, &Vortex)>,
    mut fx: Query<(Entity, &mut crate::pfx::PfxBeamEnd), With<VortexCaughtFx>>,
) {
    let Ok((titan, vortex)) = titans.single() else {
        return;
    };
    let eye = Vec3::from(titan.state.eye(&settings.0).to_array());
    let end = crate::player::to_bevy(vortex_collection_point(
        eye,
        titan.state.yaw,
        titan.state.pitch,
    ));
    for (e, mut cp) in &mut fx {
        if vortex.active {
            cp.0 = end;
        } else {
            commands.entity(e).despawn();
        }
    }
}

#[derive(Component, Default)]
pub struct Melee {
    /// Time since the punch started; None when idle.
    pub t: Option<f32>,
    hit_done: bool,
    pub cooldown: f32,
}

fn rand(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

fn spawn_tracer(commands: &mut Commands, _fx: &FxAssets, from: Vec3, to: Vec3, enemy: bool) {
    let color = if enemy {
        Vec3::new(5.0, 1.0, 0.5)
    } else {
        Vec3::new(4.0, 2.2, 0.8)
    };
    let (a, b) = (to_bevy(from), to_bevy(to));
    crate::particles::emit(
        commands,
        crate::particles::Effect::Tracer {
            from: a,
            to: b,
            width: 0.12,
            color,
        },
    );
    crate::particles::emit(
        commands,
        crate::particles::Effect::Impact {
            at: b,
            normal: (a - b).normalize_or(Vec3::Y),
            scale: 1.6,
            energy: false,
        },
    );
}

/// Health regen, death and respawn; Vortex and melee input.
#[allow(clippy::too_many_arguments)]
pub fn player_combat(
    mut commands: Commands,
    time: Res<Time>,
    mut input: ResMut<PlayerInput>,
    mode: Res<CameraMode>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    fx: Res<FxAssets>,
    mut player: Query<(
        &mut PlayerTitan,
        &mut TitanHealth,
        &mut Vortex,
        &mut Melee,
        Option<&mut crate::abilities::TitanCore>,
        Option<&mut crate::titankit::KitState>,
    )>,
    mut dummies: Query<&mut Enemy>,
    mut shields: Query<&mut Transform>,
    kit: Option<Res<crate::titankit::ActiveKit>>,
    actors: Query<&crate::actor::Actor>,
    kits: Res<crate::targets::EnemyKits>,
    pfx: Option<Res<crate::pfx::Pfx>>,
) {
    let dt = time.delta_secs();
    let Ok((mut titan, mut health, mut vortex, mut melee, mut core, kit_state)) =
        player.single_mut()
    else {
        return;
    };
    // Only the Vortex family uses the shield here; other defensives live in titankit.
    use crate::titankit::DefKind;
    let vortex_kit = kit.as_ref().is_none_or(|k| {
        matches!(
            k.def,
            DefKind::Vortex | DefKind::IonVortex | DefKind::HeatShield
        )
    });
    let (hold, recover, delay) = match vortex.mode {
        VortexMode::Heat => (HEAT_HOLD, HEAT_RECOVER, HEAT_DELAY),
        _ => (VORTEX_HOLD, VORTEX_RECOVER, VORTEX_DELAY),
    };
    let frozen = *mode == CameraMode::Free;
    health.hurt = (health.hurt - dt).max(0.0);

    // A destroyed BT stays down until the pilot calls in a new Titanfall.
    if let Some(t) = health.dead_for {
        health.dead_for = Some(t + dt);
        input.vortex = false;
        input.melee = false;
    }

    health.v.tick(dt);

    // Titans can't walk through each other: push BT out of living dummies' hulls.
    for d in dummies.iter().filter(|d| d.alive()) {
        let p = Vec3::from(titan.state.pos.to_array());
        let to = (p - d.pos).truncate();
        let min = d.radius + settings.0.radius;
        if to.length() < min && (p.z - d.pos.z).abs() < d.height {
            let push = to.normalize_or(Vec2::X) * (min - to.length());
            titan.state.pos.x += push.x;
            titan.state.pos.y += push.y;
        }
    }
    let s = titan.state.clone();
    let eye = Vec3::from(s.eye(&settings.0).to_array());
    let view = Vec3::new(
        s.pitch.cos() * s.yaw.cos(),
        s.pitch.cos() * s.yaw.sin(),
        -s.pitch.sin(),
    );

    // --- Vortex Shield ---
    let want = input.vortex && !frozen && vortex_kit;
    // Ion's shield runs on the shared energy pool (drained in titankit).
    let energy = kit_state.as_ref().map(|k| k.energy).unwrap_or(0.0);
    if vortex.mode == VortexMode::Ion {
        vortex.charge = (energy / 1000.0) * hold;
    }
    let can_raise = match vortex.mode {
        VortexMode::Ion => energy > ION_VORTEX_MIN,
        _ => vortex.charge > hold * 0.2,
    };
    if want && !vortex.active && can_raise {
        vortex.active = true;
        vortex.caught = 0;
        vortex.rounds.clear();
        vortex.absorbed = 0;
        vortex.burn_timer = HEAT_TICK;
        audio::cue(&mut commands, Cue::VortexStart, None);
    }
    if vortex.active {
        if vortex.mode != VortexMode::Ion {
            vortex.charge -= dt;
        }
        // Thermal Shield (HeatShieldDamage_Think): every 0.1 s, everything within 175 of the
        // blocker's muzzle and within 70 degrees of the view burns (HeatShield_DamagedEntity
        // plays P_wpn_HeatSheild_burn_titan/_human and the burn sound on each victim).
        if vortex.mode == VortexMode::Heat {
            vortex.burn_timer -= dt;
            if vortex.burn_timer <= 0.0 {
                vortex.burn_timer += HEAT_TICK;
                let rot = Quat::from_rotation_z(s.yaw) * Quat::from_rotation_y(s.pitch);
                let muzzle = eye + rot * HEAT_MUZZLE;
                for mut d in &mut dummies {
                    if !d.alive() {
                        continue;
                    }
                    // Nearest point of the target's hull to the muzzle.
                    let flat = (muzzle - d.pos).truncate();
                    let side = (flat.length() - d.radius).max(0.0);
                    let up = (d.pos.z - muzzle.z)
                        .max(muzzle.z - (d.pos.z + d.height))
                        .max(0.0);
                    let center = d.pos + Vec3::Z * d.height * 0.5;
                    let facing = (center - eye).normalize_or_zero().dot(view)
                        >= HEAT_CONE.to_radians().cos();
                    if side.hypot(up) < HEAT_RADIUS && facing {
                        let amount = if d.infantry { HEAT_PILOT } else { HEAT_TITAN };
                        let hit = d.damage_radius(amount, false);
                        if let Some(c) = core.as_mut() {
                            c.credit_inflicted(hit);
                        }
                        // The titan's burn sits on exp_torso_front; a point up its front stands in.
                        let (fx, sound, at) = if d.infantry {
                            (
                                "P_wpn_HeatSheild_burn_human",
                                "heat_shield_burn_human_3p",
                                d.pos,
                            )
                        } else {
                            (
                                "P_wpn_HeatSheild_burn_titan",
                                "heat_shield_burn_titan_3p",
                                center
                                    + (eye - center).truncate().normalize_or_zero().extend(0.0)
                                        * d.radius
                                        * 0.8
                                    + Vec3::Z * d.height * 0.15,
                            )
                        };
                        crate::pfx::emit_named(
                            &mut commands,
                            fx,
                            to_bevy(at),
                            to_bevy(eye - at).normalize_or(Vec3::Y),
                        );
                        audio::event_at(&mut commands, sound, at);
                    }
                }
            }
        }
        if !want || vortex.charge <= 0.0 {
            // The Thermal Shield burns what it caught instead of throwing it back.
            if vortex.mode == VortexMode::Heat {
                vortex.absorbed += vortex.caught;
                vortex.caught = 0;
                vortex.rounds.clear();
            }
            // Release (VortexPrimaryAttack): bullets come back as one hitscan blast along the
            // crosshair (Vortex_FireBackBullets; the shield's tracer, 35 / 140 each), projectiles
            // one by one as the shield's own bolts (DoVortexAttackForImpactData): explosive
            // rounds at 8000 u/s under 0.3 gravity, rockets at 1800, grenades at 1500, each with
            // the Vortex Shield's damage (35 / 140, explosion 300 in 175) and the original
            // weapon's look (Vortex_ProjectileCommonSetup).
            vortex.active = false;
            vortex.delay = delay;
            audio::cue(
                &mut commands,
                if vortex.caught > 0 {
                    Cue::VortexThrow
                } else {
                    Cue::VortexEnd
                },
                None,
            );
            vortex.charge = vortex.charge.max(0.0);
            let game_fx = pfx.as_deref().is_some_and(|p| p.ready())
                && std::env::var_os("TF_OLD_FX").is_none();
            // A fresh stream per release, independent of the clock (repeatable runs).
            static RELEASES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let mut rng = RELEASES
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .wrapping_mul(0x9E3779B97F4A7C15)
                | 1;
            let rounds = std::mem::take(&mut vortex.rounds);
            let from = eye + view * VORTEX_DIST;
            for &(refire, kit) in &rounds {
                let jitter = |rng: &mut u64, xy: f32, z: f32| {
                    Vec3::new(
                        (rand(rng) - 0.5) * 2.0 * xy,
                        (rand(rng) - 0.5) * 2.0 * xy,
                        (rand(rng) - 0.5) * 2.0 * z,
                    )
                };
                match refire {
                    VortexRefire::Bullet => {
                        let dir = (view + jitter(&mut rng, 0.04, 0.04)).normalize();
                        let mut hit_t = world
                            .0
                            .raycast(
                                SVec3::from(from.to_array()),
                                SVec3::from(dir.to_array()),
                                12000.0,
                            )
                            .map(|h| h.t)
                            .unwrap_or(12000.0);
                        let mut victim = None;
                        let mut nearest = f32::MAX;
                        for (i, d) in dummies.iter().enumerate() {
                            if d.alive() {
                                let c = d.pos + Vec3::Z * d.height * 0.5;
                                let along = (c - from).dot(dir);
                                let miss = (from + dir * along).distance(c);
                                nearest = nearest.min(miss);
                                if along > 0.0 && along < hit_t && miss < d.radius + 40.0 {
                                    hit_t = along;
                                    victim = Some(i);
                                }
                            }
                        }
                        if let Some(i) = victim {
                            if let Some(mut d) = dummies.iter_mut().nth(i) {
                                let hit = d.damage(VORTEX_RETURN_DAMAGE, false);
                                if let Some(c) = core.as_mut() {
                                    c.credit_inflicted(hit);
                                }
                                log::debug!("vortex return hit enemy {i} at {hit_t:.0} for {:.0} (health {:.0})", hit.dealt, d.v.health);
                            }
                        } else {
                            log::debug!("vortex return missed: nearest enemy centre {nearest:.0} off the line, world at {hit_t:.0}");
                        }
                        let stop = from + dir * hit_t;
                        if game_fx {
                            crate::pfx::emit_beam(
                                &mut commands,
                                "weapon_tracers_vortex",
                                to_bevy(from),
                                to_bevy(stop),
                            );
                            crate::particles::emit(
                                &mut commands,
                                crate::particles::Effect::Impact {
                                    at: to_bevy(stop),
                                    normal: -to_bevy(dir).normalize_or(Vec3::Y),
                                    scale: 1.6,
                                    energy: victim.is_none(),
                                },
                            );
                        } else {
                            spawn_tracer(&mut commands, &fx, from, stop, false);
                        }
                    }
                    VortexRefire::ExplosiveRound
                    | VortexRefire::Rocket
                    | VortexRefire::Grenade
                    | VortexRefire::GrenadeLongFuse => {
                        let k = &kits.0[(kit % crate::targets::ABILITY_BOLT).min(kits.0.len() - 1)];
                        let w = match (&k.ability, kit >= crate::targets::ABILITY_BOLT) {
                            (Some(a), true) => &a.weapon,
                            _ => &k.weapon,
                        };
                        let mut def = w.clone();
                        def.name = "mp_titanweapon_vortex_shield".to_string();
                        def.damage_near = VORTEX_RETURN_DAMAGE;
                        def.damage_far = VORTEX_RETURN_DAMAGE;
                        def.damage_near_pilot = VORTEX_RETURN_PILOT_DAMAGE;
                        def.damage_far_pilot = VORTEX_RETURN_PILOT_DAMAGE;
                        def.explosion_damage = VORTEX_RETURN_EXPLOSION;
                        def.npc_explosion_damage_heavy = VORTEX_RETURN_EXPLOSION;
                        def.explosion_inner_radius = VORTEX_RETURN_EXPLOSION_INNER;
                        def.explosion_radius = VORTEX_RETURN_EXPLOSION_RADIUS;
                        def.crit = false;
                        def.headshot_scale = 1.0;
                        def.burn = None;
                        let (speed, gravity, sxy, sz) = match refire {
                            VortexRefire::ExplosiveRound => (8000.0, 0.3, 0.15, 0.075),
                            VortexRefire::Rocket => (1800.0, 0.0, 0.0, 0.0),
                            _ => (1500.0, 1.0, 0.2, 0.2),
                        };
                        let dir = (view + jitter(&mut rng, sxy, sz)).normalize();
                        let origin = from + jitter(&mut rng, 25.0, 25.0);
                        crate::pilotweapon::spawn_bolt(
                            &mut commands,
                            &fx,
                            origin,
                            dir * speed,
                            gravity,
                            &def,
                            true,
                        );
                        log::debug!("vortex return {refire:?} from {}", w.name);
                    }
                    VortexRefire::None | VortexRefire::Absorb => {}
                }
            }
            if vortex.caught > 0 || vortex.absorbed > 0 {
                log::info!(
                    "vortex returned {} rounds, absorbed {}",
                    vortex.caught,
                    vortex.absorbed
                );
            }
            vortex.caught = 0;
        }
    } else if vortex.delay > 0.0 {
        vortex.delay -= dt;
    } else if vortex.mode != VortexMode::Ion {
        vortex.charge = (vortex.charge + dt * hold / recover).min(hold);
    }
    // The shield disc itself. The game plays a separate `_FP` system for the owner's view.
    let fp = *mode == CameraMode::Cockpit;
    let game_fx = std::env::var_os("TF_OLD_FX").is_none();
    // Where the shield's effect sits. The game plays the Vortex's on the blocker viewmodel's
    // `vortex_center` (atpov_vortexblocker.mdl, held at the end of its draw_seq: 105 forward,
    // 24 left, 20.5 down from the camera bone, its X axis turned slightly right and up), and
    // the effect's dome model (fx\vortex_shield_hld, 60 units across) sits 10 units out along
    // that axis. Elsewhere (third person, the heat shield, TF_OLD_FX's disc) it stays on the
    // view line. In third person the blocker's world model (w_vortexblocker.mdl, rooted at
    // jx_vortex) bone-merges onto BT's jx_vortex joint, which bt_vortex_arm_pose_add raises;
    // the model's idle_seq holds its vortex_center tag at (25.59, 11.81, 0) in that frame,
    // unrotated (ahead of the raised hand, facing out). The shield rides the raised arm (the
    // scale undoes the actor's UNIT).
    // BT plays the pose mirrored (player.rs), so the shield goes on the twin of the arm
    // jx_vortex hangs from: the tag's frame under def_r_elbow (jx_vortex's local in the pose's
    // first frame, then the tag) is carried over to def_l_elbow.
    const TAG: Vec3 = Vec3::new(25.59, 11.81, 0.0);
    let arm = actors.get(titan.actor).ok().and_then(|a| {
        let jx = a.bone_names.iter().position(|n| n == "jx_vortex")?;
        let pose = a
            .clip("bt_vortex_arm_pose_add")
            .or_else(|| a.clip("mp_vortex_add"))?;
        let (lp, lq) = a.clips[pose].frames.first()?[jx];
        let (bone, p, [fwd, up]) =
            a.mirrored_frame("def_r_elbow", lp + lq * TAG, [lq * Vec3::X, lq * Vec3::Z])?;
        Some((
            a.joint(bone)?,
            Transform::from_translation(p).looking_to(fwd, up),
        ))
    });
    let arm_joint = arm.map(|a| a.0);
    let arm_tf = arm
        .map_or(Transform::IDENTITY, |a| a.1)
        .with_scale(Vec3::splat(1.0 / crate::player::UNIT));
    let shield_tf = |fp: bool, heat: bool| {
        if fp && game_fx && !heat {
            let r = Quat::from_rotation_z(s.yaw) * Quat::from_rotation_y(s.pitch);
            let at = eye + r * Vec3::new(105.1, 24.0, -20.5);
            let (fwd, up) = (
                r * Vec3::new(0.936, -0.302, 0.18),
                r * Vec3::new(-0.323, -0.942, 0.1),
            );
            Transform::from_translation(to_bevy(at))
                .looking_to(to_bevy(fwd).normalize(), to_bevy(up).normalize())
        } else {
            let dist = if fp { VORTEX_DIST_FP } else { VORTEX_DIST };
            Transform::from_translation(to_bevy(eye + view * dist))
                .looking_to(to_bevy(view).normalize(), Vec3::Y)
        }
    };
    if let (true, Some(e)) = (vortex.active, vortex.shield) {
        if vortex.shield_fp != fp {
            commands.entity(e).despawn();
            vortex.shield = None;
        }
    }
    match (vortex.active, vortex.shield) {
        (true, None) => {
            let heat = vortex.mode == VortexMode::Heat;
            let (mesh, mat) = if heat {
                fx.heat_disc.clone()
            } else {
                fx.vortex_disc.clone()
            };
            let arm = arm_joint.filter(|_| !fp && game_fx && !heat);
            let root = commands
                .spawn((
                    if arm.is_some() {
                        arm_tf
                    } else {
                        shield_tf(fp, heat)
                    },
                    Visibility::default(),
                ))
                .id();
            if let Some(j) = arm {
                commands.entity(j).add_child(root);
            }
            vortex.shield_arm = arm.is_some();
            if game_fx {
                // mp_titanweapon_vortex_shield.nut's fxChargingFPControlPoint /
                // fxChargingControlPoint (the dome model, its glow and trails).
                let name = match (heat, fp) {
                    (true, true) => "P_wpn_HeatShield_FP",
                    (true, false) => "P_wpn_HeatShield",
                    (false, true) => "wpn_vortex_chargingCP_titan_FP",
                    (false, false) => "wpn_vortex_chargingCP_titan",
                };
                commands
                    .entity(root)
                    .insert(crate::pfx::PfxTrail::oriented(name));
            }
            vortex.shield_fp = fp;
            // The effect's dome model (fx\vortex_shield_hld) is drawn by the game's own shader
            // (fxuber.rs), at the particle's radius (3 in the cockpit, 4 outside: 180 and 240
            // units across). The disc stands in for it only with TF_OLD_FX or TF_OLD_SHIELDS:
            // in the cockpit where the dome sits (10 units out along the tag's axis) at the
            // model's own size, 60 units across, on BT's arm likewise. The heat shield's flame
            // fan is a drawn model.
            if !game_fx || (!heat && std::env::var_os("TF_OLD_SHIELDS").is_some()) {
                // (The size rides on a holder: the shimmer animates the disc's own scale.)
                let local = if (fp || vortex.shield_arm) && game_fx {
                    Transform::from_translation(Vec3::NEG_Z * 10.0 * crate::player::UNIT)
                        .with_scale(Vec3::splat(30.0 / VORTEX_RADIUS))
                } else {
                    Transform::IDENTITY
                };
                let holder = commands.spawn((local, Visibility::default())).id();
                commands.entity(root).add_child(holder);
                let disc = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(mat),
                        Transform::IDENTITY,
                        crate::weapons::ShieldShimmer,
                    ))
                    .id();
                commands.entity(holder).add_child(disc);
            }
            vortex.shield = Some(root);
        }
        (false, Some(e)) => {
            commands.entity(e).despawn();
            vortex.shield = None;
        }
        _ => {}
    }
    // mp_titanweapon_vortex_shield.nut's VortexSphereColorUpdate: control point 1 carries
    // GetVortexSphereCurrentColor( weapon.GetWeaponChargeFraction() ), blue while fresh,
    // orange then red as the hold runs out (Ion's: 1 - its energy fraction).
    if let (Some(e), true) = (vortex.shield, game_fx && vortex.mode != VortexMode::Heat) {
        commands
            .entity(e)
            .insert(crate::pfx::PfxColor(crate::fxuber::shield_color(
                1.0 - vortex.fraction(),
            )));
    }
    if let Some(e) = vortex.shield.filter(|_| !vortex.shield_arm) {
        if let Ok(mut tf) = shields.get_mut(e) {
            // Forward = outward, the way enemy shields orient their effects (the disc is double-sided).
            *tf = shield_tf(vortex.shield_fp, vortex.mode == VortexMode::Heat);
        }
    }

    // --- Titan punch ---
    melee.cooldown = (melee.cooldown - dt).max(0.0);
    if input.melee && melee.t.is_none() && melee.cooldown <= 0.0 && !frozen {
        melee.t = Some(0.0);
        melee.hit_done = false;
        audio::cue(&mut commands, Cue::BtPunch, None);
        melee.cooldown = MELEE_ANIM + MELEE_RECOVERY;
    }
    input.melee = false;
    if let Some(t) = melee.t {
        let t = t + dt;
        if t >= MELEE_HIT_TIME && !melee.hit_done {
            melee.hit_done = true;
            let fwd = Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0);
            for mut d in &mut dummies {
                if !d.alive() {
                    continue;
                }
                let to = (d.pos - Vec3::from(s.pos.to_array())).truncate();
                let dist = to.length() - d.radius;
                let ang = to
                    .normalize_or_zero()
                    .dot(fwd.truncate())
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees();
                let (range, damage) = kit_state
                    .as_ref()
                    .map(|k| (k.melee_range, k.melee_damage))
                    .unwrap_or((MELEE_RANGE, MELEE_DAMAGE));
                if dist < range && ang < MELEE_ANGLE {
                    let hit = d.damage_unblockable(damage, false);
                    if let Some(c) = core.as_mut() {
                        c.credit_inflicted(hit);
                    }
                    log::info!("melee hit for {damage}");
                    audio::cue(
                        &mut commands,
                        Cue::PunchImpact,
                        Some(d.pos + Vec3::Z * 150.0),
                    );
                }
            }
        }
        melee.t = if t >= MELEE_ANIM { None } else { Some(t) };
    }
}
