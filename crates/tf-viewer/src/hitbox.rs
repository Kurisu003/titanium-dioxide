//! Hit detection against enemies' real hitboxes (the model's default hitbox set, posed by the
//! animated skeleton) and the damage multipliers that come with them:
//! - infantry: a head hit (HITGROUP_HEAD, 1) is multiplied by the weapon's
//!   `damage_headshot_scale`; other groups take normal damage;
//! - Titans: boxes flagged `critShotOverride` (the cockpit hatch) are critical hits, multiplied
//!   by `critical_hit_damage_scale` for weapons with `critical_hit` 1. Weapons with
//!   `titanarmor_critical_hit_required` only do their Titan-armour damage on a crit and their
//!   normal damage elsewhere.

use crate::actor::Actor;
use crate::player::to_bevy;
use crate::targets::{Enemy, EnemyKits};
use crate::weapons::{ray_cylinder, WeaponDef};
use bevy::prelude::*;
use tf_assets::mdl::Hitbox;

/// Hitboxes per enemy class (index into EnemyKits).
#[derive(Resource, Default)]
pub struct EnemyHitboxes(pub Vec<Vec<Hitbox>>);

/// Load each enemy class's hitboxes once the kits exist.
pub fn load_enemy_hitboxes(mut commands: Commands, gd: Res<crate::gamedata::GameData>, kits: Option<Res<EnemyKits>>, have: Option<Res<EnemyHitboxes>>) {
    let (Some(kits), None) = (kits, have) else { return };
    let boxes: Vec<Vec<Hitbox>> = kits.0.iter().map(|k| crate::actor::model_hitboxes(&gd, &k.model)).collect();
    log::info!("hitboxes: {:?}", kits.0.iter().zip(&boxes).map(|(k, b)| format!("{} {} ({} crit)", k.name, b.len(), b.iter().filter(|h| h.crit).count())).collect::<Vec<_>>());
    commands.insert_resource(EnemyHitboxes(boxes));
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    /// Distance along the ray (game units).
    pub t: f32,
    pub group: i32,
    pub crit: bool,
}

pub const HITGROUP_HEAD: i32 = 1;

/// The player's recent hits this frame (for headshot / crit hit markers).
#[derive(Resource, Default)]
pub struct RecentHits(pub Vec<Hit>);

impl RecentHits {
    pub fn push(&mut self, h: Hit) {
        self.0.push(h);
    }
}

/// Trace a ray (game space, `d` normalized) against an enemy's hitboxes; without hitboxes (or
/// before the skeleton is posed) its collision cylinder stands in as a chest hit.
pub fn trace(o: Vec3, d: Vec3, max: f32, e: &Enemy, boxes: &EnemyHitboxes, actors: &Query<&Actor>, globals: &Query<&GlobalTransform>) -> Option<Hit> {
    // Broad phase: a generous cylinder (arms and weapons reach past the body).
    ray_cylinder(o, d, e.pos - Vec3::Z * 20.0, e.radius * 2.5 + 20.0, e.height * 1.35 + 20.0).filter(|t| *t < max)?;
    let list = boxes.0.get(e.kit).filter(|b| !b.is_empty());
    let actor = actors.get(e.actor).ok();
    let (Some(list), Some(actor)) = (list, actor) else {
        return ray_cylinder(o, d, e.pos, e.radius, e.height).filter(|t| *t < max).map(|t| Hit { t, group: 2, crit: false });
    };
    let ob = to_bevy(o);
    let db = to_bevy(o + d) - ob;
    let mut best: Option<Hit> = None;
    for hb in list {
        let Some(g) = actor.joints.get(hb.bone).and_then(|j| globals.get(*j).ok()) else { continue };
        let inv = g.affine().inverse();
        let lo = inv.transform_point3(ob);
        let ld = inv.transform_vector3(db);
        if let Some(t) = slab(lo, ld, Vec3::from(hb.min), Vec3::from(hb.max)) {
            if t < max && best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, group: hb.group, crit: hb.crit });
            }
        }
    }
    if best.is_none() && std::env::var_os("TF_HITBOX_DEBUG").is_some() {
        let g0 = list.first().and_then(|hb| actor.joints.get(hb.bone)).and_then(|j| globals.get(*j).ok()).map(|g| crate::player::from_bevy(g.translation()));
        log::debug!("hitbox miss: enemy at {:.0} (kit {}), first box bone at {:?}, ray from {:.0} dir {:.2} max {:.0}", e.pos, e.kit, g0, o, d, max);
    }
    best
}

