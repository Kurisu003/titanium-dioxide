//! Smart ammo for the pilot's weapons (the `smart_ammo_*` keys of a weapon script and
//! `sh_smart_ammo.gnut`): the Smart Pistol paints locks on infantry and a trigger pull fires a
//! bullet at each lock; the Archer locks onto a Titan while aiming down sights and its rocket
//! homes on it.
//!
//! From the scripts:
//! - Targets are searched in a cone of `smart_ammo_search_angle` (the full angle, as the
//!   crosshair's `smartFov` draws it) out to `smart_ammo_search_distance`, with line of sight
//!   (other Titans block it with `smart_ammo_titans_block_los`). `smart_ammo_lock_type` picks
//!   them: `small` (not Titans), `large` (Titans only) or `any`.
//! - A target starts locking `smart_ammo_new_target_delay` after it is found. Each lock takes
//!   between `smart_ammo_targeting_time_min` and `_max`. A target takes up to
//!   `smart_ammo_target_max_locks_normal` / `_heavy` locks; -1 means its health over the
//!   weapon's near damage ("divide health by damage near"), NPCs needing
//!   `smart_ammo_target_npc_lock_factor` times fewer ("smart pistol does head shots at 2x
//!   damage"). All targets together hold at most `smart_ammo_max_targeted_burst` locks, and at
//!   most `smart_ammo_max_targets` targets are tracked.
//! - Locks are kept only while searching is allowed (`smart_ammo_allow_hip_fire_lock`,
//!   `smart_ammo_allow_ads_lock`) and survive `smart_ammo_unlock_debounce_time` out of sight.
//! - `SmartAmmo_FireWeapon`: a pull of the trigger fires one round per full lock (the burst
//!   walks the locked targets), bullets flying straight at the target's aim attachment
//!   (`HEADSHOT` on NPCs) and missiles homing on it. The locks clear after the burst
//!   (`SmartAmmo_SetUnlockAfterBurst`). Without a lock the weapon fires normally
//!   (`SmartAmmo_SetAllowUnlockedFiring`: the Smart Pistol, and the Archer in SP).
//!
//! Engine-side, so estimated: the targeting time runs from min (point blank) to max (at the
//! search distance) with range, and missile homing is taken as a turn rate in degrees/s.

use crate::player::Collision;
use crate::targets::Enemy;
use crate::weapons::{ray_cylinder, WeaponDef};
use bevy::prelude::*;
use tf_assets::settings::PlayerSettings;
use tf_sim::glam::Vec3 as SVec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    /// Anything but Titans (the Smart Pistol).
    Small,
    /// Titans only (the Archer).
    Large,
    Any,
}

/// A weapon's smart ammo settings.
#[derive(Debug, Clone)]
pub struct SmartDef {
    /// Full cone angle, degrees.
    pub search_angle: f32,
    pub search_distance: f32,
    /// Lock time on players (unused here, kept for reference) and on NPCs; NPCs lock faster.
    #[allow(dead_code)]
    pub time_min: f32,
    #[allow(dead_code)]
    pub time_max: f32,
    /// NPC targets lock faster (`smart_ammo_targeting_time_*_npc`).
    pub time_min_npc: f32,
    pub time_max_npc: f32,
    pub new_target_delay: f32,
    pub max_targeted_burst: u32,
    /// 0: no limit.
    pub max_targets: u32,
    pub max_locks_normal: i32,
    pub max_locks_heavy: i32,
    pub npc_lock_factor: f32,
    pub unlock_debounce: f32,
    pub hip_lock: bool,
    pub ads_lock: bool,
    pub lock_type: LockType,
    pub titans_block_los: bool,
    /// `smart_ammo_weapon_type` "homing_missile" (otherwise bullets).
    pub missile: bool,
    /// Missile launch speed and homing (SmartAmmo_SetMissileSpeed / _SetMissileHomingSpeed in
    /// the weapon's .nut).
    pub missile_speed: f32,
    pub homing_speed: f32,
    /// Sound events: a lock completes, a target starts locking, the lock is held.
    pub confirmed_sound: String,
    pub acquiring_sound: String,
    pub locked_sound: String,
    /// The crosshair shows the search cone (`smart_ammo_hud_type` smart_pistol).
    pub hud_circle: bool,
}

