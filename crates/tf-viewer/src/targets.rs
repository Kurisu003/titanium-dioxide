//! Enemy Titans (Ions), set up from the game's own data: AI settings
//! (scripts/aisettings/npc_titan_atlas_stickybomb.txt), Titan settings (titan_atlas_stickybomb.set)
//! and their Splitter Rifle (mp_titanweapon_particle_accelerator). They hold engagement range,
//! circle-strafe, dodge when hurt (within their dodge budget), punch when close, and fire bursts
//! of Splitter Rifle bolts. A fixed pool is spawned at load and the game director activates
//! them in waves.

use crate::abilities::TitanCore;
use crate::actor::{Actor, Aim, Layer};
use crate::combat::{TitanHealth, Vortex};
use crate::pilotctl::{Control, PilotHealth, PlayerPilot};
use crate::player::{to_bevy, Collision, MainCamera, PlayerTitan, TitanSettings};
use crate::vitals::{Difficulty, Hit, Vitals};
use crate::weapons::{Fx, FxAssets, WeaponDef};
use crate::audio::{self, Cue};
use bevy::prelude::*;
use tf_assets::settings::PlayerSettings;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::pilot::PilotMove;
use tf_sim::titan::{self, TitanInput, TitanParams, TitanState};

const DEATH_HIDE_SECS: f32 = 4.0;
/// TPAC_PROJECTILE_SPEED_NPC (mp_titanweapon_particle_accelerator.nut).
const BOLT_SPEED_NPC: f32 = 5000.0;
/// bolt_hitsize_growfinal_size.
const BOLT_HITSIZE: f32 = 8.0;
/// MFP_CHANCE_TO_HIT: an AI shooting at a fast-moving Pilot lands only this share of shots.
const FAST_PILOT_CHANCE_TO_HIT: f32 = 0.5;
/// Seconds of damage history the dodge decision looks at.
const DODGE_WINDOW: f32 = 1.0;

/// Everything the enemy Titans share, loaded from the game files at startup.
#[derive(Resource, Clone)]
pub struct EnemyKit {
    /// Class name shown over the Titan (ION, SCORCH, ...).
    pub name: String,
    pub model: String,
    /// Bolt speed (hitscan weapons get a very fast bolt), drop and bolt pattern.
    pub projectile: f32,
    pub gravity: f32,
    pub pellets: Option<(&'static [[f32; 2]], f32)>,
    /// Index into weapons::TITAN_ARSENAL for sounds.
    pub arsenal: usize,
    pub radius: f32,
    pub height: f32,
    pub ability: Option<Ability>,
    pub defense: Option<Defense>,
    /// A grunt rather than a Titan.
    pub infantry: bool,
    /// Per-shot NPC fire sound event (infantry; Titans use the cue table).
    pub fire_sound: String,
    pub vitals: Vitals,
    pub weapon: WeaponDef,
    pub params: TitanParams,
    /// The Titan .set the AI uses (npc_titan_player_settings).
    pub settings_path: String,
    pub chase_stop_dist: f32,
    pub circle_strafe_dist: f32,
    pub dodge_period: f32,
    pub max_dodge_per_period: u32,
    pub strafe_dodge_damage: f32,
    pub melee_range: f32,
    pub melee_damage_heavy: (f32, f32),
    pub melee_damage: (f32, f32),
    pub melee_interval: f32,
}

/// The enemy Titan classes: AI settings, model, the primary from the class loadout, name.
pub const ENEMY_CLASSES: &[(&str, &str, &str, &str)] = &[
    ("npc_titan_atlas_stickybomb", "models/titans/medium/titan_medium_ajax.mdl", "mp_titanweapon_particle_accelerator", "ION"),
    ("npc_titan_ogre_meteor", "models/titans/heavy/titan_heavy_ogre.mdl", "mp_titanweapon_meteor", "SCORCH"),
    ("npc_titan_stryder_sniper", "models/titans/light/titan_light_raptor.mdl", "mp_titanweapon_sniper", "NORTHSTAR"),
    ("npc_titan_stryder_leadwall", "models/titans/light/titan_light_locust.mdl", "mp_titanweapon_leadwall", "RONIN"),
    ("npc_titan_atlas_tracker", "models/titans/medium/titan_medium_wraith.mdl", "mp_titanweapon_sticky_40mm", "TONE"),
    ("npc_titan_ogre_minigun", "models/titans/heavy/titan_heavy_deadbolt.mdl", "mp_titanweapon_predator_cannon", "LEGION"),
];

/// Each class's MP ordnance, fired between bursts of the main weapon: (weapon script, bolt
/// speed, bolts per use, fan width in degrees, travels along the ground). Damage, range and the
/// rest between uses come from the weapon scripts; the speeds and Scorch's fan are
/// approximations of script-side behaviour (the Firewall's damage is its thermite's).
const ENEMY_ABILITIES: &[(&str, f32, u32, f32, bool)] = &[
    ("mp_titanweapon_laser_lite", 30000.0, 1, 0.0, false),
    ("mp_titanweapon_flame_wall", 1500.0, 7, 30.0, true),
    // FireClusterRocket's missileSpeed.
    ("mp_titanweapon_dumbfire_rockets", 3500.0, 1, 0.0, false),
    ("mp_titanweapon_arc_wave", 2000.0, 1, 0.0, true),
    // SmartAmmo_SetMissileSpeed.
    ("mp_titanweapon_tracker_rockets", 1800.0, 6, 12.0, false),
    ("mp_titanability_power_shot", 30000.0, 1, 0.0, false),
];

/// An enemy class's defensive ability (its MP defensive slot).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefenseKind {
    /// Ion's Vortex Shield: catches rounds from the front and throws them back.
    Vortex,
    /// Scorch's Thermal Shield: burns what's close in front, eats rounds.
    Heat,
    /// Ronin's Sword Block: 0.3 x damage from the front (TITAN_BLOCK_DAMAGE_REDUCTION).
    Block,
    /// Legion's Gun Shield: 2500 health in front (TITAN_GUN_SHIELD_HEALTH).
    GunShield,
    /// Tone's Particle Wall: a 1750-health pane dropped in front (titankit::ParticleWall).
    ParticleWall,
}

#[derive(Clone, Copy, Debug)]
pub struct Defense {
    pub kind: DefenseKind,
    /// Seconds it stays up (charge_time / fire_duration).
    pub hold: f32,
    /// Seconds before it can be used again (charge_cooldown_time + delay, or the ammo regen).
    pub cooldown: f32,
}

/// Per class (ENEMY_CLASSES order). The NPC decision to use them is script that isn't
/// shipped: here a Titan raises its defensive after DEFENSE_TRIGGER_DAMAGE of recent damage
/// from a target it sees in front (Scorch also when the target is within his shield's reach).
const ENEMY_DEFENSES: &[Option<Defense>] = &[
    Some(Defense { kind: DefenseKind::Vortex, hold: 3.0, cooldown: 4.0 }),
    Some(Defense { kind: DefenseKind::Heat, hold: 3.0, cooldown: 9.0 }),
    None,
    Some(Defense { kind: DefenseKind::Block, hold: 3.0, cooldown: 2.0 }),
    Some(Defense { kind: DefenseKind::ParticleWall, hold: 0.0, cooldown: 14.0 }),
    Some(Defense { kind: DefenseKind::GunShield, hold: 6.0, cooldown: 8.0 }),
];
const DEFENSE_TRIGGER_DAMAGE: f32 = 200.0;
/// VORTEX_BULLET_ABSORB_COUNT_MAX (_vortex.nut).
const VORTEX_ABSORB_MAX: u32 = 32;
/// mp_titanweapon_vortex_shield: returned rounds deal 35 to Pilots, 140 to Titans.
const VORTEX_RETURN_PILOT: f32 = 35.0;
const VORTEX_RETURN_TITAN: f32 = 140.0;
/// Thermal Shield (combat.rs has BT's): range, damage per tick to Titans/Pilots, ticks per second.
const HEAT_RANGE: f32 = 300.0;
const HEAT_TITAN: f32 = 200.0;
const HEAT_PILOT: f32 = 25.0;
const HEAT_RATE: f32 = 5.0;
const GUN_SHIELD_HEALTH: f32 = 2500.0;
const BLOCK_DAMAGE_REDUCTION: f32 = 0.3;

/// Typed-sound prefix for an enemy class.
fn enemy_sound_type(name: &str) -> Option<&'static str> {
    Some(match name {
        "ION" => "ion",
        "SCORCH" => "scorch",
        "NORTHSTAR" => "northstar",
        "RONIN" => "ronin",
        "TONE" => "tone",
        "LEGION" => "legion",
        _ => return None,
    })
}

/// (idle, move forward, move back, death) sequences of an enemy model.
pub fn enemy_clips(model: &str) -> [&'static str; 4] {
    if model.contains("/humans/") {
        ["CQB_Idle_MP", "Run_forward_mp", "Run_Backward_mp", "CQB_DeathMP"]
    } else {
        ["CQB_Idle_MP", "Walk_forward_MP", "Walk_backward_MP", "at_Death"]
    }
}

/// Bolts fired by an ability carry their class index plus this.
const ABILITY_BOLT: usize = 1000;

#[derive(Clone)]
pub struct Ability {
    pub weapon: WeaponDef,
    pub speed: f32,
    pub count: u32,
    pub fan: f32,
    pub ground: bool,
    /// fire_sound_1_player_3p (a sound event).
    pub sound: String,
}

/// Enemy infantry: (ai settings, model, weapon, name).
pub const INFANTRY: (&str, &str, &str, &str) = ("npc_soldier", "models/humans/grunts/imc_grunt_rifle.mdl", "mp_weapon_rspn101", "GRUNT");

/// Every enemy class's kit (index = class).
#[derive(Resource, Clone)]
pub struct EnemyKits(pub Vec<EnemyKit>);

/// The (standing, moving) aim-matrix sequences of a Titan model's chassis: the heavy chassis
/// names them differently from the light and medium ones.
pub fn aim_grids(model: &str) -> [&'static str; 2] {
    if model.contains("/humans/") {
        return ["Aim_static", "Aim_move_MP"];
    }
    if model.contains("/heavy/") { ["Aim_Stand_MP", "Aim_run_MP"] } else { ["MP_stand_Aim_all", "Aim_combat_walk_MP"] }
}

