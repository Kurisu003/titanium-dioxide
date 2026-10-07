//! Two-way combat: BT's segmented health and shield, dummy Titans firing back, the Vortex
//! Shield (catch rounds, fling them back) and the Titan punch.
//! Values from titan_buddy.set, mp_titanweapon_vortex_shield.txt and melee_titan_punch.txt.

use crate::player::{to_bevy, CameraMode, Collision, PlayerInput, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use crate::vitals::{Hit, Vitals};
use crate::weapons::FxAssets;
use crate::audio::{self, Cue};
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

/// HUD segment size (titan_buddy.set healthPerSegment); cosmetic, health is not segmented in SP.
pub const SEGMENT: f32 = 1800.0;
pub const SEGMENTS: u32 = 5;

// Vortex Shield.
const VORTEX_HOLD: f32 = 3.0; // charge_time
const VORTEX_RECOVER: f32 = 3.0; // charge_cooldown_time
const VORTEX_DELAY: f32 = 1.0; // charge_cooldown_delay
const VORTEX_RETURN_DAMAGE: f32 = 140.0; // damage_near_value_titanarmor
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
const HEAT_RATE: f32 = 5.0;
const HEAT_RANGE: f32 = 300.0; // damage_far_distance
const HEAT_TITAN: f32 = 200.0; // damage_near_value_titanarmor
const HEAT_PILOT: f32 = 25.0;
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
        Self { v: full.clone(), full, dead_for: None, hurt: 0.0, block: 1.0, block_cone: None }
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
            && self.block_cone.is_none_or(|(eye, fwd)| (origin - eye).normalize_or_zero().dot(fwd) >= BLOCK_ANGLE.to_radians().cos());
        self.apply(if blocked { dmg * self.block } else { dmg }, stops_regen)
    }
    fn apply(&mut self, dmg: f32, stops_regen: bool) -> Hit {
        if self.dead_for.is_some() {
            return Hit::default();
        }
        let hit = self.v.damage(dmg, stops_regen);
        if hit.dealt > 0.0 {
            self.hurt = 0.15;
            log::debug!("BT hit: {dmg:.1} dealt {:.1} -> health {:.0} shield {:.0}", hit.dealt, self.v.health, self.v.shield);
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
    /// Rounds swallowed without being returned (absorb-type weapons) this activation.
    pub absorbed: u32,
    delay: f32,
    shield: Option<Entity>,
    /// The spawned shield effect is the first-person (`_FP`) variant.
    shield_fp: bool,
    pub mode: VortexMode,
    burn_timer: f32,
}

impl Default for Vortex {
    fn default() -> Self {
        Self { charge: VORTEX_HOLD, active: false, caught: 0, absorbed: 0, delay: 0.0, shield: None, shield_fp: false, mode: VortexMode::Vortex, burn_timer: 0.0 }
    }
}

impl Vortex {
    fn hold(&self) -> f32 {
        if self.mode == VortexMode::Heat { HEAT_HOLD } else { VORTEX_HOLD }
    }
    pub fn fraction(&self) -> f32 {
        self.charge / self.hold()
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
    let color = if enemy { Vec3::new(5.0, 1.0, 0.5) } else { Vec3::new(4.0, 2.2, 0.8) };
    let (a, b) = (to_bevy(from), to_bevy(to));
    crate::particles::emit(commands, crate::particles::Effect::Tracer { from: a, to: b, width: 0.12, color });
    crate::particles::emit(commands, crate::particles::Effect::Impact { at: b, normal: (a - b).normalize_or(Vec3::Y), scale: 1.6, energy: false });
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
    mut player: Query<(&mut PlayerTitan, &mut TitanHealth, &mut Vortex, &mut Melee, Option<&mut crate::abilities::TitanCore>, Option<&mut crate::titankit::KitState>)>,
    mut dummies: Query<&mut Enemy>,
    mut shields: Query<&mut Transform>,
    kit: Option<Res<crate::titankit::ActiveKit>>,
) {
    let dt = time.delta_secs();
    let Ok((mut titan, mut health, mut vortex, mut melee, mut core, kit_state)) = player.single_mut() else { return };
    // Only the Vortex family uses the shield here; other defensives live in titankit.
    use crate::titankit::DefKind;
    let vortex_kit = kit.as_ref().is_none_or(|k| matches!(k.def, DefKind::Vortex | DefKind::IonVortex | DefKind::HeatShield));
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
    let view = Vec3::new(s.pitch.cos() * s.yaw.cos(), s.pitch.cos() * s.yaw.sin(), -s.pitch.sin());

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
        vortex.absorbed = 0;
        vortex.burn_timer = 0.0;
        audio::cue(&mut commands, Cue::VortexStart, None);
    }
    if vortex.active {
        if vortex.mode != VortexMode::Ion {
            vortex.charge -= dt;
        }
        // Thermal Shield: burn everything close in front of BT at fire_rate.
        if vortex.mode == VortexMode::Heat {
            vortex.burn_timer -= dt;
            if vortex.burn_timer <= 0.0 {
                vortex.burn_timer += 1.0 / HEAT_RATE;
                let p = Vec3::from(titan.state.pos.to_array());
                for mut d in &mut dummies {
                    if !d.alive() {
                        continue;
                    }
                    let to = d.pos - p;
                    let dist = to.truncate().length() - d.radius;
                    let ang = to.truncate().normalize_or_zero().dot(Vec2::new(titan.state.yaw.cos(), titan.state.yaw.sin()));
                    if dist < HEAT_RANGE && ang > (VORTEX_BULLET_FOV * 0.5).to_radians().cos() {
                        let amount = if d.infantry { HEAT_PILOT } else { HEAT_TITAN };
                        let hit = d.damage(amount, false);
                        if let Some(c) = core.as_mut() {
                            c.credit_inflicted(hit);
                        }
                    }
                }
            }
        }
        if !want || vortex.charge <= 0.0 {
            // The Thermal Shield burns what it caught instead of throwing it back.
            if vortex.mode == VortexMode::Heat {
                vortex.absorbed += vortex.caught;
                vortex.caught = 0;
            }
            // Release: fling every caught round back along the crosshair.
            vortex.active = false;
            vortex.delay = delay;
            audio::cue(&mut commands, if vortex.caught > 0 { Cue::VortexThrow } else { Cue::VortexEnd }, None);
            vortex.charge = vortex.charge.max(0.0);
            let mut rng = (time.elapsed_secs() * 1000.0) as u64 | 1;
            for _ in 0..vortex.caught {
                let jitter = Vec3::new(rand(&mut rng) - 0.5, rand(&mut rng) - 0.5, rand(&mut rng) - 0.5) * 0.08;
                let dir = (view + jitter).normalize();
                let from = eye + view * VORTEX_DIST;
                let mut hit_t = world.0.raycast(SVec3::from(from.to_array()), SVec3::from(dir.to_array()), 12000.0).map(|h| h.t).unwrap_or(12000.0);
                let mut victim = None;
                for (i, d) in dummies.iter().enumerate() {
                    if d.alive() {
                        let c = d.pos + Vec3::Z * d.height * 0.5;
                        let along = (c - from).dot(dir);
                        if along > 0.0 && along < hit_t && (from + dir * along).distance(c) < d.radius + 40.0 {
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
                    }
                }
                spawn_tracer(&mut commands, &fx, from, from + dir * hit_t, false);
            }
            if vortex.caught > 0 || vortex.absorbed > 0 {
                log::info!("vortex returned {} rounds, absorbed {}", vortex.caught, vortex.absorbed);
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
    if let (true, Some(e)) = (vortex.active, vortex.shield) {
        if vortex.shield_fp != fp {
            commands.entity(e).despawn();
            vortex.shield = None;
        }
    }
    match (vortex.active, vortex.shield) {
        (true, None) => {
            let heat = vortex.mode == VortexMode::Heat;
            let (mesh, mat) = if heat { fx.heat_disc.clone() } else { fx.vortex_disc.clone() };
            // The game's shield effect rides the same entity (the dome itself is a refraction
            // material in the game; the disc stands in for it).
            let dist = if fp { VORTEX_DIST_FP } else { VORTEX_DIST };
            let at = Transform::from_translation(to_bevy(eye + view * dist)).looking_to(to_bevy(view).normalize(), Vec3::Y);
            let root = commands.spawn((at, Visibility::default())).id();
            let game_fx = std::env::var_os("TF_OLD_FX").is_none();
            if game_fx {
                let name = match (heat, fp) {
                    (true, true) => "P_wpn_HeatShield_FP",
                    (true, false) => "P_wpn_HeatShield",
                    (false, true) => "wpn_vortex_shield_charging_FP",
                    (false, false) => "wpn_vortex_shield_charging",
                };
                commands.entity(root).insert(crate::pfx::PfxTrail::oriented(name));
            }
            vortex.shield_fp = fp;
            // The vortex dome is a refraction material (pinched_ring_distort) that we cannot
            // draw, so the disc stands in for it; the heat shield's flame fan is a drawn model.
            if !(game_fx && heat) {
                let disc = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, crate::weapons::ShieldShimmer)).id();
                commands.entity(root).add_child(disc);
            }
            vortex.shield = Some(root);
        }
        (false, Some(e)) => {
            commands.entity(e).despawn();
            vortex.shield = None;
        }
        _ => {}
    }
    if let Some(e) = vortex.shield {
        if let Ok(mut tf) = shields.get_mut(e) {
            let c = to_bevy(eye + view * if vortex.shield_fp { VORTEX_DIST_FP } else { VORTEX_DIST });
            // Forward = outward, the way enemy shields orient their effects (the disc is double-sided).
            *tf = Transform::from_translation(c).looking_to(to_bevy(view).normalize(), Vec3::Y);
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
                let ang = to.normalize_or_zero().dot(fwd.truncate()).clamp(-1.0, 1.0).acos().to_degrees();
                let (range, damage) = kit_state.as_ref().map(|k| (k.melee_range, k.melee_damage)).unwrap_or((MELEE_RANGE, MELEE_DAMAGE));
                if dist < range && ang < MELEE_ANGLE {
                    let hit = d.damage_unblockable(damage, false);
                    if let Some(c) = core.as_mut() {
                        c.credit_inflicted(hit);
                    }
                    log::info!("melee hit for {damage}");
                    audio::cue(&mut commands, Cue::PunchImpact, Some(d.pos + Vec3::Z * 150.0));
                }
            }
        }
        melee.t = if t >= MELEE_ANIM { None } else { Some(t) };
    }
}