/// Where a smart round aims on an enemy (game space): on infantry the head
/// (`SMART_AMMO_AI_AIM_ATTACHMENT` "HEADSHOT"), the centre of the model's largest head box as
/// posed now; the chest otherwise or without hitboxes.
pub fn aim_point(e: &Enemy, boxes: &EnemyHitboxes, actors: &Query<&Actor>, globals: &Query<&GlobalTransform>) -> Vec3 {
    let chest = e.pos + Vec3::Z * e.height * 0.6;
    let (true, Some(list), Ok(actor)) = (e.infantry, boxes.0.get(e.kit), actors.get(e.actor)) else { return chest };
    let volume = |h: &Hitbox| (Vec3::from(h.max) - Vec3::from(h.min)).abs().element_product();
    let Some(head) = list.iter().filter(|h| h.group == HITGROUP_HEAD).max_by(|a, b| volume(a).total_cmp(&volume(b))) else { return chest };
    let Some(g) = actor.joints.get(head.bone).and_then(|j| globals.get(*j).ok()) else { return chest };
    crate::player::from_bevy(g.transform_point((Vec3::from(head.min) + Vec3::from(head.max)) * 0.5))
}

/// Ray / axis-aligned box: the entry parameter (or 0 when starting inside).
fn slab(o: Vec3, d: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for a in 0..3 {
        if d[a].abs() < 1e-9 {
            if o[a] < min[a] || o[a] > max[a] {
                return None;
            }
            continue;
        }
        let (mut n, mut f) = ((min[a] - o[a]) / d[a], (max[a] - o[a]) / d[a]);
        if n > f {
            std::mem::swap(&mut n, &mut f);
        }
        t0 = t0.max(n);
        t1 = t1.min(f);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// Damage for a hit at `dist` (falloff between the weapon's near and far distances), against
/// infantry (normal damage values) or Titans (Titan-armour values), with the hit's multiplier.
pub fn damage(def: &WeaponDef, hit: &Hit, dist: f32, infantry: bool) -> f32 {
    // Near value to far value between the near and far distances, then on toward the very-far
    // value at the very-far distance.
    let f = ((dist - def.near_dist) / (def.far_dist - def.near_dist).max(1.0)).clamp(0.0, 1.0);
    let g = ((dist - def.far_dist) / (def.very_far_dist - def.far_dist).max(1.0)).clamp(0.0, 1.0);
    let lerp3 = |near: f32, far: f32, very: f32| if dist <= def.far_dist { near + (far - near) * f } else { far + (very - far) * g };
    if infantry {
        let base = lerp3(def.damage_near_pilot, def.damage_far_pilot, def.damage_very_far_pilot);
        if hit.group == HITGROUP_HEAD { base * def.headshot_scale } else { base }
    } else {
        let armour = lerp3(def.damage_near, def.damage_far, def.damage_very_far);
        if hit.crit && def.crit {
            armour * def.crit_scale
        } else if def.titanarmor_crit_required {
            lerp3(def.damage_near_pilot, def.damage_far_pilot, def.damage_very_far_pilot)
        } else {
            armour
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slab_hits_and_misses() {
        let (min, max) = (Vec3::splat(-1.0), Vec3::splat(1.0));
        assert_eq!(slab(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, min, max), Some(4.0));
        assert_eq!(slab(Vec3::new(-5.0, 3.0, 0.0), Vec3::X, min, max), None);
        assert_eq!(slab(Vec3::ZERO, Vec3::X, min, max), Some(0.0));
    }
}