impl EnemyKit {
    pub fn class(index: usize, read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let (ai, model, weapon, name) = ENEMY_CLASSES[index];
        let mut k = Self::load(ai, read);
        k.weapon = WeaponDef::load(weapon, read);
        k.model = model.to_string();
        k.name = name.to_string();
        let arsenal = crate::weapons::TITAN_ARSENAL.iter().position(|g| g.id == weapon).unwrap_or(0);
        let gun = &crate::weapons::TITAN_ARSENAL[arsenal];
        k.arsenal = arsenal;
        // The Splitter Rifle's NPC bolts are slower than the player's (TPAC_PROJECTILE_SPEED_NPC).
        k.projectile = if weapon == "mp_titanweapon_particle_accelerator" { BOLT_SPEED_NPC } else { gun.projectile.unwrap_or(30000.0) };
        k.gravity = gun.gravity;
        k.pellets = gun.pellets;
        k.defense = ENEMY_DEFENSES.get(index).copied().flatten();
        k.ability = ENEMY_ABILITIES.get(index).map(|&(id, speed, count, fan, ground)| {
            let mut weapon = WeaponDef::load(id, read);
            // The Firewall does its damage through thermite, not the projectile.
            if weapon.npc_damage_near <= 0.0 && weapon.npc_explosion_damage_heavy <= 0.0 {
                weapon.npc_damage_near = 0.0;
                weapon.npc_damage_far = 0.0;
                weapon.npc_explosion_damage_heavy = k.weapon.npc_explosion_damage_heavy;
                weapon.explosion_radius = weapon.explosion_radius.max(k.weapon.explosion_radius);
                weapon.explosion_inner_radius = k.weapon.explosion_inner_radius;
            }
            let sound = PlayerSettings::load(&format!("scripts/weapons/{id}.txt"), true, read)
                .and_then(|s| s.get(".fire_sound_1_player_3p").map(str::to_string))
                .unwrap_or_default();
            Ability { weapon, speed, count, fan, ground, sound }
        });
        (k.radius, k.height) = if model.contains("heavy") { (80.0, 250.0) } else if model.contains("light") { (60.0, 225.0) } else { (70.0, 235.0) };
        k
    }

    /// An IMC grunt (`npc_soldier`) with the R-201, from the SP AI settings.
    pub fn infantry(read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let (ai, model, weapon, name) = INFANTRY;
        let mut k = Self::load(ai, read);
        let a = PlayerSettings::load(&format!("scripts/aisettings/{ai}.txt"), true, read).unwrap_or_default();
        k.weapon = WeaponDef::load(weapon, read);
        let w = PlayerSettings::load(&format!("scripts/weapons/{weapon}.txt"), true, read).unwrap_or_default();
        k.fire_sound = w.get(".fire_sound_2_npc").or_else(|| w.get(".fire_sound_1_npc")).unwrap_or_default().to_string();
        k.model = model.to_string();
        k.name = name.to_string();
        k.infantry = true;
        k.projectile = 30000.0;
        k.arsenal = crate::pilotweapon::arsenal_index(weapon).unwrap_or(0);
        (k.radius, k.height) = (16.0, 72.0);
        k.vitals = Vitals::from_settings(&PlayerSettings::default(), Some(a.f32(".health", 90.0)));
        // Human movement (run speed of the grunt's Run_forward animation, no dashes).
        k.params = TitanParams {
            speed: 190.0,
            accel: 900.0,
            decel: 1200.0,
            sprint_speed: 260.0,
            dash_speed: 0.0,
            dash_drain: 1e9,
            step_height: 18.0,
            radius: 16.0,
            height: 72.0,
            eye_height: 60.0,
            ..TitanParams::default()
        };
        k.chase_stop_dist = 700.0;
        k.circle_strafe_dist = 500.0;
        k.max_dodge_per_period = 0;
        // Grunts don't punch Titans.
        k.melee_damage_heavy = (0.0, 0.0);
        k
    }

    pub fn load(ai: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let a = PlayerSettings::load(&format!("scripts/aisettings/{ai}.txt"), true, read).unwrap_or_default();
        let f = |k: &str, d: f32| a.f32(&format!(".{k}"), d);
        let set = a.get(".npc_titan_player_settings").unwrap_or("titan_atlas").to_string();
        let settings_path = format!("scripts/players/mp/{set}.set");
        let ts = PlayerSettings::load(&settings_path, true, read).unwrap_or_default();
        let weapon_name = a.get(".defaultweapon").unwrap_or("mp_titanweapon_particle_accelerator").to_string();
        let ai_health = a.get(".health").and_then(|v| v.parse().ok());
        Self {
            name: "ION".into(),
            model: String::new(),
            projectile: BOLT_SPEED_NPC,
            gravity: 0.0,
            pellets: None,
            arsenal: 0,
            radius: 70.0,
            height: 235.0,
            ability: None,
            defense: None,
            infantry: false,
            fire_sound: String::new(),
            vitals: Vitals::from_settings(&ts, ai_health),
            weapon: WeaponDef::load(&weapon_name, read),
            params: TitanParams::default(),
            settings_path,
            chase_stop_dist: f("chasestopdistheavyarmor", 1000.0),
            circle_strafe_dist: f("circlestrafedist", 1000.0),
            dodge_period: f("dodgeperiod", 8.0),
            max_dodge_per_period: f("maxdodgeperperiod", 2.0) as u32,
            strafe_dodge_damage: f("strafedodgedamage", 800.0),
            melee_range: f("meleerange", 200.0),
            melee_damage_heavy: (f("meleedamageminheavyarmor", 500.0), f("meleedamagemaxheavyarmor", 600.0)),
            melee_damage: (f("meleedamagemin", 150.0), f("meleedamagemax", 300.0)),
            melee_interval: f("meleeinterval", 2.0),
        }
    }
}

