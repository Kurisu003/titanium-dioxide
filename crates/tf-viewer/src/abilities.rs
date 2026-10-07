//! BT's offhand kit: the Multi-Target Missile System (hold to paint locks, release to
//! fire homing missiles from the shoulder pods) and Burst Core. Numbers come from
//! `mp_titanweapon_shoulder_rockets.txt` / `.nut` and `mp_titancore_amp_core.txt`.

use crate::actor::Actor;
use crate::player::{to_bevy, CameraMode, Collision, PlayerInput, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use crate::weapons::{Fx, FxAssets};
use crate::audio::{self, Cue};
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

// Multi-Target Missile System (SP values).
const LOCK_ANGLE: f32 = 80.0; // smart_ammo_search_angle (full cone)
const LOCK_RANGE: f32 = 4000.0; // smart_ammo_search_distance
const LOCK_TIME: f32 = 0.3; // smart_ammo_targeting_time
const MAX_LOCKS: usize = 12; // smart_ammo_max_targeted_burst
const MISSILE_RATE: f32 = 30.0; // fire_rate [SP]
const MISSILE_SPEED: f32 = 1000.0; // SHOULDERROCKETS_MISSILE_SPEED
const HOMING_SPEED: f32 = 250.0; // SmartAmmo_SetMissileHomingSpeed
const LAUNCH_OUT_TIME: f32 = 0.2;
const LAUNCH_OUT_ANGLE: f32 = 10.0;
const MISSILE_DAMAGE: f32 = 150.0; // damage_near_value_titanarmor [SP]
const MISSILE_SPLASH: f32 = 100.0; // explosion_damage_heavy_armor [SP]
const MISSILE_RADIUS: f32 = 200.0; // explosionradius [SP]; no inner radius, so linear falloff
const COOLDOWN: f32 = 18.0; // charge_cooldown_time
const COOLDOWN_DELAY: f32 = 2.0; // charge_cooldown_delay

// Burst Core.
pub const CORE_CHARGEUP: f32 = 1.85;
pub const CORE_DURATION: f32 = 5.5;
pub const CORE_FIRE_RATE: f32 = 20.0;
pub const CORE_DAMAGE: f32 = 150.0;
/// Full core from damage dealt (titan armor damage), plus passive build over core_build_time.

#[derive(Component, Default)]
pub struct Ordnance {
    pub cooldown: f32,
    pub delay: f32,
    pub charging: bool,
    lock_timer: f32,
    /// Painted locks: dummy entity per lock.
    pub locks: Vec<Entity>,
    /// Missiles still to launch in the current burst (target per missile).
    queue: Vec<Option<Entity>>,
    fire_timer: f32,
    pod_right: bool,
}

impl Ordnance {
    /// How recharged the launcher is, 0..1.
    pub fn fraction(&self) -> f32 {
        if self.delay > 0.0 { 0.0 } else { (1.0 - self.cooldown / COOLDOWN).clamp(0.0, 1.0) }
    }
    pub fn ready(&self) -> bool {
        self.cooldown <= 0.0 && self.queue.is_empty()
    }
    pub fn locks_on(&self, e: Entity) -> usize {
        self.locks.iter().filter(|&&l| l == e).count()
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum CoreState {
    #[default]
    Building,
    Charging(f32),
    Active(f32),
}

#[derive(Component)]
pub struct TitanCore {
    /// 0..1
    pub meter: f32,
    pub state: CoreState,
    /// BT has said the core is ready.
    announced: bool,
    /// Which core the loadout carries, its spin-up and how long it runs (set by titankit).
    pub kind: crate::titankit::CoreKind,
    pub chargeup: f32,
    pub duration: f32,
}

impl Default for TitanCore {
    fn default() -> Self {
        Self { meter: 0.0, state: CoreState::Building, announced: false, kind: crate::titankit::CoreKind::Burst, chargeup: CORE_CHARGEUP, duration: CORE_DURATION }
    }
}

impl TitanCore {
    pub fn active(&self) -> bool {
        matches!(self.state, CoreState::Active(_))
    }
    /// BT is slowed during the core's spin-up and firing (move_slow 0.4).
    pub fn move_scale(&self) -> f32 {
        if self.state == CoreState::Building { 1.0 } else { 0.6 }
    }
    /// Core meter credit for damaging an enemy Titan (titan_core_from_titan_damage: the meter
    /// only fills from Titan damage, never over time, and not while the core is in use).
    pub fn credit_inflicted(&mut self, hit: crate::vitals::Hit) {
        use crate::vitals::*;
        let credit = hit.dealt * CORE_CREDIT_PER_DAMAGE_INFLICTED + if hit.doomed_now { CORE_CREDIT_DOOM_INFLICTED } else { 0.0 };
        self.add_credit(credit);
    }
    pub fn credit_received(&mut self, dealt: f32) {
        self.add_credit(dealt * crate::vitals::CORE_CREDIT_PER_DAMAGE_RECEIVED);
    }
    fn add_credit(&mut self, credit: f32) {
        if self.state == CoreState::Building && credit > 0.0 {
            // JFS: >= 0.998 shows as full, so it is full.
            self.meter = (self.meter + credit).min(1.0);
            if self.meter >= 0.998 {
                self.meter = 1.0;
            }
        }
    }
}

#[derive(Component)]
pub struct Missile {
    /// Game-space position and velocity.
    pos: Vec3,
    vel: Vec3,
    target: Option<Entity>,
    age: f32,
    out_dir: Vec3,
    spec: MissileSpec,
}

/// How a missile flies and hits: the Multi-Target Missile System's by default; Salvo Core,
/// Tracker Rockets and Flight Core rockets use their own scripts' numbers.
#[derive(Clone, Copy, Debug)]
pub struct MissileSpec {
    pub speed: f32,
    /// Turn rate (0: flies straight).
    pub homing: f32,
    pub direct: f32,
    pub splash: f32,
    pub radius: f32,
    /// Fly straight out of the pod for this long before homing.
    pub out_time: f32,
}

pub const MTMS: MissileSpec = MissileSpec { speed: MISSILE_SPEED, homing: HOMING_SPEED, direct: MISSILE_DAMAGE, splash: MISSILE_SPLASH, radius: MISSILE_RADIUS, out_time: LAUNCH_OUT_TIME };

/// Launch a missile (with the rocket model) from `pos` along `dir`.
pub fn spawn_missile(commands: &mut Commands, assets: &MissileAssets, pos: Vec3, dir: Vec3, target: Option<Entity>, spec: MissileSpec) {
    let e = commands
        .spawn((Transform::from_translation(pos), Visibility::default(), Missile { pos, vel: dir * spec.speed, target, age: 0.0, out_dir: dir, spec }))
        .id();
    for (mesh, mat) in &assets.parts {
        let c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY)).id();
        commands.entity(e).add_child(c);
    }
    commands.entity(assets.root).add_child(e);
}

#[derive(Resource)]
pub struct MissileAssets {
    pub parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    pub root: Entity,
}

fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin())
}