impl SmartDef {
    /// The smart ammo block of a (modded) weapon script, if it has one.
    pub fn from_settings(name: &str, mods: &[String], s: &PlayerSettings) -> Option<Self> {
        let get = |k: &str| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}")));
        let f = |k: &str, d: f32| get(k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
        let text = |k: &str| get(k).unwrap_or_default().to_string();
        if f("smart_ammo_search_distance", 0.0) <= 0.0 || f("smart_ammo_search_angle", 0.0) <= 0.0 {
            return None;
        }
        let missile = get("smart_ammo_weapon_type").is_some_and(|t| t.eq_ignore_ascii_case("homing_missile"));
        let (missile_speed, homing_speed) = missile_flight(name, mods);
        Some(Self {
            search_angle: f("smart_ammo_search_angle", 0.0),
            search_distance: f("smart_ammo_search_distance", 0.0),
            time_min: f("smart_ammo_targeting_time_min", 0.5),
            time_max: f("smart_ammo_targeting_time_max", f("smart_ammo_targeting_time_min", 0.5)),
            time_min_npc: f("smart_ammo_targeting_time_min_npc", f("smart_ammo_targeting_time_min", 0.5)),
            time_max_npc: f("smart_ammo_targeting_time_max_npc", f("smart_ammo_targeting_time_max", f("smart_ammo_targeting_time_min", 0.5))),
            new_target_delay: f("smart_ammo_new_target_delay", 0.0),
            max_targeted_burst: f("smart_ammo_max_targeted_burst", 1.0).max(1.0) as u32,
            max_targets: f("smart_ammo_max_targets", 0.0).max(0.0) as u32,
            max_locks_normal: f("smart_ammo_target_max_locks_normal", 1.0) as i32,
            max_locks_heavy: f("smart_ammo_target_max_locks_heavy", 1.0) as i32,
            npc_lock_factor: f("smart_ammo_target_npc_lock_factor", 1.0).max(0.01),
            unlock_debounce: f("smart_ammo_unlock_debounce_time", 0.0),
            hip_lock: f("smart_ammo_allow_hip_fire_lock", 0.0) > 0.0,
            ads_lock: f("smart_ammo_allow_ads_lock", 0.0) > 0.0,
            lock_type: match get("smart_ammo_lock_type").map(str::to_ascii_lowercase).as_deref() {
                Some("small") => LockType::Small,
                Some("large") => LockType::Large,
                _ => LockType::Any,
            },
            titans_block_los: f("smart_ammo_titans_block_los", 0.0) > 0.0,
            missile,
            missile_speed,
            homing_speed,
            confirmed_sound: text("smart_ammo_target_confirmed_sound"),
            acquiring_sound: text("smart_ammo_looping_sound_acquiring"),
            locked_sound: text("smart_ammo_looping_sound_locked"),
            hud_circle: get("smart_ammo_hud_type").is_some_and(|t| t.eq_ignore_ascii_case("smart_pistol")),
        })
    }

    pub fn can_target(&self, e: &Enemy) -> bool {
        match self.lock_type {
            LockType::Small => e.infantry,
            LockType::Large => !e.infantry,
            LockType::Any => true,
        }
    }

    /// How many locks `e` can take (at most the whole burst).
    pub fn max_locks(&self, e: &Enemy, def: &WeaponDef) -> u32 {
        let n = if e.infantry {
            match self.max_locks_normal {
                n if n < 0 => (e.v.health / (def.damage_near_pilot * self.npc_lock_factor).max(1.0)).ceil() as i32,
                n => n,
            }
        } else {
            match self.max_locks_heavy {
                n if n < 0 => ((e.v.health + e.v.shield) / def.damage_near.max(1.0)).ceil() as i32,
                n => n,
            }
        };
        (n.max(1) as u32).min(self.max_targeted_burst)
    }

    /// Seconds per lock at `dist`; every enemy here is an NPC, so the `_npc` times apply.
    pub fn targeting_time(&self, dist: f32) -> f32 {
        let k = (dist / self.search_distance.max(1.0)).clamp(0.0, 1.0);
        (self.time_min_npc + (self.time_max_npc - self.time_min_npc) * k).max(0.01)
    }
}

/// Missile speed and homing from each smart launcher's .nut (SP branch); sh_smart_ammo.gnut's
/// defaults otherwise.
fn missile_flight(name: &str, mods: &[String]) -> (f32, f32) {
    let has = |m: &str| mods.iter().any(|x| x.eq_ignore_ascii_case(m));
    match name {
        // OnWeaponActivate_weapon_rocket_launcher: S2S_MISSILE_SPEED / S2S_MISSILE_HOMING with
        // sp_s2s_settings, else 1750 / 70 in single player.
        "mp_weapon_rocket_launcher" if has("sp_s2s_settings") => (2500.0, 5000.0),
        "mp_weapon_rocket_launcher" => (1750.0, 70.0),
        _ => (2500.0, 300.0),
    }
}

/// One tracked target.
#[derive(Debug, Clone, Copy)]
pub struct Lock {
    pub target: Entity,
    /// Locks so far: the whole part counts, the fraction is the next one forming.
    pub frac: f32,
    pub max: u32,
    /// Seconds since found (locking starts after `new_target_delay`).
    pub seen: f32,
    /// Seconds out of the search (dropped after `unlock_debounce`).
    pub lost: f32,
}

impl Lock {
    pub fn full(&self) -> u32 {
        (self.frac + 1e-4).floor() as u32
    }
}