#[derive(Component)]
pub struct Enemy {
    pub aim_grids: [&'static str; 2],
    /// Titanfall in progress: seconds into the drop (negative = still waiting to drop), then
    /// into the quickstand.
    drop: Option<f32>,
    /// Played by a synced execution (executions.rs): no AI or locomotion while held, and no
    /// death sequence afterwards (the victim sequence already ends the body).
    pub held: bool,
    pub executed: bool,
    stand: Option<f32>,
    landed: bool,
    /// Prefix for the model's typed sound events ("ion", "scorch", ...).
    pub sound_type: Option<&'static str>,
    pub clips: [&'static str; 4],
    pub infantry: bool,
    /// Class (index into EnemyKits).
    pub kit: usize,
    /// Feet position, game units (mirrors `state.pos`).
    pub pos: Vec3,
    pub radius: f32,
    pub height: f32,
    pub v: Vitals,
    pub actor: Entity,
    /// Active in the current wave.
    pub active: bool,
    pub dead_for: f32,
    pub bar: Option<Entity>,
    pub flash: f32,
    pub state: TitanState,
    /// Seconds since last taking damage.
    pub since_hit: f32,
    /// Recent damage taken (age, amount), for dodging.
    recent: Vec<(f32, f32)>,
    dodges: Vec<f32>,
    strafe: f32,
    strafe_timer: f32,
    /// Seconds spent wanting to move but barely moving (a wall or ledge in the way).
    stuck: f32,
    /// Navmesh route to `path_goal` (next waypoint first) and its age.
    path: Vec<Vec3>,
    path_goal: Option<Vec3>,
    /// Seconds left of waypoint-by-waypoint following after a collision.
    no_skip: f32,
    path_age: f32,
    /// Shots left in the current burst; rest time before the next one.
    burst_left: u32,
    rest: f32,
    /// Time until the class ability can be used again.
    ability_cd: f32,
    shot_timer: f32,
    melee_cooldown: f32,
    rng: u64,
    move_cycle: f32,
    idle_cycle: f32,
    /// Seconds without line of sight to the target, and with it (aim focus).
    blind: f32,
    /// Movement speed multiplier while `slow_t` runs (Slow Trap gas).
    pub slow: f32,
    pub slow_t: f32,
    /// The Titan still has its battery (rodeo rips it out).
    pub battery: bool,
    /// Electric Smoke charges left for a rider on its back (anti-rodeo).
    pub smoke_charges: u32,
    /// The class defensive, while it's up: seconds left, then its cooldown.
    defense: Option<Defense>,
    defense_left: f32,
    defense_cd: f32,
    /// Gun Shield health left; rounds the Vortex caught and their damage.
    shield_hp: f32,
    caught: u32,
    burn_timer: f32,
    /// Where this frame's target is (shots are taken to come from there).
    threat: Vec3,
    defense_fx: Option<Entity>,
    seen: f32,
    /// Flanking point to reach while the target is out of sight, and when to re-plan.
    goal: Option<Vec3>,
    replan: f32,
    /// Killed this frame (for scoring); cleared by the game director.
    pub just_died: bool,
}

impl Enemy {
    pub fn new(pos: Vec3, actor: Entity, seed: u64, kit_index: usize, kit: &EnemyKit) -> Self {
        let mut v = kit.vitals.clone();
        v.health = 0.0;
        Self {
            kit: kit_index,
            pos,
            radius: kit.radius,
            height: kit.height,
            aim_grids: aim_grids(&kit.model),
            clips: enemy_clips(&kit.model),
            drop: None,
            held: false,
            executed: false,
            stand: None,
            landed: false,
            sound_type: enemy_sound_type(&kit.name),
            infantry: kit.infantry,
            v,
            actor,
            active: false,
            dead_for: 99.0,
            bar: None,
            flash: 0.0,
            state: TitanState::new(SVec3::new(pos.x, pos.y, pos.z), 0.0),
            since_hit: 99.0,
            recent: Vec::new(),
            dodges: Vec::new(),
            strafe: 1.0,
            strafe_timer: 0.0,
            stuck: 0.0,
            path: Vec::new(),
            path_goal: None,
            path_age: 0.0,
            no_skip: 0.0,
            burst_left: 0,
            rest: 1.0,
            ability_cd: 4.0 + (seed % 5) as f32,
            shot_timer: 0.0,
            melee_cooldown: 0.0,
            rng: seed | 1,
            move_cycle: 0.0,
            idle_cycle: 0.0,
            blind: 0.0,
            slow: 1.0,
            slow_t: 0.0,
            battery: true,
            smoke_charges: if kit.infantry { 0 } else { 1 },
            defense: kit.defense,
            defense_left: 0.0,
            defense_cd: 0.0,
            shield_hp: 0.0,
            caught: 0,
            burn_timer: 0.0,
            threat: Vec3::ZERO,
            defense_fx: None,
            seen: 0.0,
            goal: None,
            replan: 0.0,
            just_died: false,
        }
    }
    pub fn alive(&self) -> bool {
        self.active && self.v.alive()
    }
    /// Stop shooting for `secs` (Arc Grenade, Gravity Star).
    pub fn stun(&mut self, secs: f32) {
        self.burst_left = 0;
        self.rest = self.rest.max(secs);
        self.ability_cd = self.ability_cd.max(secs);
    }
    pub fn doomed(&self) -> bool {
        self.v.doomed.is_some()
    }
    pub fn damage(&mut self, amount: f32, stops_regen: bool) -> Hit {
        if !self.alive() {
            return Hit::default();
        }
        // The defensive up: shots from the target's side are caught, eaten, blocked or
        // soaked by the Gun Shield (Vortex: VORTEX_BULLET_FOV 120; Block: TITAN_BLOCK_ANGLE 150).
        let mut amount = amount;
        if let Some(d) = self.defense.filter(|_| self.defense_left > 0.0) {
            let to = (self.threat - self.pos).truncate().normalize_or_zero();
            let facing = Vec2::new(self.state.yaw.cos(), self.state.yaw.sin());
            let cone = if d.kind == DefenseKind::Block { 75.0f32 } else { 60.0 };
            if to.dot(facing) >= cone.to_radians().cos() {
                self.flash = 0.05;
                match d.kind {
                    DefenseKind::Vortex => {
                        self.caught = (self.caught + 1).min(VORTEX_ABSORB_MAX);
                        self.since_hit = 0.0;
                        return Hit::default();
                    }
                    DefenseKind::Heat => return Hit::default(),
                    DefenseKind::Block => amount *= BLOCK_DAMAGE_REDUCTION,
                    DefenseKind::ParticleWall => {}
                    DefenseKind::GunShield => {
                        self.shield_hp -= amount;
                        if self.shield_hp <= 0.0 {
                            self.defense_left = 0.0;
                            log::info!("enemy gun shield broken");
                        }
                        return Hit::default();
                    }
                }
            }
        }
        self.damage_unblockable(amount, stops_regen)
    }
    /// Damage no defensive stops (melee, executions: `shouldPassThroughDamage`).
    pub fn damage_unblockable(&mut self, amount: f32, stops_regen: bool) -> Hit {
        if !self.alive() {
            return Hit::default();
        }
        let hit = self.v.damage(amount, stops_regen);
        if hit.dealt > 0.0 {
            self.flash = 0.1;
            self.since_hit = 0.0;
            self.recent.push((0.0, hit.dealt));
        }
        if hit.doomed_now {
            log::info!("enemy titan doomed");
        }
        if hit.killed {
            self.dead_for = 0.0;
            self.just_died = true;
            log::info!("enemy titan destroyed");
        }
        hit
    }
    /// Damage with DF_BYPASS_SHIELD | DF_SKIPS_DOOMED_STATE (a Titanfall landing on it).
    pub fn damage_bypass(&mut self, amount: f32) {
        if self.alive() {
            self.flash = 0.1;
            if self.v.health - amount <= 1.0 {
                self.kill();
            } else {
                self.v.health -= amount;
            }
        }
    }
    /// Instant kill.
    pub fn kill(&mut self) {
        if self.alive() {
            self.v.health = 0.0;
            self.dead_for = 0.0;
            self.just_died = true;
            log::info!("enemy titan destroyed");
        }
    }
    /// Bring this Titan into the fight at `pos`, facing `yaw`.
    pub fn activate(&mut self, pos: Vec3, yaw: f32, kit: &EnemyKit) {
        self.active = true;
        self.radius = kit.radius;
        self.height = kit.height;
        self.v = kit.vitals.clone();
        self.battery = true;
        self.dead_for = 0.0;
        self.state = TitanState::new(SVec3::new(pos.x, pos.y, pos.z), yaw);
        self.pos = pos;
        self.burst_left = 0;
        self.rest = 1.5;
        self.seen = 0.0;
        self.recent.clear();
        self.dodges.clear();
        // Titans arrive by Titanfall, a little staggered; grunts are simply there.
        self.drop = if kit.infantry { None } else { Some(-self.rand() * 1.5) };
        self.stand = None;
        self.landed = false;
        self.held = false;
        self.executed = false;
    }
    /// Still dropping in by Titanfall or standing up from it.
    pub fn arriving(&self) -> bool {
        self.drop.is_some() || self.stand.is_some()
    }
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Overhead target info: name and locks, shield bar, health bar (entities of its parts).
#[derive(Component)]
pub struct HealthBar {
    enemy: Entity,
    label: Entity,
    shield: Entity,
    health: Entity,
}

/// A Splitter Rifle bolt in flight (game units).
#[derive(Component)]
pub struct Bolt {
    pos: Vec3,
    vel: Vec3,
    travelled: f32,
    gravity: f32,
    /// Firing class (index into EnemyKits) for damage values.
    kit: usize,
    /// Fixed (Pilot, Titan) damage instead of the class weapon's (Vortex returns).
    dmg: Option<(f32, f32)>,
}

/// The player's current body for targeting: (position, radius, height, is_pilot).
fn player_target(control: Control, titan: &PlayerTitan, titan_alive: bool, pilot: &PlayerPilot, settings: &TitanSettings) -> Option<(Vec3, f32, f32, bool)> {
    match control {
        Control::Pilot => Some((Vec3::from(pilot.state.pos.to_array()), 30.0, 72.0, true)),
        _ if titan_alive => Some((Vec3::from(titan.state.pos.to_array()), settings.0.radius, settings.0.height, false)),
        _ => None,
    }
}

impl Enemy {
    /// The direction to head for `goal` along the navmesh (the Titan mesh, or the infantry one
    /// for grunts): replans when the goal moves or the route ages, drops reached waypoints, and
    /// skips ahead to the furthest of the next few waypoints in a straight clear line. None
    /// without a mesh or route.
    fn route(&mut self, world: &Collision, goal: Vec3, dt: f32) -> Option<Vec2> {
        let nav = if self.infantry { world.2.as_ref()? } else { world.1.as_ref()? };
        let (eye_h, reach) = if self.infantry { (50.0, 60.0) } else { (150.0, 120.0) };
        self.path_age += dt;
        let moved = self.path_goal.is_none_or(|g| g.truncate().distance(goal.truncate()) > 200.0);
        // A failed search is not retried for a second (it is the expensive case).
        let retry = self.path.is_empty() && (self.path_goal.is_none() || self.path_age > 1.0);
        if retry || moved || self.path_age > 2.0 {
            self.path_age = 0.0;
            self.path_goal = Some(goal);
            match nav.path(self.pos, goal) {
                Some(p) => {
                    log::debug!("route ({}): {} waypoints from {:.0} to {:.0}: {:?}", if self.infantry { "grunt" } else { "titan" }, p.len(), self.pos, goal, p.iter().take(6).map(|w| w.round()).collect::<Vec<_>>());
                    self.path = p;
                }
                None => {
                    log::debug!("route ({}): none from {:.0} to {:.0}", if self.infantry { "grunt" } else { "titan" }, self.pos, goal);
                    self.path.clear();
                    return None;
                }
            }
        }
        if self.path.is_empty() {
            return None;
        }
        // Reached waypoints go; then look ahead for a straight shot.
        while self.path.len() > 1 && self.path[0].truncate().distance(self.pos.truncate()) < reach {
            self.path.remove(0);
        }
        // A waypoint on another level is never a straight shot, however clear the view: from a
        // roof the whole route below is visible, and walking at it means walking into the
        // parapet.
        let eye = SVec3::new(self.pos.x, self.pos.y, self.pos.z + eye_h);
        let knee = SVec3::new(self.pos.x, self.pos.y, self.pos.z + if self.infantry { 30.0 } else { 90.0 });
        let clear = |to: Vec3| {
            let d = SVec3::new(to.x - self.pos.x, to.y - self.pos.y, 0.0);
            let len = d.length();
            (to.z - self.pos.z).abs() <= reach && (len < 1.0 || (world.0.raycast(eye, d / len, len).is_none() && world.0.raycast(knee, d / len, len).is_none()))
        };
        // After walking into something, follow the waypoints one by one for a while.
        self.no_skip = (self.no_skip - dt).max(0.0);
        let look = if self.no_skip > 0.0 { 1 } else { self.path.len().min(4) };
        if let Some(k) = (1..look).rev().find(|&k| clear(self.path[k])) {
            self.path.drain(..k);
        }
        let next = *self.path.first()?;
        log::trace!("route: at {:.0} heading for {:.0} ({} left)", self.pos, next, self.path.len());
        Some((next - self.pos).truncate().normalize_or_zero())
    }
}

/// Where a defensive's effect is played: P_titan_gun_shield_3P offsets its hex panel 80 units
/// down its control point's Z, so that one is played 80 above the shield spot.
fn defense_fx_spot(kind: DefenseKind, shield_spot: Vec3) -> Vec3 {
    match kind {
        DefenseKind::GunShield => shield_spot + Vec3::Z * 80.0,
        _ => shield_spot,
    }
}

/// IsFastPilot (_utility.gnut): wall-running, airborne, or faster than 180 u/s.
fn fast_pilot(p: &PlayerPilot) -> bool {
    let s = &p.state;
    matches!(s.mode, PilotMove::WallRun { .. }) || !s.on_ground() || s.vel.length() > 180.0
}

/// AI: movement, aiming and firing. Runs after the player's movement.
#[allow(clippy::too_many_arguments)]
pub fn update_enemies(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    difficulty: Res<Difficulty>,
    kits: Res<EnemyKits>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    fx: Res<FxAssets>,
    mut enemies: Query<(Entity, &mut Enemy, &mut Transform, &mut Visibility)>,
    mut player: Query<(&PlayerTitan, &mut TitanHealth, Option<&mut TitanCore>)>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    mut actors: Query<&mut Actor>,
    ui: Res<crate::ui::UiAssets>,
    mut damage_from: ResMut<crate::hud::DamageFrom>,
    status: Res<crate::pilotability::PilotStatus>,
    rodeo: Res<crate::rodeo::Rodeo>,
) {
    let dt = time.delta_secs().min(0.1);
    let Ok((titan, mut bt_health, mut core)) = player.single_mut() else { return };
    let Ok((pilot, mut pilot_health)) = pilots.single_mut() else { return };
    let target = player_target(*control, titan, bt_health.dead_for.is_none(), pilot, &settings);
    // On foot, BT fights too (autotitan.rs): an enemy nearer to him than to you takes him on.
    let bt_target = (*control == Control::Pilot && bt_health.dead_for.is_none()).then(|| (Vec3::from(titan.state.pos.to_array()), settings.0.radius, settings.0.height, false));
    let positions: Vec<(Entity, Vec3, bool)> = enemies.iter().filter(|(_, e, _, _)| e.alive()).map(|(id, e, _, _)| (id, e.pos, e.infantry)).collect();
    // Enemy Titans get worse while the player's Titan is doomed (OnTitanDoomed).
    let mut proficiency = difficulty.titan_proficiency();
    if *control != Control::Pilot && bt_health.v.doomed.is_some() {
        proficiency = proficiency.lowered();
    }
    let (cone0, focus_time) = difficulty.aim_cone_focus();

    for (id, mut e, mut tf, mut vis) in &mut enemies {
        let kit = &kits.0[e.kit.min(kits.0.len() - 1)];
        let params = &kit.params;
        let w = &kit.weapon;
        e.flash = (e.flash - dt).max(0.0);
        e.since_hit += dt;
        e.melee_cooldown -= dt;
        for r in &mut e.recent {
            r.0 += dt;
        }
        e.recent.retain(|r| r.0 < DODGE_WINDOW);
        e.defense_cd -= dt;
        if let Some(t) = target {
            e.threat = t.0;
        }
        for d in &mut e.dodges {
            *d += dt;
        }
        let period = kit.dodge_period;
        e.dodges.retain(|d| *d < period);
        e.v.tick(dt);
        ensure_health_bar(&mut commands, &ui, id, &mut e);
        if !e.active {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        // BT crushes grunts he walks into (on his own too).
        if e.infantry && e.alive() && bt_health.dead_for.is_none() {
            let tp = Vec3::from(titan.state.pos.to_array());
            let moving = Vec2::new(titan.state.vel.x, titan.state.vel.y).length() > 40.0;
            if moving && (tp - e.pos).truncate().length() < settings.0.radius + e.radius + 10.0 && (e.pos.z - tp.z).abs() < 100.0 {
                e.damage(10_000.0, false);
            }
        }
        if !e.alive() {
            e.dead_for += dt;
            if e.dead_for > DEATH_HIDE_SECS {
                e.active = false;
            }
            if !e.executed {
                animate_death(&mut actors, &e);
            }
            continue;
        }
        if e.held {
            continue;
        }

        // --- Titanfall: ride the hotdrop's root motion down, impact, quickstand ---
        if e.drop.is_some() || e.stand.is_some() {
            enemy_titanfall(&mut commands, &mut actors, &mut e, &mut tf, &mut vis, dt);
            continue;
        }
        let base = match (target, bt_target) {
            (Some(p), Some(b)) if (e.pos - b.0).length() < (e.pos - p.0).length() => Some(b),
            _ => target,
        };
        // The Holo Pilot draws fire.
        let (mut target, at_decoy) = crate::pilotability::enemy_target(base, &status, e.pos);
        // A Titan can't shoot or punch the Pilot on its own back (it would need its smoke).
        if rodeo.ride.as_ref().is_some_and(|r| r.target == id) {
            target = None;
        }

        // --- Decide movement ---
        let mut input = TitanInput { yaw: e.state.yaw, ..Default::default() };
        if let Some((tpos, _, _, _)) = target {
            let to = tpos - e.pos;
            let dist = to.truncate().length();
            let want_yaw = to.y.atan2(to.x);
            let diff = (want_yaw - e.state.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            input.yaw = e.state.yaw + diff.clamp(-2.0 * dt, 2.0 * dt);
            input.pitch = -(to.z + 60.0).atan2(dist.max(1.0));
            e.strafe_timer -= dt;
            if e.strafe_timer <= 0.0 {
                e.strafe = if e.rand() < 0.5 { -1.0 } else { 1.0 };
                e.strafe_timer = 2.0 + e.rand() * 3.0;
            }
            // In sight: close to chase-stop distance, back off inside the minimum engagement
            // range, circle-strafe in between. Out of sight: head for a flanking point.
            let mut want = Vec2::ZERO;
            if e.blind > 1.5 {
                e.replan -= dt;
                if e.goal.is_none() || e.replan <= 0.0 {
                    e.goal = flank_point(&world, e.pos, tpos);
                    e.replan = 2.5;
                }
                if let Some(g) = e.goal {
                    want = e.route(&world, g, dt).unwrap_or_else(|| (g - e.pos).truncate().normalize_or_zero());
                }
            } else {
                e.goal = None;
                let mut towards = to.truncate().normalize_or_zero();
                let stop = kit.chase_stop_dist.max(kit.circle_strafe_dist);
                let radial = if dist > stop * 1.5 { 1.0 } else if dist < w.npc_min_engage_titan.max(kit.melee_range * 2.0) { -1.0 } else { 0.0 };
                // Closing in follows the navmesh (ledges, buildings); the rest steers directly.
                if radial > 0.0 {
                    if let Some(d) = e.route(&world, tpos, dt) {
                        towards = d;
                    }
                } else {
                    e.path.clear();
                }
                let side = Vec2::new(towards.y, -towards.x) * e.strafe;
                // Titans circle-strafe; grunts hold their spot and shoot (no cover use yet).
                let circle = if dist < stop * 2.0 && !e.infantry { 0.8 } else { 0.0 };
                want = towards * radial + side * circle;
            }
            if want != Vec2::ZERO {
                let dir = steer(&world, e.pos, want);
                let f = Vec2::new(input.yaw.cos(), input.yaw.sin());
                let r = Vec2::new(input.yaw.sin(), -input.yaw.cos());
                input.forward = dir.dot(f);
                input.right = dir.dot(r);
                // Walking into something: strafe the other way instead of standing there.
                let speed = Vec2::new(e.state.vel.x, e.state.vel.y).length();
                e.stuck = if speed < 40.0 && e.state.on_ground && !e.infantry { e.stuck + dt } else { 0.0 };
                if e.stuck > 0.6 {
                    e.stuck = 0.0;
                    e.strafe = -e.strafe;
                    e.strafe_timer = 2.0 + e.rand() * 3.0;
                    log::debug!("{}: blocked at {:.0} heading {:?}, strafing the other way", kit.name, e.pos, e.path.first().map(|p| p.round()));
                    if log::log_enabled!(log::Level::Trace) {
                        // What is in the way: hit distances towards the heading at several heights.
                        let d3 = Vec3::new(dir.x, dir.y, 0.0);
                        let probes: Vec<(f32, Option<i32>)> = [10.0, 40.0, 70.0, 100.0, 150.0, 200.0]
                            .iter()
                            .map(|&h| (h, world.0.raycast(SVec3::from((e.pos + Vec3::Z * h).to_array()), SVec3::from(d3.to_array()), 300.0).map(|hit| hit.t as i32)))
                            .collect();
                        let ground = world.0.raycast(SVec3::from((e.pos + d3 * 100.0 + Vec3::Z * 100.0).to_array()), -SVec3::Z, 600.0).map(|h| h.point.z as i32);
                        let side = Vec3::new(-dir.y, dir.x, 0.0);
                        let sides: Vec<(f32, Option<i32>, Option<i32>)> = [40.0, 100.0, 150.0]
                            .iter()
                            .map(|&h| {
                                let o = SVec3::from((e.pos + Vec3::Z * h).to_array());
                                (h, world.0.raycast(o, SVec3::from(side.to_array()), 200.0).map(|hit| hit.t as i32), world.0.raycast(o, SVec3::from((-side).to_array()), 200.0).map(|hit| hit.t as i32))
                            })
                            .collect();
                        let up = world.0.raycast(SVec3::from((e.pos + Vec3::Z * 5.0).to_array()), SVec3::Z, 400.0).map(|hit| hit.t as i32);
                        let (lo, hi) = (params.step_height + params.radius, (params.height - params.radius).max(params.step_height + params.radius));
                        let pushes: Vec<(f32, SVec3)> = [lo, (lo + hi) * 0.5, hi].iter().map(|&z| (z, world.0.push_sphere(SVec3::from((e.pos + Vec3::Z * z).to_array()), params.radius, 0.7).0)).collect();
                        log::trace!("{}: wanted {:?}; hits by height {:?}; ground 100 ahead at z {:?}; left/right hits {:?}; up hit {:?}; body pushes {:?}; vel {:?} on_ground {} input fwd {:.2} right {:.2} yaw {:.2}", kit.name, dir, probes, ground, sides, up, pushes, e.state.vel, e.state.on_ground, input.forward, input.right, input.yaw);
                    }
                    // Replan from here (once; a failed search waits a second) and stop cutting
                    // corners for a while.
                    if e.no_skip <= 0.0 {
                        e.path.clear();
                        e.path_goal = None;
                    }
                    e.no_skip = 3.0;
                }
            } else {
                e.stuck = 0.0;
            }
            // Dodge when the recent damage passes StrafeDodgeDamage, within the dodge budget.
            let recent: f32 = e.recent.iter().map(|r| r.1).sum();
            if recent >= kit.strafe_dodge_damage && (e.dodges.len() as u32) < kit.max_dodge_per_period {
                input.dash = true;
                e.dodges.push(0.0);
                e.recent.clear();
            }
        }
        // Keep apart from others of the same kind on the same level (a Titan is not steered
        // by grunts, and nothing below a deck pushes what stands on it).
        for &(other, p, infantry) in &positions {
            if other != id && infantry == e.infantry && (p.z - e.pos.z).abs() < 150.0 {
                let away = (e.pos - p).truncate();
                if away.length() < if e.infantry { 80.0 } else { 300.0 } {
                    let side = away.normalize_or(Vec2::X);
                    let f = Vec2::new(input.yaw.cos(), input.yaw.sin());
                    let r = Vec2::new(input.yaw.sin(), -input.yaw.cos());
                    input.forward += side.dot(f);
                    input.right += side.dot(r);
                }
            }
        }
        e.slow_t = (e.slow_t - dt).max(0.0);
        input.speed_scale = if e.slow_t > 0.0 { e.slow } else { 1.0 };
        // Fixed 120 Hz substeps, like the player's: a long frame (a shader compile when the
        // first effects play) must not fling an NPC over a ledge it would otherwise walk down.
        let n = (dt * 120.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let mut step_input = input;
            step_input.dash = input.dash && i == 0;
            titan::step(&mut e.state, &step_input, params, &world.0, dt / n as f32);
        }
        e.pos = Vec3::from(e.state.pos.to_array());
        tf.translation = e.pos;
        tf.rotation = Quat::from_rotation_z(e.state.yaw);

        animate_enemy(&mut actors, &mut e, dt, target.map(|t| t.0));

        // --- Melee (MeleeRange, meleeInterval, MeleeDamage*) ---
        let Some((tpos, tr, th, is_pilot)) = target else { continue };
        let reach = (tpos - e.pos).truncate().length() - tr - e.radius;
        if reach < kit.melee_range && e.melee_cooldown <= 0.0 && (tpos.z - e.pos.z).abs() < e.height && !(is_pilot && status.phased()) && !at_decoy {
            e.melee_cooldown = kit.melee_interval;
            damage_from.push(e.pos + Vec3::Z * 150.0);
            let (lo, hi) = if is_pilot { kit.melee_damage } else { kit.melee_damage_heavy };
            let dmg = (lo + (hi - lo) * e.rand()) * difficulty.damage_to_player();
            if is_pilot {
                pilot_health.damage_capped(dmg, difficulty.max_pilot_damage_per_hit());
            } else {
                let hit = bt_health.damage(dmg, false);
                if let Some(c) = core.as_mut() {
                    c.credit_received(hit.dealt);
                }
            }
            log::info!("enemy titan punched for {dmg:.0}");
            continue;
        }

        // --- Fire ---
        let aim_at = tpos + Vec3::Z * th * 0.6;
        let yaw = e.state.yaw;
        let muzzle = if e.infantry {
            e.pos + Vec3::Z * 52.0 + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 25.0
        } else {
            e.pos + Vec3::Z * 190.0 + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 110.0
        };
        let dist = (aim_at - muzzle).length();
        let facing = (aim_at - muzzle).truncate().normalize_or_zero().dot(Vec2::new(yaw.cos(), yaw.sin())) > 0.95;
        // Cloak and Phase Shift hide the Pilot.
        let hidden = is_pilot && !at_decoy && !status.visible_at(dist);
        let los = !hidden
            && world
                .0
                .raycast(SVec3::from(muzzle.to_array()), SVec3::from((aim_at - muzzle).normalize().to_array()), dist - tr)
                .is_none();
        if los {
            e.blind = 0.0;
            e.seen += dt;
        } else {
            e.blind += dt;
            e.seen = 0.0;
        }
        // --- Defensive ---
        let scale = difficulty.damage_to_player();
        let shield_spot = e.pos + Vec3::Z * 150.0 + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 130.0;
        if let Some(d) = kit.defense {
            if e.defense_left > 0.0 {
                e.defense_left -= dt;
                if let Some(fx_e) = e.defense_fx {
                    let facing = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
                    commands.entity(fx_e).insert(Transform::from_translation(to_bevy(defense_fx_spot(d.kind, shield_spot))).looking_to(to_bevy(facing).normalize(), Vec3::Y));
                }
                if d.kind == DefenseKind::Heat {
                    // Burns everything close in front at HEAT_RATE.
                    e.burn_timer -= dt;
                    if e.burn_timer <= 0.0 {
                        e.burn_timer += 1.0 / HEAT_RATE;
                        let to = aim_at - muzzle;
                        let close = to.truncate().length() - tr < HEAT_RANGE && to.truncate().normalize_or_zero().dot(Vec2::new(yaw.cos(), yaw.sin())) > 60f32.to_radians().cos();
                        if close && !(is_pilot && status.phased()) {
                            damage_from.push(e.pos + Vec3::Z * 150.0);
                            if is_pilot {
                                pilot_health.damage_capped(HEAT_PILOT * scale, difficulty.max_pilot_damage_per_hit());
                            } else {
                                let hit = bt_health.damage_from(HEAT_TITAN * scale, true, e.pos);
                                if let Some(c) = core.as_mut() {
                                    c.credit_received(hit.dealt);
                                }
                            }
                        }
                    }
                }
                // Down: after its hold, or a Vortex once nothing has hit it for a second.
                let drop_vortex = d.kind == DefenseKind::Vortex && e.since_hit > 1.0 && e.caught > 0;
                if e.defense_left <= 0.0 || drop_vortex {
                    e.defense_left = 0.0;
                    e.defense_cd = d.cooldown;
                    if let Some(fx_e) = e.defense_fx.take() {
                        commands.entity(fx_e).despawn();
                    }
                    if d.kind == DefenseKind::Vortex {
                        // Throw every caught round back along the aim (35 / 140 each).
                        let n = e.caught;
                        e.caught = 0;
                        crate::audio::event_at(&mut commands, if n > 0 { "vortex_1p_shield_throw" } else { "vortex_3p_shield_end_1" }, muzzle);
                        let dir0 = (aim_at - muzzle).normalize_or(Vec3::X);
                        for _ in 0..n {
                            let jitter = Vec3::new(e.rand() - 0.5, e.rand() - 0.5, e.rand() - 0.5) * 0.08;
                            let dir = (dir0 + jitter).normalize();
                            spawn_bolt_dmg(&mut commands, &fx, muzzle + dir0 * 60.0, dir * BOLT_SPEED_NPC, 0.0, e.kit, Some((VORTEX_RETURN_PILOT, VORTEX_RETURN_TITAN)));
                        }
                        log::info!("enemy vortex threw back {n} rounds");
                    }
                }
            } else if e.defense_cd <= 0.0 && los && facing && !e.infantry {
                let recent: f32 = e.recent.iter().map(|r| r.1).sum();
                let reach = d.kind == DefenseKind::Heat && dist - tr < HEAT_RANGE;
                if recent >= DEFENSE_TRIGGER_DAMAGE || reach {
                    e.defense_left = d.hold;
                    e.shield_hp = GUN_SHIELD_HEALTH;
                    e.caught = 0;
                    e.burn_timer = 0.0;
                    e.recent.clear();
                    let (color, size) = match d.kind {
                        DefenseKind::Vortex => (Vec3::new(0.6, 1.6, 3.0), 3.5),
                        DefenseKind::Heat => (Vec3::new(3.0, 1.0, 0.2), 3.5),
                        DefenseKind::GunShield => (Vec3::new(0.4, 1.2, 2.0), 3.0),
                        DefenseKind::Block | DefenseKind::ParticleWall => (Vec3::ZERO, 0.0),
                    };
                    if d.kind == DefenseKind::ParticleWall {
                        // Dropped 200 ahead (like BT's) and left there; the cooldown starts now.
                        let facing = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
                        let center = e.pos + facing * 200.0 + Vec3::Z * 130.0;
                        commands.spawn((Transform::from_translation(to_bevy(center)), Visibility::default(), crate::titankit::ParticleWall::enemy(center, facing)));
                        e.defense_left = 0.0;
                        e.defense_cd = d.cooldown;
                    }
                    // The game's shield systems, fixed to the Titan and facing its aim
                    // (TF_OLD_FX keeps the glow stand-in).
                    let system = match d.kind {
                        DefenseKind::Vortex => Some("wpn_vortex_shield_charging"),
                        DefenseKind::Heat => Some("P_wpn_HeatShield"),
                        DefenseKind::GunShield => Some("P_titan_gun_shield_3P"),
                        DefenseKind::Block | DefenseKind::ParticleWall => None,
                    };
                    let facing = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
                    match system {
                        Some(name) if std::env::var_os("TF_OLD_FX").is_none() => {
                            let tf = Transform::from_translation(to_bevy(defense_fx_spot(d.kind, shield_spot))).looking_to(to_bevy(facing).normalize(), Vec3::Y);
                            let fx_e = commands.spawn((tf, Visibility::default(), crate::pfx::PfxTrail::oriented(name))).id();
                            // The dome itself is a refraction effect in the game: a translucent
                            // disc stands in for it.
                            let disc = match d.kind {
                                DefenseKind::Vortex => Some(&fx.vortex_disc),
                                DefenseKind::Heat => Some(&fx.heat_disc),
                                _ => None,
                            };
                            if let Some((m, mat)) = disc {
                                let c = commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, crate::weapons::ShieldShimmer)).id();
                                commands.entity(fx_e).add_child(c);
                            }
                            e.defense_fx = Some(fx_e);
                        }
                        _ if size > 0.0 => {
                            e.defense_fx = Some(commands.spawn((Transform::from_translation(to_bevy(shield_spot)), crate::particles::Glow::new(color, size, false))).id());
                        }
                        _ => {}
                    }
                    if d.kind == DefenseKind::Vortex {
                        crate::audio::event_at(&mut commands, "vortex_3p_shield_start_1_long", muzzle);
                    }
                    log::info!("enemy {} raises its {:?}", kit.name, d.kind);
                }
            }
        }
        // Holding a Vortex or Thermal Shield means not firing; the Gun Shield and Block don't.
        let holding = e.defense_left > 0.0 && matches!(kit.defense.map(|d| d.kind), Some(DefenseKind::Vortex | DefenseKind::Heat));
        let can_shoot = dist < w.npc_max_range && los && facing && !holding;
        e.ability_cd -= dt;
        if let Some(ab) = kit.ability.as_ref().filter(|_| e.ability_cd <= 0.0 && e.burst_left == 0 && los && facing && dist < ab_range(kit) && !holding) {
            let aw = &ab.weapon;
            e.ability_cd = aw.npc_rest_min + (aw.npc_rest_max - aw.npc_rest_min).max(0.0) * e.rand();
            if !ab.sound.is_empty() {
                crate::audio::event_at(&mut commands, &ab.sound, muzzle);
            }
            let mut dir = (aim_at - muzzle).normalize();
            let mut from = muzzle;
            if ab.ground {
                dir = dir.with_z(0.0).normalize_or_zero();
                from = Vec3::new(muzzle.x, muzzle.y, e.pos.z + 40.0);
            }
            // Scorch's Firewall and Ronin's Arc Wave are real ground waves (titankit.rs).
            let wave = match aw.name.as_str() {
                "mp_titanweapon_flame_wall" => Some(crate::titankit::WaveKind::Firewall),
                "mp_titanweapon_arc_wave" => Some(crate::titankit::WaveKind::Arc),
                _ => None,
            };
            if let Some(kind) = wave {
                crate::titankit::spawn_enemy_wave(&mut commands, kind, from, dir);
                continue;
            }
            for i in 0..ab.count {
                let off = if ab.count > 1 { (i as f32 / (ab.count - 1) as f32 - 0.5) * ab.fan } else { 0.0 };
                let (s, c) = off.to_radians().sin_cos();
                let d = Vec3::new(dir.x * c - dir.y * s, dir.x * s + dir.y * c, dir.z);
                spawn_bolt(&mut commands, &fx, from, d * ab.speed, 0.0, e.kit + ABILITY_BOLT);
            }
            continue;
        }
        if e.burst_left == 0 {
            e.rest -= dt;
            if e.rest <= 0.0 && can_shoot {
                let span = (w.npc_max_burst - w.npc_min_burst + 1) as f32;
                e.burst_left = w.npc_min_burst + ((e.rand() * span) as u32).min(w.npc_max_burst - w.npc_min_burst);
                e.shot_timer = 0.0;
                // burst_or_looping_fire_sound_middle_npc, held for the length of the burst.
                let length = e.burst_left as f32 / npc_fire_rate(w);
                if kit.name == "ION" {
                    commands.spawn(crate::audio::SoundCue { cue: Cue::SplitterBurst, at: Some(muzzle), delay: 0.0, duration: Some(length + 0.15) });
                }
            }
            continue;
        }
        if !can_shoot {
            e.burst_left = 0;
            e.rest = w.npc_rest_min;
            continue;
        }
        e.shot_timer -= dt;
        while e.shot_timer <= 0.0 && e.burst_left > 0 {
            e.shot_timer += 1.0 / npc_fire_rate(w);
            e.burst_left -= 1;
            if e.burst_left == 0 {
                e.rest = w.npc_rest_min + (w.npc_rest_max - w.npc_rest_min) * e.rand();
            }
            // Spread: the weapon's hip spread times the proficiency's spread scale, plus the
            // aim cone that narrows over the focus time after the target comes into view.
            // The bias pulls shots toward the centre (an approximation of the engine's use).
            let (scale, bias) = w.proficiency[proficiency as usize];
            let focus = cone0 * (1.0 - e.seen / focus_time).clamp(0.0, 1.0);
            let half = ((w.spread_hip * scale + focus) * 0.5).to_radians();
            let r = e.rand().powf(0.5 + bias) * half;
            let a = e.rand() * std::f32::consts::TAU;
            let mut dir = (aim_at - muzzle).normalize();
            let side = dir.cross(Vec3::Z).normalize_or_zero();
            let up = side.cross(dir);
            // SPMP_Callback_ForceAIMissPlayer: fast Pilots are deliberately missed half the time.
            let miss = is_pilot && fast_pilot(pilot) && e.rand() >= FAST_PILOT_CHANCE_TO_HIT;
            let extra = if miss { ((tr * 2.5) / dist.max(1.0)).atan() } else { 0.0 };
            dir = (dir + side * (r * a.cos() + extra) + up * r * a.sin()).normalize();
            if kit.infantry {
                if !kit.fire_sound.is_empty() {
                    audio::event_at(&mut commands, &kit.fire_sound, muzzle);
                }
            } else if kit.name != "ION" {
                audio::cue(&mut commands, Cue::EnemyGun(kit.arsenal as u8), Some(muzzle));
            }
            let energy = matches!(e.kit, 0 | 2);
            crate::particles::emit(&mut commands, crate::particles::Effect::Muzzle { at: to_bevy(muzzle), dir: to_bevy(dir).normalize_or(Vec3::NEG_Z), scale: 3.0, energy });
            match kit.pellets {
                Some((table, k)) => {
                    for o in table {
                        let d = (dir + up * o[0] * k + side * o[1] * k).normalize();
                        spawn_bolt(&mut commands, &fx, muzzle, d * kit.projectile, kit.gravity, e.kit);
                    }
                }
                None => spawn_bolt(&mut commands, &fx, muzzle, dir * kit.projectile, kit.gravity, e.kit),
            }
        }
    }
}

fn spawn_bolt(commands: &mut Commands, fx: &FxAssets, pos: Vec3, vel: Vec3, gravity: f32, kit: usize) {
    spawn_bolt_dmg(commands, fx, pos, vel, gravity, kit, None);
}

fn spawn_bolt_dmg(commands: &mut Commands, fx: &FxAssets, pos: Vec3, vel: Vec3, gravity: f32, kit: usize, dmg: Option<(f32, f32)>) {
    let _ = fx;
    // Per class (ENEMY_CLASSES order): Ion and Northstar fire energy, Scorch and Tone shells.
    let (color, size, smoke) = match kit {
        0 => (Vec3::new(0.8, 2.0, 6.0), 0.35, false),
        1 => (Vec3::new(6.0, 2.0, 0.4), 0.45, true),
        2 => (Vec3::new(2.0, 3.5, 6.0), 0.3, false),
        4 => (Vec3::new(3.5, 5.0, 1.0), 0.35, true),
        // Abilities (ENEMY_ABILITIES order): Laser Shot, Firewall, Cluster Missile, Arc Wave,
        // Tracker Rockets, Power Shot.
        k if k == ABILITY_BOLT => (Vec3::new(8.0, 1.2, 0.6), 0.5, false),
        k if k == ABILITY_BOLT + 1 => (Vec3::new(8.0, 3.0, 0.5), 0.9, true),
        k if k == ABILITY_BOLT + 2 || k == ABILITY_BOLT + 4 => (Vec3::new(6.0, 3.5, 1.5), 0.4, true),
        k if k == ABILITY_BOLT + 3 => (Vec3::new(2.5, 4.0, 8.0), 1.0, false),
        k if k == ABILITY_BOLT + 5 => (Vec3::new(8.0, 6.0, 3.0), 0.6, false),
        _ => (Vec3::new(5.0, 2.8, 1.0), 0.25, false),
    };
    commands.spawn((Transform::from_translation(to_bevy(pos)), crate::particles::Glow::new(color, size, smoke), Bolt { pos, vel, travelled: 0.0, gravity, kit, dmg }));
}

/// Move Splitter Rifle bolts; they hit the world, the Vortex Shield (absorbed: the Splitter
/// Rifle's vortex_refire_behavior is "absorb"), BT or the Pilot. Damage falls off with distance
/// travelled; Pilots also take the SP explosion splash. Damage to the player is scaled by
/// difficulty, and a Pilot's is capped per hit.
#[allow(clippy::too_many_arguments)]
pub fn update_bolts(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<Control>,
    difficulty: Res<Difficulty>,
    kits: Res<EnemyKits>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    _fx: Res<FxAssets>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Transform)>,
    mut player: Query<(&PlayerTitan, &mut TitanHealth, &mut Vortex, Option<&mut TitanCore>)>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    mut damage_from: ResMut<crate::hud::DamageFrom>,
    status: Res<crate::pilotability::PilotStatus>,
    mut walls: ResMut<crate::titankit::BoltShields>,
    mut shields: ResMut<crate::pilotability::Shields>,
) {
    let dt = time.delta_secs().min(0.1);
    let Ok((titan, mut bt_health, mut vortex, mut core)) = player.single_mut() else { return };
    let Ok((pilot, mut pilot_health)) = pilots.single_mut() else { return };
    let target = player_target(*control, titan, bt_health.dead_for.is_none(), pilot, &settings);
    let eye = Vec3::from(titan.state.eye(&settings.0).to_array());
    let view = Vec3::new(titan.state.pitch.cos() * titan.state.yaw.cos(), titan.state.pitch.cos() * titan.state.yaw.sin(), -titan.state.pitch.sin());
    for (id, mut b, mut tf) in &mut bolts {
        let k = &kits.0[(b.kit % ABILITY_BOLT).min(kits.0.len() - 1)];
        let w = match (&k.ability, b.kit >= ABILITY_BOLT) {
            (Some(a), true) => &a.weapon,
            _ => &k.weapon,
        };
        let falloff = |d: f32, near: f32, far: f32| {
            let f = ((d - w.near_dist) / (w.far_dist - w.near_dist).max(1.0)).clamp(0.0, 1.0);
            near + (far - near) * f
        };
        b.vel.z -= 750.0 * b.gravity * dt;
        let step = b.vel * dt;
        let len = step.length();
        let dir = step / len.max(1e-6);
        let mut t_end = len;
        enum Struck {
            Nothing,
            World,
            Vortex,
            Player,
            Shield(usize),
        }
        let mut struck = Struck::Nothing;
        if let Some(h) = world.0.raycast(SVec3::from(b.pos.to_array()), SVec3::from(dir.to_array()), len) {
            t_end = h.t;
            struck = Struck::World;
        }
        if vortex.active && *control != Control::Pilot {
            // Ray against the vortex sphere around BT's eye, from the front cone only.
            let facing = -dir.dot(view) >= (crate::combat::VORTEX_BULLET_FOV * 0.5).to_radians().cos();
            let oc = b.pos - eye;
            let half_b = oc.dot(dir);
            let c = oc.length_squared() - crate::combat::VORTEX_RADIUS * crate::combat::VORTEX_RADIUS;
            let disc = half_b * half_b - c;
            if facing && disc >= 0.0 {
                let t = -half_b - disc.sqrt();
                if t > 0.0 && t < t_end {
                    t_end = t;
                    struck = Struck::Vortex;
                }
            }
        }
        if let Some((tpos, tr, th, _)) = target.filter(|t| !(t.3 && status.phased())) {
            if let Some(t) = crate::weapons::ray_cylinder(b.pos, dir, tpos, tr + BOLT_HITSIZE, th) {
                if t < t_end {
                    t_end = t;
                    struck = Struck::Player;
                }
            }
        }
        // A-Walls stop enemy rounds and take their damage.
        if let Some((i, t)) = shields.block(b.pos, dir, t_end) {
            let at = b.pos + dir * t;
            shields.0[i].health -= falloff(b.travelled + t, w.npc_damage_near_pilot, w.npc_damage_far_pilot).max(1.0);
            commands.entity(id).despawn();
            crate::particles::emit(&mut commands, crate::particles::Effect::Hit { at: to_bevy(at), normal: -to_bevy(dir), table: w.impact_table, surface: crate::particles::Surface::Shield, victim: false, scale: 1.0 });
            continue;
        }
        // BT's Particle Wall and Gun Shield stop rounds from the front.
        if let Some((i, t)) = walls.hit(b.pos, dir, t_end) {
            t_end = t;
            struck = Struck::Shield(i);
        }
        let at = b.pos + dir * t_end;
        b.travelled += t_end;
        b.pos = at;
        tf.translation = to_bevy(at);
        if matches!(struck, Struck::Nothing) {
            if b.travelled > w.npc_max_range * 1.5 {
                commands.entity(id).despawn();
            }
            continue;
        }
        commands.entity(id).despawn();
        if let Struck::Shield(i) = struck {
            walls.absorbed.push((i, falloff(b.travelled, w.npc_damage_near, w.npc_damage_far)));
            crate::particles::emit(&mut commands, crate::particles::Effect::Hit { at: to_bevy(at), normal: -to_bevy(dir).normalize_or(Vec3::Y), table: w.impact_table, surface: crate::particles::Surface::Shield, victim: false, scale: 1.0 });
            continue;
        }
        if matches!(struck, Struck::Vortex) {
            vortex.absorbed += 1;
            audio::cue(&mut commands, Cue::VortexAbsorb, Some(at));
            // The caught round hangs in the shield (vortex_absorb_effect_third_person).
            crate::pfx::emit_named(&mut commands, "wpn_vortex_projectile_rifle", to_bevy(at), to_bevy(dir).normalize_or(Vec3::NEG_Z));
            continue;
        }
        let scale = difficulty.damage_to_player();
        if matches!(struck, Struck::Player) {
            // Point back along the bolt's path for the HUD's damage direction.
            damage_from.push(at - b.vel.normalize_or_zero() * 3000.0);
        }
        match target {
            Some((_, _, _, false)) => {
                let mut dmg = if matches!(struck, Struck::Player) { b.dmg.map_or(falloff(b.travelled, w.npc_damage_near, w.npc_damage_far), |d| d.1) } else { 0.0 };
                // Splash against heavy armour (Scorch's thermite, Tone's grenades).
                if w.npc_explosion_damage_heavy > 0.0 && w.explosion_radius > 0.0 {
                    let (tpos, _, th, _) = target.unwrap();
                    let d = at.distance(tpos + Vec3::Z * th * 0.5) - 100.0;
                    if d < w.explosion_radius {
                        let k = 1.0 - ((d - w.explosion_inner_radius) / (w.explosion_radius - w.explosion_inner_radius).max(1.0)).clamp(0.0, 1.0);
                        dmg += w.npc_explosion_damage_heavy * k;
                    }
                }
                if dmg <= 0.0 {
                    continue;
                }
                let dmg = dmg * scale;
                let hit = bt_health.damage_from(dmg, w.stops_regen, at - b.vel.normalize_or_zero() * 3000.0);
                if bt_health.block < 1.0 && hit.dealt < dmg * 0.99 {
                    log::debug!("sword block: {dmg:.0} -> {:.0}", hit.dealt);
                }
                if let Some(c) = core.as_mut() {
                    c.credit_received(hit.dealt);
                }
                if hit.doomed_now {
                    audio::cue(&mut commands, Cue::BtDoomed, None);
                }
            }
            Some((tpos, _, th, true)) if !status.phased() => {
                let mut dmg = 0.0;
                if matches!(struck, Struck::Player) {
                    dmg += b.dmg.map_or(falloff(b.travelled, w.npc_damage_near_pilot, w.npc_damage_far_pilot), |d| d.0);
                }
                // Explosion splash: full inside the inner radius, linear to the edge.
                let d = at.distance(tpos + Vec3::Z * th * 0.5);
                if w.explosion_damage > 0.0 && d < w.explosion_radius {
                    let k = 1.0 - ((d - w.explosion_inner_radius) / (w.explosion_radius - w.explosion_inner_radius).max(1.0)).clamp(0.0, 1.0);
                    dmg += w.explosion_damage * k;
                }
                if dmg > 0.0 {
                    pilot_health.damage_capped(dmg * scale, difficulty.max_pilot_damage_per_hit());
                }
            }
            _ => {}
        }
        let effect = match b.kit {
            // Scorch's thermite bursts; everything else plays its impact table (Tone's 40mm
            // has its own blast systems there).
            1 => crate::particles::Effect::Explosion { at: to_bevy(at), scale: 0.8 },
            _ => {
                // What it struck: the player's Titan hull, the Pilot, or the level.
                let (surface, victim) = match (&struck, target) {
                    (Struck::Player, Some((_, _, _, true))) => (crate::particles::Surface::Flesh, true),
                    (Struck::Player, _) => (crate::particles::Surface::Titan, true),
                    _ => (crate::particles::Surface::World, false),
                };
                crate::particles::Effect::Hit { at: to_bevy(at), normal: -dir_b(b.vel), table: w.impact_table, surface, victim, scale: 1.6 }
            }
        };
        crate::particles::emit(&mut commands, effect);
    }
}

fn dir_b(vel: Vec3) -> Vec3 {
    to_bevy(vel).normalize_or(Vec3::NEG_Z)
}

/// A point around the target, about 1500 units out, that can see it, closest to `from`.
fn flank_point(world: &Collision, from: Vec3, target: Vec3) -> Option<Vec3> {
    let eye = target + Vec3::Z * 150.0;
    let mut best: Option<(f32, Vec3)> = None;
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        for r in [1200.0f32, 1700.0] {
            let p = target + Vec3::new(a.cos(), a.sin(), 0.0) * r;
            let Some(ground) = world.0.raycast(SVec3::new(p.x, p.y, p.z + 500.0), -SVec3::Z, 1200.0) else { continue };
            let p = Vec3::new(p.x, p.y, ground.point.z);
            if (p.z - target.z).abs() > 400.0 {
                continue;
            }
            let from_p = p + Vec3::Z * 200.0;
            let d = eye - from_p;
            if world.0.raycast(SVec3::from(from_p.to_array()), SVec3::from(d.normalize().to_array()), d.length() - 40.0).is_some() {
                continue;
            }
            let cost = p.distance(from);
            if best.is_none_or(|b| cost < b.0) {
                best = Some((cost, p));
            }
        }
    }
    best.map(|b| b.1)
}

/// How far a class uses its ability: the ability script's npc_max_range when it has one.
fn ab_range(kit: &EnemyKit) -> f32 {
    kit.ability.as_ref().map(|a| if a.weapon.npc_max_range > 0.0 { a.weapon.npc_max_range } else { kit.weapon.npc_max_range }).unwrap_or(0.0)
}

/// NPCs fire at most one shot per server frame (10 Hz); the scripts' npc_damage values make up
/// for it ("need to compensate for NPCs not firing as fast as players (1 shot per frame max)").
fn npc_fire_rate(w: &WeaponDef) -> f32 {
    w.fire_rate.clamp(0.1, 10.0)
}

/// Pick the free direction (of 16, probed at chest height) closest to `want`.
pub fn steer(world: &Collision, pos: Vec3, want: Vec2) -> Vec2 {
    // Probed at chest height and just above step height (railings, kerbs).
    let origin = SVec3::new(pos.x, pos.y, pos.z + 150.0);
    let low = SVec3::new(pos.x, pos.y, pos.z + 90.0);
    let free = |d: Vec2| {
        let d = SVec3::new(d.x, d.y, 0.0);
        world.0.raycast(origin, d, 350.0).is_none() && world.0.raycast(low, d, 350.0).is_none()
    };
    let want = want.normalize_or_zero();
    if free(want) {
        return want;
    }
    let mut best = (f32::MIN, Vec2::ZERO);
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        let d = Vec2::new(a.cos(), a.sin());
        if free(d) {
            let score = d.dot(want);
            if score > best.0 {
                best = (score, d);
            }
        }
    }
    best.1
}