/// Lock-on, launch and the core trigger. Runs after the movement simulation.
#[allow(clippy::too_many_arguments)]
pub fn update_abilities(
    mut commands: Commands,
    time: Res<Time>,
    mut input: ResMut<PlayerInput>,
    mode: Res<CameraMode>,
    settings: Res<TitanSettings>,
    assets: Option<Res<MissileAssets>>,
    mut titans: Query<(&PlayerTitan, &mut Ordnance, &mut TitanCore)>,
    dummies: Query<(Entity, &Enemy)>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    kit: Option<Res<crate::titankit::ActiveKit>>,
) {
    let dt = time.delta_secs();
    let Ok((titan, mut ord, mut core)) = titans.single_mut() else { return };
    let missiles = kit.as_ref().is_none_or(|k| k.ord == crate::titankit::OrdKind::Missiles);
    let frozen = *mode == CameraMode::Free;
    let s = &titan.state;

    // --- Burst Core ---
    core.state = match core.state {
        CoreState::Building => {
            if core.meter >= 1.0 && !core.announced {
                core.announced = true;
                audio::cue(&mut commands, Cue::BtCoreReady, None);
            }
            if input.core && core.meter >= 1.0 && !frozen {
                log::info!("{:?} core: charging", core.kind);
                core.announced = false;
                audio::cue(&mut commands, Cue::BtCoreActivated, None);
                CoreState::Charging(0.0)
            } else {
                CoreState::Building
            }
        }
        CoreState::Charging(t) if t + dt >= core.chargeup => {
            log::info!("{:?} core: online", core.kind);
            CoreState::Active(0.0)
        }
        CoreState::Charging(t) => CoreState::Charging(t + dt),
        CoreState::Active(t) => {
            core.meter = (1.0 - t / core.duration).max(0.0);
            if t + dt >= core.duration {
                core.meter = 0.0;
                CoreState::Building
            } else {
                CoreState::Active(t + dt)
            }
        }
    };
    input.core = false;

    // --- Multi-Target Missile System ---
    if ord.delay > 0.0 {
        ord.delay -= dt;
    } else if ord.cooldown > 0.0 {
        ord.cooldown = (ord.cooldown - dt).max(0.0);
    }
    let eye = Vec3::from(s.eye(&settings.0).to_array());
    let dir = view_dir(s.yaw, s.pitch);
    if !missiles {
        // Another loadout's ordnance; titankit handles it.
        ord.charging = false;
        ord.locks.clear();
    }
    if missiles && input.ordnance && ord.ready() && !frozen {
        ord.charging = true;
        ord.lock_timer += dt;
        // Paint one lock per LOCK_TIME on the targets in the search cone, spreading locks
        // across targets (closest-to-crosshair first).
        if ord.lock_timer >= LOCK_TIME && ord.locks.len() < MAX_LOCKS {
            ord.lock_timer = 0.0;
            let mut cands: Vec<(Entity, f32)> = dummies
                .iter()
                .filter(|(_, d)| d.alive())
                .filter_map(|(e, d)| {
                    let to = d.pos + Vec3::Z * d.height * 0.6 - eye;
                    let dist = to.length();
                    let ang = to.normalize().dot(dir).clamp(-1.0, 1.0).acos().to_degrees();
                    (dist < LOCK_RANGE && ang < LOCK_ANGLE * 0.5).then_some((e, ang))
                })
                .collect();
            cands.sort_by(|a, b| {
                let (la, lb) = (ord.locks_on(a.0), ord.locks_on(b.0));
                la.cmp(&lb).then(a.1.total_cmp(&b.1))
            });
            if let Some(&(e, _)) = cands.first() {
                ord.locks.push(e);
                audio::cue(&mut commands, Cue::MissileLock, None);
            }
        }
    }
    let released = ord.charging && (!input.ordnance || ord.locks.len() >= MAX_LOCKS || frozen);
    if released {
        ord.charging = false;
        ord.lock_timer = 0.0;
        // No locks: a single dumb-fire missile (SP behaviour).
        ord.queue = if ord.locks.is_empty() { vec![None] } else { ord.locks.drain(..).map(Some).collect() };
        ord.fire_timer = 0.0;
        ord.delay = COOLDOWN_DELAY;
        ord.cooldown = COOLDOWN;
        log::info!("missiles away: {}", ord.queue.len());
    }
    let Some(assets) = assets else { return };
    if !ord.queue.is_empty() {
        ord.fire_timer -= dt;
        while ord.fire_timer <= 0.0 && !ord.queue.is_empty() {
            ord.fire_timer += 1.0 / MISSILE_RATE;
            let target = ord.queue.remove(0);
            ord.pod_right = !ord.pod_right;
            // Launch from BT's shoulder pod bones (game space via the bone's world position).
            let pod = if ord.pod_right { "jx_r_rocketPod" } else { "jx_l_rocketPod" };
            let pod_pos = actors
                .get(titan.actor)
                .ok()
                .and_then(|a| a.joint(pod))
                .and_then(|j| globals.get(j).ok())
                .map(|g| from_bevy(g.translation()))
                .unwrap_or(eye);
            let side = Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0) * if ord.pod_right { 1.0 } else { -1.0 };
            let out = (dir + side * LAUNCH_OUT_ANGLE.to_radians().tan() + Vec3::Z * 0.15).normalize();
            spawn_missile(&mut commands, &assets, pod_pos, out, target, MTMS);
            audio::cue(&mut commands, Cue::MissileFire, None);
        }
    }
}