/// A weapon's current locks.
#[derive(Debug, Clone, Default)]
pub struct Locks {
    pub targets: Vec<Lock>,
    /// Targets of the rounds left in the burst being fired.
    pub queue: Vec<Entity>,
}

impl Locks {
    pub fn clear(&mut self) {
        self.targets.clear();
        self.queue.clear();
    }
    /// The burst a trigger pull fires: each locked target once per full lock, in lock order.
    pub fn burst(&self, cap: u32) -> Vec<Entity> {
        let mut out = Vec::new();
        for l in &self.targets {
            for _ in 0..l.full() {
                out.push(l.target);
            }
        }
        out.truncate(cap as usize);
        out
    }
}

/// What the HUD shows for one target: its lock fraction (whole part = full locks) and the
/// locks it can take.
#[derive(Debug, Clone, Copy)]
pub struct LockView {
    pub target: Entity,
    pub frac: f32,
    pub max: u32,
}

impl Locks {
    pub fn view(&self) -> Vec<LockView> {
        self.targets.iter().filter(|l| l.lost == 0.0).map(|l| LockView { target: l.target, frac: l.frac, max: l.max }).collect()
    }
}

/// Where smart rounds and locks aim on an enemy: its chest (the lock and line-of-sight check).
pub fn chest(e: &Enemy) -> Vec3 {
    e.pos + Vec3::Z * e.height * 0.6
}

/// One frame of target search: find the enemies in the cone, grow their locks, drop the ones
/// that left. `searching` is false when the weapon can't lock right now (hip fire on the
/// Archer, reloading, sprinting). Plays the script's lock sounds.
#[allow(clippy::too_many_arguments)]
pub fn search(commands: &mut Commands, sd: &SmartDef, def: &WeaponDef, locks: &mut Locks, searching: bool, eye: Vec3, view: Vec3, dt: f32, world: &Collision, enemies: &Query<(Entity, &mut Enemy)>) {
    let cos_half = (sd.search_angle * 0.5).to_radians().cos();
    // (target, distance, alignment, max locks)
    let mut found: Vec<(Entity, f32, f32, u32)> = Vec::new();
    if searching {
        for (id, e) in enemies.iter() {
            if !e.alive() || !sd.can_target(e) {
                continue;
            }
            let to = chest(e) - eye;
            let dist = to.length();
            if dist > sd.search_distance || dist < 1.0 {
                continue;
            }
            let dir = to / dist;
            let align = dir.dot(view);
            if align < cos_half {
                continue;
            }
            let reach = (dist - e.radius).max(1.0);
            if world.0.raycast(SVec3::from(eye.to_array()), SVec3::from(dir.to_array()), reach).is_some() {
                continue;
            }
            if sd.titans_block_los
                && enemies.iter().any(|(o, oe)| o != id && oe.alive() && !oe.infantry && ray_cylinder(eye, dir, oe.pos, oe.radius, oe.height).is_some_and(|t| t < reach))
            {
                continue;
            }
            found.push((id, dist, align, sd.max_locks(e, def)));
        }
    }
    for l in &mut locks.targets {
        match found.iter().find(|f| f.0 == l.target) {
            Some(f) => {
                l.lost = 0.0;
                l.seen += dt;
                l.max = f.3;
            }
            None => l.lost += dt,
        }
    }
    locks.targets.retain(|l| l.lost == 0.0 || l.lost < sd.unlock_debounce);
    // New targets, nearest the crosshair first.
    found.sort_by(|a, b| b.2.total_cmp(&a.2));
    for &(id, _, _, max) in &found {
        let full = sd.max_targets > 0 && locks.targets.len() as u32 >= sd.max_targets;
        if !full && !locks.targets.iter().any(|l| l.target == id) {
            locks.targets.push(Lock { target: id, frac: 0.0, max, seen: 0.0, lost: 0.0 });
        }
    }
    // Grow the locks of the targets in view, keeping the total within one burst.
    let mut total: u32 = locks.targets.iter().map(Lock::full).sum();
    let (mut started, mut confirmed) = (false, false);
    for l in &mut locks.targets {
        let before = l.full();
        let room = sd.max_targeted_burst.saturating_sub(total - before);
        let limit = l.max.min(room) as f32;
        if l.lost == 0.0 && l.seen >= sd.new_target_delay && l.frac < limit {
            let dist = found.iter().find(|f| f.0 == l.target).map(|f| f.1).unwrap_or(sd.search_distance);
            started |= l.frac == 0.0;
            l.frac += dt / sd.targeting_time(dist);
        }
        l.frac = l.frac.min(limit.max(before as f32));
        total = total + l.full() - before;
        confirmed |= l.full() > before;
    }
    if started && !sd.acquiring_sound.is_empty() {
        crate::audio::event(commands, &sd.acquiring_sound);
    }
    if confirmed {
        for s in [&sd.confirmed_sound, &sd.locked_sound] {
            if !s.is_empty() {
                crate::audio::event(commands, s);
            }
        }
    }
}