fn animate_enemy(actors: &mut Query<&mut Actor>, e: &mut Enemy, dt: f32, target: Option<Vec3>) {
    let Ok(mut actor) = actors.get_mut(e.actor) else { return };
    actor.autoplay = false;
    // Footsteps and servos from the walk cycle's typed sound events, heard where the Titan is.
    actor.event_sounds = e.sound_type.is_some();
    actor.sound_type = e.sound_type;
    actor.sound_at = Some(e.pos);
    let s = &e.state;
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let v = Vec2::new(s.vel.x, s.vel.y);
    let speed = v.length();
    let forward_speed = v.dot(fwd);
    let moving = (speed / 60.0).clamp(0.0, 1.0);
    let idle = actor.clip(e.clips[0]).or_else(|| actor.clip("at_IDLE_2"));
    let walk_f = actor.clip(e.clips[1]);
    let walk_b = actor.clip(e.clips[2]);
    let walk = if forward_speed < -20.0 { walk_b } else { walk_f };
    if let Some(c) = walk {
        let clip = &actor.clips[c];
        let stride = (clip.ground_speed * clip.duration).max(1.0);
        e.move_cycle = (e.move_cycle + speed * dt / stride).rem_euclid(1.0);
    }
    if let Some(c) = idle {
        e.idle_cycle = (e.idle_cycle + dt / actor.clips[c].duration.max(1e-3)).rem_euclid(1.0);
    }
    let mut layers = Vec::new();
    if let Some(c) = idle {
        layers.push(Layer { clip: c, cycle: e.idle_cycle, weight: 1.0 - moving });
    }
    if let Some(c) = walk {
        layers.push(Layer { clip: c, cycle: e.move_cycle, weight: moving.max(1e-3) });
    }
    actor.layers = layers;
    let pitch = target.map(|t| {
        let to = t - e.pos;
        (to.z + 60.0).atan2(to.truncate().length().max(1.0)).to_degrees()
    });
    let grid = e.aim_grids[(moving > 0.5) as usize];
    actor.aim = Some(Aim { grid: grid.into(), yaw: 0.0, pitch: pitch.unwrap_or(0.0), weight: 1.0 });
}