/// Inverse of `to_bevy`.
fn from_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y) / crate::player::UNIT
}

#[allow(clippy::too_many_arguments)]
pub fn update_missiles(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    _fx: Res<FxAssets>,
    mut missiles: Query<(Entity, &mut Missile, &mut Transform)>,
    mut dummies: Query<(Entity, &mut Enemy)>,
    mut cores: Query<&mut TitanCore>,
    mut enemy_walls: ResMut<crate::titankit::EnemyWalls>,
) {
    let dt = time.delta_secs();
    for (e, mut m, mut tf) in &mut missiles {
        m.age += dt;
        // Fly out from the pods briefly, then home on the painted target.
        let target_pos = m.target.and_then(|t| dummies.get(t).ok()).filter(|(_, d)| d.alive()).map(|(_, d)| d.pos + Vec3::Z * d.height * 0.6);
        let spec = m.spec;
        let speed = spec.speed;
        if m.age < spec.out_time {
            m.vel = m.out_dir * speed;
        } else if let (Some(tp), true) = (target_pos, spec.homing > 0.0) {
            let want = (tp - m.pos).normalize() * speed;
            let steer = (want - m.vel).clamp_length_max(spec.homing * 8.0 * dt * speed / MISSILE_SPEED);
            m.vel = (m.vel + steer).normalize() * speed;
        }
        let step = m.vel * dt;
        let mut hit_world = world.0.raycast(SVec3::from(m.pos.to_array()), SVec3::from(step.normalize().to_array()), step.length()).map(|h| h.t);
        // An enemy Tone's Particle Wall detonates the missile on it.
        if let Some((we, t)) = enemy_walls.hit(m.pos, step.normalize_or_zero(), hit_world.unwrap_or(step.length())) {
            hit_world = Some(t);
            enemy_walls.absorbed.push((we, spec.direct));
        }
        let hit_target = target_pos.map(|tp| m.pos.distance(tp) < 90.0).unwrap_or(false);
        let next = m.pos + step;
        if hit_target || hit_world.is_some() || m.age > 6.0 {
            let at = hit_world.map(|t| m.pos + step.normalize() * t).unwrap_or(next);
            // Direct hit plus splash on anything within the explosion radius.
            let mut hits = Vec::new();
            for (de, mut d) in &mut dummies {
                if !d.alive() {
                    continue;
                }
                let center = d.pos + Vec3::Z * d.height * 0.5;
                let dist = (at - center).truncate().length().max(0.0) - d.radius;
                let mut dmg = 0.0;
                if hit_target && Some(de) == m.target {
                    dmg += spec.direct;
                }
                if dist < spec.radius {
                    dmg += spec.splash * (1.0 - dist.max(0.0) / spec.radius);
                }
                if dmg > 0.0 {
                    hits.push(d.damage(dmg, false));
                }
            }
            for mut c in &mut cores {
                for h in &hits {
                    c.credit_inflicted(*h);
                }
            }
            audio::cue(&mut commands, Cue::MissileExplode, Some(at));
            let b = to_bevy(at);
            crate::particles::emit(&mut commands, crate::particles::Effect::Explosion { at: b, scale: 1.1 });
            commands.spawn((
                PointLight { color: Color::srgb(1.0, 0.6, 0.3), intensity: 400_000.0, range: 25.0, ..default() },
                Transform::from_translation(b),
                Fx { life: 0.3, max: 0.3, base: Vec3::ONE, keep_z: false },
            ));
            commands.entity(e).despawn();
            continue;
        }
        m.pos = next;
        tf.translation = next;
        // The rocket model points along its Z axis.
        tf.rotation = Quat::from_rotation_arc(Vec3::Z, m.vel.normalize());
        // Smoke trail puffs.
        if (m.age * 30.0) as u32 != ((m.age - dt) * 30.0) as u32 {
            crate::particles::emit(&mut commands, crate::particles::Effect::TrailPuff { at: to_bevy(m.pos), scale: 1.0 });
        }
    }
}