const ENEMY_DROP_ANIM: &str = "at_hotdrop_drop_2knee_turbo";
const ENEMY_STAND_ANIM: &str = "at_hotdrop_quickstand";

fn enemy_titanfall(commands: &mut Commands, actors: &mut Query<&mut Actor>, e: &mut Enemy, tf: &mut Transform, vis: &mut Visibility, dt: f32) {
    let Ok(mut actor) = actors.get_mut(e.actor) else {
        e.drop = None;
        e.stand = None;
        return;
    };
    actor.autoplay = false;
    let rot = Quat::from_rotation_z(e.state.yaw);
    tf.rotation = rot;
    if let Some(t) = e.drop {
        let t = t + dt;
        if t < 0.0 {
            *vis = Visibility::Hidden;
            e.drop = Some(t);
            return;
        }
        let Some(c) = actor.clip(ENEMY_DROP_ANIM) else {
            e.drop = None;
            return;
        };
        let clip = &actor.clips[c];
        let total = clip.movement.last().copied().unwrap_or_default();
        let n = clip.movement.len().max(2);
        let hit = clip.movement.iter().position(|m| m[2] <= total[2] + 2.0).unwrap_or(n - 1);
        let impact = hit as f32 / (n - 1) as f32 * clip.duration;
        let duration = clip.duration;
        // Join the drop for its last two seconds before impact (the rest is high in the sky).
        let t = if e.landed { t } else { t.max(impact - 2.0) };
        let now = clip.movement_at(t).unwrap_or(total);
        tf.translation = e.pos + rot * Vec3::new(now[0] - total[0], now[1] - total[1], now[2] - total[2]);
        actor.layers = vec![Layer { clip: c, cycle: (t / duration.max(1e-3)).min(0.999), weight: 1.0 }];
        actor.aim = None;
        if !e.landed && t >= impact {
            e.landed = true;
            log::debug!("enemy titanfall impact at {:.0?} after {t:.2}s", e.pos);
            audio::cue(commands, Cue::TitanLand, Some(e.pos));
            let b = to_bevy(e.pos);
            crate::particles::emit(commands, crate::particles::Effect::Explosion { at: b, scale: 0.8 });
            crate::particles::emit(commands, crate::particles::Effect::DustRing { at: b, radius: 220.0 * crate::player::UNIT });
        }
        if t >= duration {
            e.drop = None;
            e.stand = Some(0.0);
            tf.translation = e.pos;
        } else {
            e.drop = Some(t);
        }
    } else if let Some(t) = e.stand {
        let t = t + dt;
        tf.translation = e.pos;
        match actor.clip(ENEMY_STAND_ANIM) {
            Some(c) if t < actor.clips[c].duration => {
                let d = actor.clips[c].duration;
                actor.layers = vec![Layer { clip: c, cycle: (t / d).min(0.999), weight: 1.0 }];
                e.stand = Some(t);
            }
            _ => e.stand = None,
        }
    }
}

fn animate_death(actors: &mut Query<&mut Actor>, e: &Enemy) {
    let Ok(mut actor) = actors.get_mut(e.actor) else { return };
    actor.autoplay = false;
    actor.aim = None;
    if let Some(c) = actor.clip(e.clips[3]) {
        let cycle = (e.dead_for / actor.clips[c].duration.max(1e-3)).min(0.999);
        actor.layers = vec![Layer { clip: c, cycle, weight: 1.0 }];
    }
}

fn ensure_health_bar(commands: &mut Commands, ui: &crate::ui::UiAssets, id: Entity, e: &mut Enemy) {
    if e.bar.is_some() || e.infantry {
        return;
    }
    let frame = commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Vh(13.0), flex_direction: FlexDirection::Column, row_gap: Val::Vh(0.25), ..default() }, Visibility::Hidden))
        .id();
    let label = commands
        .spawn((
            Text::new("ION"),
            TextFont { font: ui.bold_font.clone(), font_size: 14.0, ..default() },
            TextColor(Color::srgb(1.0, 0.36, 0.26)),
            crate::ui::VhText(1.5),
        ))
        .id();
    let bar = |commands: &mut Commands, h: f32, color: Color| {
        let bg = commands.spawn((Node { height: Val::Vh(h), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)))).id();
        let fill = commands.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(color))).id();
        commands.entity(bg).add_child(fill);
        (bg, fill)
    };
    let (shield_bg, shield) = bar(commands, 0.35, Color::srgb(0.38, 0.78, 1.0));
    let (health_bg, health) = bar(commands, 0.8, Color::srgb(1.0, 0.36, 0.26));
    commands.entity(frame).add_children(&[label, shield_bg, health_bg]);
    commands.entity(frame).insert(HealthBar { enemy: id, label, shield, health });
    e.bar = Some(frame);
}

/// Explosion when an enemy Titan dies.
pub fn death_explosions(mut commands: Commands, enemies: Query<&Enemy, Changed<Enemy>>) {
    for e in &enemies {
        if e.just_died && !e.infantry {
            audio::cue(&mut commands, Cue::TitanDeath, Some(e.pos + Vec3::Z * 150.0));
            let b = to_bevy(e.pos + Vec3::Z * 150.0);
            crate::particles::emit(&mut commands, crate::particles::Effect::TitanDeath { at: b });
            commands.spawn((
                PointLight { color: Color::srgb(1.0, 0.55, 0.25), intensity: 400_000.0, range: 40.0, ..default() },
                Transform::from_translation(b),
                Fx { life: 0.6, max: 0.6, base: Vec3::ONE, keep_z: false },
            ));
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn update_health_bars(
    kits: Res<EnemyKits>,
    hidden: Res<crate::hud::HudHidden>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    ordnance: Query<&crate::abilities::Ordnance>,
    enemies: Query<&Enemy>,
    mut bars: Query<(&HealthBar, &mut Node, &mut Visibility)>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), Without<HealthBar>>,
    mut labels: Query<(&mut Text, &mut TextColor)>,
) {
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    for (bar, mut node, mut vis) in &mut bars {
        let Ok(e) = enemies.get(bar.enemy) else { continue };
        let locks = ordnance.single().map(|o| o.locks_on(bar.enemy)).unwrap_or(0);
        let top = to_bevy(e.pos + Vec3::Z * (e.height + 60.0));
        match camera.world_to_viewport(cam_tf, top) {
            Ok(p) if e.alive() && !hidden.0 => {
                *vis = Visibility::Inherited;
                node.left = px(p.x);
                node.top = px(p.y);
                // Centre on the point (13vh wide).
                node.margin = UiRect::left(Val::Vh(-6.5));
            }
            _ => *vis = Visibility::Hidden,
        }
        if let Ok((mut t, mut c)) = labels.get_mut(bar.label) {
            let name = kits.0.get(e.kit).map(|k| k.name.as_str()).unwrap_or("TITAN");
            let s = if locks > 0 { format!("{name}   LOCK x{locks}") } else if e.doomed() { format!("{name}   DOOMED") } else { name.to_string() };
            if t.0 != s {
                t.0 = s;
            }
            c.0 = if locks > 0 { Color::srgb(1.0, 0.8, 0.25) } else { Color::srgb(1.0, 0.36, 0.26) };
        }
        if let Ok((mut n, _)) = fills.get_mut(bar.shield) {
            n.width = percent(e.v.shield / e.v.max_shield.max(1.0) * 100.0);
        }
        if let Ok((mut n, mut bg)) = fills.get_mut(bar.health) {
            n.width = percent(e.v.fraction() * 100.0);
            bg.0 = if e.flash > 0.0 {
                Color::WHITE
            } else if e.doomed() {
                Color::srgb(0.75, 0.1, 0.05)
            } else {
                Color::srgb(1.0, 0.36, 0.26)
            };
        }
    }
}

/// Test helper: while a scripted `aim` entry is active, look at the nearest living enemy.
#[allow(clippy::too_many_arguments)]
pub fn script_aim(
    script: Res<crate::player::Script>,
    control: Res<Control>,
    settings: Res<TitanSettings>,
    pilot_settings: Res<crate::pilotctl::PilotSettings>,
    mut input: ResMut<crate::player::PlayerInput>,
    titans: Query<&PlayerTitan>,
    pilots: Query<&PlayerPilot>,
    enemies: Query<&Enemy>,
) {
    if !script.aim_dummy {
        return;
    }
    let (Ok(t), Ok(p)) = (titans.single(), pilots.single()) else { return };
    let eye = if *control == Control::Pilot {
        Vec3::from(p.state.eye(&pilot_settings.0).to_array())
    } else {
        Vec3::from(t.state.eye(&settings.0).to_array())
    };
    let any_titan = enemies.iter().any(|d| d.alive() && !d.infantry) && std::env::var_os("TF_AIM_GRUNTS").is_none();
    let target = enemies
        .iter()
        .filter(|d| d.alive() && (d.infantry != any_titan))
        .map(|d| d.pos + Vec3::Z * d.height * std::env::var("TF_AIM_Z").ok().and_then(|v| v.parse().ok()).unwrap_or(0.6))
        .min_by(|a, b| a.distance(eye).total_cmp(&b.distance(eye)));
    if let Some(p) = target {
        let d = (p - eye).normalize();
        input.yaw = d.y.atan2(d.x);
        input.pitch = -d.z.asin();
    }
}
