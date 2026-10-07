//! Titan weapons: the XO-16, from its weapon script. Hitscan shots against the world and
//! dummy Titans, spread and view kick, reload, ADS zoom, first-person viewmodel animation,
//! the gun in BT's hand, tracers and impact flashes.

use crate::actor::{Actor, Layer};
use crate::player::{to_bevy, CameraMode, Collision, PlayerInput, PlayerTitan, TitanSettings};
use crate::particles;
use crate::targets::Enemy;
use crate::audio::{self, Cue};
use bevy::prelude::*;
use tf_assets::settings::PlayerSettings;
use tf_sim::glam::Vec3 as SVec3;

/// Impact table names live for the program (effects are `Copy` and carry them).
pub fn intern(s: &str) -> &'static str {
    static SET: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut set = SET.lock().unwrap();
    if let Some(t) = set.iter().find(|t| **t == s) {
        return t;
    }
    let t: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.push(t);
    t
}

#[derive(Debug, Clone)]
pub struct WeaponDef {
    pub name: String,
    pub fire_rate: f32,
    pub clip: u32,
    pub reload_time: f32,
    pub reload_empty_time: f32,
    pub damage_near: f32,
    pub damage_far: f32,
    pub near_dist: f32,
    pub far_dist: f32,
    pub spread_hip: f32,
    pub spread_ads: f32,
    pub kick_hip: f32,
    pub kick_ads: f32,
    pub kick_max_hip: f32,
    pub kick_max_ads: f32,
    pub decay_rate: f32,
    pub decay_delay: f32,
    pub zoom_fov: f32,
    pub zoom_time: f32,
    pub punch_pitch: (f32, f32),
    pub punch_yaw: (f32, f32),
    /// Damage against non-Titans (pilots).
    pub damage_near_pilot: f32,
    pub damage_far_pilot: f32,
    pub explosion_damage: f32,
    /// What an NPC deals with this weapon (npc_damage_*; falls back to the player numbers).
    pub npc_damage_near: f32,
    pub npc_damage_far: f32,
    pub npc_damage_near_pilot: f32,
    pub npc_damage_far_pilot: f32,
    pub npc_explosion_damage_heavy: f32,
    pub explosion_inner_radius: f32,
    pub explosion_radius: f32,
    /// damage_flags has DF_STOPS_TITAN_REGEN.
    pub stops_regen: bool,
    /// impact_effect_table: which `scripts/impacts/<table>.txt` the hits play.
    pub impact_table: &'static str,
    pub playermodel: String,
    // AI use (npc_* keys and proficiency_*).
    pub npc_min_burst: u32,
    pub npc_max_burst: u32,
    pub npc_rest_min: f32,
    pub npc_rest_max: f32,
    pub npc_max_range: f32,
    pub npc_min_engage_titan: f32,
    /// Spread scale and bias per eWeaponProficiency (poor..perfect).
    pub proficiency: [(f32, f32); 5],
    // Pilot weapon handling.
    pub printname: String,
    pub viewmodel: String,
    /// fire_mode "auto" (hold to fire) vs semi-auto (one shot or burst per pull).
    pub automatic: bool,
    pub burst_count: u32,
    pub burst_delay: f32,
    /// Projectile speed (units/s) for bolt weapons; None for hitscan.
    pub projectile_speed: Option<f32>,
    /// Fraction of gravity applied to projectiles.
    pub gravity: f32,
    pub shotgun: bool,
    pub bolt_spread: (f32, f32),
    pub headshot_scale: f32,
    /// `critical_hit`: hits on a Titan's crit boxes are critical, times `critical_hit_damage_scale`.
    pub crit: bool,
    pub crit_scale: f32,
    /// `titanarmor_critical_hit_required`: Titan-armour damage only on a crit.
    pub titanarmor_crit_required: bool,
    /// Mods applied (`Mods` block names).
    pub mods: Vec<String>,
    /// Mods the script defines.
    pub available_mods: Vec<String>,
    /// `fast_swap_to` (Quick Swap): switching to this gun is instant.
    pub fast_swap: bool,
    /// `primary_fire_does_not_block_sprint` (Gunrunner): fire while sprinting.
    pub fire_while_sprinting: bool,
    pub zoom_time_out: f32,
    pub deploy_time: f32,
    pub holster_time: f32,
    /// Seconds to bring the gun back up after sprinting before it can fire.
    pub raise_time: f32,
    /// No magazine (`ammo_clip_size` 0): `clip` is the stockpile and there is no reload.
    pub no_reload: bool,
    pub charge: ChargeDef,
    /// Smart ammo (Smart Pistol, Archer), see `smartammo`.
    pub smart: Option<crate::smartammo::SmartDef>,
    /// `attack_button_presses_ads`: the trigger also aims, and the shot waits for full zoom
    /// (the Archer's and Thunderbolt's `OnWeaponPrimaryAttack` refuse to fire below zoomFrac 1).
    pub attack_presses_ads: bool,
    /// `projectilemodel`.
    pub projectile_model: String,
    /// Recoil (`viewkick_*`) and viewmodel sway/bob (`sway_*`, `bob_*`).
    pub kick: crate::vmmotion::KickDef,
    pub motion: crate::vmmotion::MotionDef,
    /// Spread by stance (`spread_*`).
    pub spread: crate::spread::SpreadDef,
    /// The crosshair RUI (`RUI_CrosshairData` Crosshair_1 `ui`), e.g. `ui/crosshair_tri`.
    /// `ads_move_speed_scale`: movement speed while fully aimed down sights.
    pub ads_move_scale: f32,
    /// Fire rate ramp (`fire_rate_max`, `_time_speedup`, `_time_cooldown`, `_use_ads`): the rate
    /// climbs from `fire_rate` to this while firing (Devotion, the XO-16 accelerator).
    pub fire_rate_max: f32,
    pub fire_rate_speedup: f32,
    pub fire_rate_cooldown: f32,
    pub fire_rate_by_ads: bool,
    /// The third falloff step (`damage_very_far_distance`, `damage_very_far_value`,
    /// `_titanarmor`): damage keeps falling past `far_dist` to this at this distance.
    pub very_far_dist: f32,
    pub damage_very_far: f32,
    pub damage_very_far_pilot: f32,
    /// `reload_time_late1..3` / `reloadempty_time_late1..3`: how long a reload interrupted
    /// past that stage takes to resume (0: no such stage).
    pub reload_late: [f32; 3],
    pub reloadempty_late: [f32; 3],
    pub crosshair: String,
    /// Effects from the script: first-person and world muzzle flash and shell eject, the
    /// attachments they play at, and the tracers.
    pub fx: WeaponFx,
    /// `bodygroupN_name` / `bodygroupN_set`: which model of each named body part to show
    /// (sights, screens, scopes).
    pub bodygroups: Vec<(String, usize)>,
    /// `UiDataN` blocks switched on by `uiN_enable` (mods switch sights' screens on and off):
    /// the RUI and the model's RUI mesh it draws on.
    pub rui: Vec<(String, String)>,
    /// The weapon with its burn mod (amped through an A-Wall); player weapons only.
    pub burn: Option<Box<WeaponDef>>,
    /// `bodygroup_ads_scope_name/set`: the scope interior shown at the eye in ADS, and
    /// `zoom_scope_frac_start/end`: the zoom at which it appears and the rest of the gun goes.
    pub ads_scope: Option<(String, usize)>,
    pub scope_frac: (f32, f32),
    /// `viewmodel_offset_hip` / `viewmodel_offset_ads` (right, forward, up): where the
    /// viewmodel sits relative to its authored pose; sight mods set the ADS one so their
    /// optic, not the iron sights, lines up with the eye.
    pub vm_offset: (Vec3, Vec3),
}

/// A weapon's charge-up (`charge_*` keys); `time` 0 means it has none.
#[derive(Debug, Clone, Default)]
pub struct ChargeDef {
    /// Seconds from empty to full.
    pub time: f32,
    /// Discrete levels across the charge (`charge_levels`, 0 when continuous).
    pub levels: u32,
    /// Seconds from full to empty once charging stops.
    pub cooldown: f32,
    /// ADS charges the weapon (Plasma Railgun) instead of the trigger.
    pub by_ads: bool,
    /// Keeps charging once started even if the trigger is let go (`charge_require_input` 0).
    pub require_input: bool,
    /// Extra damage per charge level (`damage_additional_bullets[_titanarmor]`).
    pub extra_per_level: f32,
}

/// A charge in progress (see `ChargeDef`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Charge {
    /// 0 (empty) to 1 (full).
    pub frac: f32,
    /// Charging last frame (to start the wind-up and wind-down sounds once).
    pub charging: bool,
    /// Charging continues without input (`charge_require_input` 0).
    pub latched: bool,
}

impl Charge {
    /// Completed charge levels (0..=levels); continuous charges report 0 until full, then 1.
    pub fn level(&self, def: &ChargeDef) -> u32 {
        let n = def.levels.max(1);
        ((self.frac * n as f32 + 1e-4).floor() as u32).min(n)
    }

    /// Advance the charge. Returns the level reached when it goes up.
    pub fn update(&mut self, def: &ChargeDef, charging: bool, dt: f32) -> Option<u32> {
        let before = self.level(def);
        self.frac = if charging {
            (self.frac + dt / def.time.max(0.01)).min(1.0)
        } else if def.cooldown > 0.0 {
            (self.frac - dt / def.cooldown).max(0.0)
        } else {
            0.0
        };
        let after = self.level(def);
        (after > before).then_some(after)
    }
}

/// A weapon's effect names (`fx_*`, `tracer_effect*`); empty when the script has none.
#[derive(Debug, Clone, Default)]
pub struct WeaponFx {
    pub muzzle_view: String,
    pub muzzle_world: String,
    pub muzzle_attach: String,
    pub shell_view: String,
    pub shell_world: String,
    pub shell_attach: String,
    pub tracer_view: String,
    pub tracer_world: String,
}

impl WeaponDef {
    /// The fire rate at `ramp` (0..1) of the way to `fire_rate_max`.
    pub fn rate_at(&self, ramp: f32) -> f32 {
        if self.fire_rate_max > 0.0 {
            self.fire_rate + (self.fire_rate_max - self.fire_rate) * ramp.clamp(0.0, 1.0)
        } else {
            self.fire_rate
        }
    }

    /// The ramp after `dt`: up over `fire_rate_max_time_speedup` while firing, down over
    /// `_time_cooldown` (0: at once) otherwise; with `_use_ads` it follows the zoom instead.
    pub fn ramp_step(&self, ramp: f32, firing: bool, ads: f32, dt: f32) -> f32 {
        if self.fire_rate_max <= 0.0 {
            return 0.0;
        }
        if self.fire_rate_by_ads {
            return ads;
        }
        if firing {
            (ramp + dt / self.fire_rate_speedup.max(1e-3)).min(1.0)
        } else if self.fire_rate_cooldown <= 0.0 {
            0.0
        } else {
            (ramp - dt / self.fire_rate_cooldown).max(0.0)
        }
    }

    /// The `body` selection for `model` from the script's bodygroups.
    pub fn body_for(&self, model: &tf_assets::mdl::Model) -> Vec<usize> {
        let mut body = vec![0; model.bodyparts.len()];
        // TF_BODY="part=set,part=set" overrides bodygroups (debugging).
        let extra: Vec<(String, usize)> = std::env::var("TF_BODY")
            .map(|v| v.split(',').filter_map(|kv| kv.split_once('=')).filter_map(|(k, n)| Some((k.trim().to_string(), n.trim().parse().ok()?))).collect())
            .unwrap_or_default();
        for (name, set) in self.bodygroups.iter().chain(&self.ads_scope).chain(&extra) {
            match model.bodyparts.iter().position(|b| b.name.eq_ignore_ascii_case(name)) {
                Some(i) => body[i] = (*set).min(model.bodyparts[i].num_models.saturating_sub(1)),
                None => log::debug!("{}: no body part {name}", model.name),
            }
        }
        body
    }

    /// Load a weapon script (`scripts/weapons/<name>.txt`), preferring SP_BASE values.
    pub fn load(name: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        Self::load_modded(name, &[], read)
    }

    /// Load a weapon script with mods from its `Mods` block applied (see
    /// `PlayerSettings::with_mods`); unknown mod names are ignored.
    pub fn load_modded(name: &str, mods: &[&str], read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let base = PlayerSettings::load(&format!("scripts/weapons/{name}.txt"), true, read).unwrap_or_default();
        let available_mods = base.mod_names();
        let mods: Vec<&str> = mods.iter().copied().filter(|m| available_mods.iter().any(|a| a.eq_ignore_ascii_case(m))).collect();
        let s = base.with_mods(&mods);
        let f = |k: &str, d: f32| -> f32 {
            s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).and_then(|v| v.parse().ok()).unwrap_or(d)
        };
        Self {
            name: name.to_string(),
            fire_rate: f("fire_rate", 12.0),
            clip: if f("ammo_clip_size", 30.0) > 0.0 { f("ammo_clip_size", 30.0) as u32 } else { f("ammo_default_total", 10.0) as u32 },
            no_reload: f("ammo_clip_size", 30.0) <= 0.0,
            charge: ChargeDef {
                time: f("charge_time", 0.0),
                levels: f("charge_levels", 0.0) as u32,
                cooldown: f("charge_cooldown_time", 0.0),
                by_ads: f("charge_is_triggered_by_ADS", 0.0) > 0.0,
                require_input: f("charge_require_input", 1.0) > 0.0,
                extra_per_level: f("damage_additional_bullets_titanarmor", f("damage_additional_bullets", 0.0)),
            },
            reload_time: f("reload_time", 2.6),
            reload_empty_time: f("reloadempty_time", 2.6),
            // Titan armour damage; weapons without it hit Titans with their normal damage.
            damage_near: f("damage_near_value_titanarmor", f("damage_near_value", 120.0)),
            damage_far: f("damage_far_value_titanarmor", f("damage_far_value", 100.0)),
            near_dist: f("damage_near_distance", 1200.0),
            far_dist: f("damage_far_distance", 2000.0),
            spread_hip: f("spread_stand_hip", 1.2),
            spread_ads: f("spread_stand_ads", 0.4),
            kick_hip: f("spread_kick_on_fire_stand_hip", 0.4),
            kick_ads: f("spread_kick_on_fire_stand_ads", 0.3),
            kick_max_hip: f("spread_max_kick_stand_hip", 8.0),
            kick_max_ads: f("spread_max_kick_stand_ads", 2.0),
            decay_rate: f("spread_decay_rate", 6.5),
            decay_delay: f("spread_decay_delay", 0.15),
            zoom_fov: f("zoom_fov", 33.0),
            zoom_time: f("zoom_time_in", 0.3),
            punch_pitch: (f("viewkick_pitch_base", -0.2), f("viewkick_pitch_random", 0.5)),
            punch_yaw: (f("viewkick_yaw_base", 0.2), f("viewkick_yaw_random", 0.15)),
            damage_near_pilot: f("damage_near_value", 20.0),
            damage_far_pilot: f("damage_far_value", 15.0),
            explosion_damage: f("explosion_damage", 0.0),
            npc_damage_near: f("npc_damage_near_value_titanarmor", f("damage_near_value_titanarmor", f("damage_near_value", 120.0))),
            npc_damage_far: f("npc_damage_far_value_titanarmor", f("damage_far_value_titanarmor", f("damage_far_value", 100.0))),
            npc_damage_near_pilot: f("npc_damage_near_value", f("damage_near_value", 20.0)),
            npc_damage_far_pilot: f("npc_damage_far_value", f("damage_far_value", 15.0)),
            npc_explosion_damage_heavy: f("npc_explosion_damage_heavy_armor", f("explosion_damage_heavy_armor", 0.0)),
            explosion_inner_radius: f("explosion_inner_radius", 0.0),
            explosion_radius: f("explosionradius", 0.0),
            stops_regen: s.get(".damage_flags").is_some_and(|v| v.contains("DF_STOPS_TITAN_REGEN")),
            impact_table: intern(s.get(".impact_effect_table").as_deref().filter(|t| !t.is_empty()).unwrap_or("default")),
            playermodel: s.get(".playermodel").unwrap_or_default().to_string(),
            npc_min_burst: f("npc_min_burst", 3.0) as u32,
            npc_max_burst: f("npc_max_burst", 6.0) as u32,
            npc_rest_min: f("npc_rest_time_between_bursts_min", 0.5),
            npc_rest_max: f("npc_rest_time_between_bursts_max", 1.0),
            npc_max_range: f("npc_max_range", 5000.0),
            npc_min_engage_titan: f("npc_min_engage_range_heavy_armor", 0.0),
            proficiency: [
                (f("proficiency_poor_spreadscale", 5.0), f("proficiency_poor_bias", 0.0)),
                (f("proficiency_average_spreadscale", 3.5), f("proficiency_average_bias", 0.2)),
                (f("proficiency_good_spreadscale", 3.0), f("proficiency_good_bias", 0.5)),
                (f("proficiency_very_good_spreadscale", 2.0), f("proficiency_very_good_bias", 0.75)),
                // Engine default when the script leaves it out.
                (f("proficiency_perfect_spreadscale", 1.0), f("proficiency_perfect_bias", 1.0)),
            ],
            printname: s.get(".printname").unwrap_or_default().trim_start_matches('#').to_string(),
            viewmodel: s.get(".viewmodel").unwrap_or_default().to_string(),
            automatic: s.get("sp_base.fire_mode").or_else(|| s.get(".fire_mode")).is_none_or(|m| m == "auto"),
            burst_count: f("burst_fire_count", 0.0) as u32,
            burst_delay: f("burst_fire_delay", 0.0),
            projectile_speed: {
                let p = f("projectile_launch_speed", 0.0).max(f("bolt_speed", 0.0));
                (p > 0.0).then_some(p)
            },
            gravity: if f("bolt_gravity_enabled", 0.0) > 0.0 { f("bolt_gravity_amount", 0.5) } else { f("projectile_gravity_scale", 0.0) },
            shotgun: s.get(".damage_flags").is_some_and(|v| v.contains("DF_SHOTGUN")),
            bolt_spread: (f("bolt_spread_max", 0.05), f("bolt_spread_min", 0.025)),
            // The Smart Pistol leaves the head shot scale out; its smart_ammo_target_npc_lock_factor
            // is that scale ("smart pistol does head shots at 2x damage").
            headshot_scale: f("damage_headshot_scale", f("smart_ammo_target_npc_lock_factor", 1.0)),
            crit: f("critical_hit", 0.0) > 0.0,
            crit_scale: f("critical_hit_damage_scale", 1.0),
            titanarmor_crit_required: f("titanarmor_critical_hit_required", 0.0) > 0.0,
            mods: mods.iter().map(|m| m.to_string()).collect(),
            available_mods,
            fast_swap: f("fast_swap_to", 0.0) > 0.0,
            fire_while_sprinting: f("primary_fire_does_not_block_sprint", 0.0) > 0.0,
            zoom_time_out: f("zoom_time_out", f("zoom_time_in", 0.3)),
            deploy_time: f("deploy_time", 0.6),
            holster_time: f("holster_time", 0.4),
            raise_time: f("raise_time", 0.3),
            smart: crate::smartammo::SmartDef::from_settings(name, &mods.iter().map(|m| m.to_string()).collect::<Vec<_>>(), &s),
            attack_presses_ads: f("attack_button_presses_ads", 0.0) > 0.0,
            projectile_model: s.get(".projectilemodel").unwrap_or_default().to_string(),
            kick: crate::vmmotion::KickDef::from_settings(&s),
            motion: crate::vmmotion::MotionDef::from_settings(&s),
            spread: crate::spread::SpreadDef::from_settings(&s),
            crosshair: s.get("rui_crosshairdata.ui").unwrap_or("").trim().to_string(),
            ads_move_scale: f("ads_move_speed_scale", 1.0),
            reload_late: [f("reload_time_late1", 0.0), f("reload_time_late2", 0.0), f("reload_time_late3", 0.0)],
            reloadempty_late: [f("reloadempty_time_late1", 0.0), f("reloadempty_time_late2", 0.0), f("reloadempty_time_late3", 0.0)],
            very_far_dist: f("damage_very_far_distance", f("damage_far_distance", 2000.0)),
            damage_very_far: f("damage_very_far_value_titanarmor", f("damage_very_far_value", f("damage_far_value_titanarmor", f("damage_far_value", 100.0)))),
            damage_very_far_pilot: f("damage_very_far_value", f("damage_far_value", 100.0)),
            fire_rate_max: f("fire_rate_max", 0.0),
            fire_rate_speedup: f("fire_rate_max_time_speedup", 0.0),
            fire_rate_cooldown: f("fire_rate_max_time_cooldown", 0.0),
            fire_rate_by_ads: s.get("sp_base.fire_rate_max_use_ads").or_else(|| s.get(".fire_rate_max_use_ads")).is_some_and(|v| matches!(v.trim(), "1" | "true")),
            fx: {
                let g = |k: &str| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none")).unwrap_or_default();
                WeaponFx {
                    muzzle_view: g("fx_muzzle_flash_view"),
                    muzzle_world: g("fx_muzzle_flash_world"),
                    muzzle_attach: g("fx_muzzle_flash_attach"),
                    shell_view: g("fx_shell_eject_view"),
                    shell_world: g("fx_shell_eject_world"),
                    shell_attach: g("fx_shell_eject_attach"),
                    tracer_view: g("tracer_effect_first_person"),
                    tracer_world: g("tracer_effect"),
                }
            },
            ads_scope: {
                let get = |k: &str| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).map(|v| v.trim().to_string());
                get("bodygroup_ads_scope_name").zip(get("bodygroup_ads_scope_set").and_then(|v| v.parse().ok())).filter(|(_, n)| *n > 0)
            },
            scope_frac: (f("zoom_scope_frac_start", 0.2), f("zoom_scope_frac_end", 0.7)),
            vm_offset: {
                let v = |k: &str| {
                    let t = s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).unwrap_or_default();
                    let n: Vec<f32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if n.len() == 3 { Vec3::new(n[0], n[1], n[2]) } else { Vec3::ZERO }
                };
                (v("viewmodel_offset_hip"), v("viewmodel_offset_ads"))
            },
            bodygroups: (1..=16)
                .filter_map(|i| {
                    let get = |k: &str| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).map(|v| v.trim().to_string());
                    Some((get(&format!("bodygroup{i}_name"))?, get(&format!("bodygroup{i}_set"))?.parse().ok()?))
                })
                .collect(),
            burn: None,
            rui: (1..=10)
                .filter_map(|i| {
                    let on = s.get(&format!(".ui{i}_enable")).or_else(|| s.get(&format!("sp_base.ui{i}_enable")))?.trim() == "1";
                    on.then_some(())?;
                    Some((s.get(&format!("uidata{i}.ui"))?.trim().to_ascii_lowercase(), s.get(&format!("uidata{i}.mesh"))?.trim().to_ascii_lowercase()))
                })
                .collect(),
        }
    }

    /// Load a weapon the player uses, with the mods `TF_MODS` names for it (see `env_mods`).
    pub fn load_player(name: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let mut def = Self::load_player_inner(name, read);
        // The amped version (its `burn_mod_*`), for rounds through an A-Wall.
        let want = env_mods(name);
        if let Some(burn) = def.available_mods.iter().find(|m| m.starts_with("burn_mod")).cloned() {
            let mut refs: Vec<&str> = want.iter().map(String::as_str).collect();
            refs.push(&burn);
            def.burn = Some(Box::new(Self::load_modded(name, &refs, read)));
        }
        def
    }

    fn load_player_inner(name: &str, read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let want = env_mods(name);
        if want.is_empty() {
            return Self::load(name, read);
        }
        let refs: Vec<&str> = want.iter().map(String::as_str).collect();
        let def = Self::load_modded(name, &refs, read);
        for m in &want {
            if !def.mods.iter().any(|a| a.eq_ignore_ascii_case(m)) {
                log::warn!("{name} has no mod {m:?}; its mods: {}", def.available_mods.join(", "));
            }
        }
        log::info!("{name} with mods {:?}", def.mods);
        def
    }
}

/// Mods for a weapon from `TF_MODS`: `weapon:mod,mod;weapon:mod`, with `*` for every weapon,
/// e.g. `TF_MODS="mp_weapon_r97:pas_fast_reload,extended_ammo;mp_weapon_smart_pistol:enhanced_targeting"`.
pub fn env_mods(weapon: &str) -> Vec<String> {
    let Ok(v) = std::env::var("TF_MODS") else { return Vec::new() };
    parse_mods(&v, weapon)
}

fn parse_mods(spec: &str, weapon: &str) -> Vec<String> {
    spec.split(';')
        .filter_map(|w| w.split_once(':'))
        .filter(|(n, _)| n.trim() == "*" || n.trim().eq_ignore_ascii_case(weapon))
        .flat_map(|(_, m)| m.split(',').map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// BT's loadouts (the campaign lets BT use every Titan's primary): weapon script, fire
/// sounds, and what the weapon's .nut does when it fires (projectile speed, bolt pattern).
pub struct TitanGun {
    pub id: &'static str,
    pub kit: &'static str,
    pub first: &'static [&'static str],
    pub shot: &'static [&'static str],
    pub tail: &'static [&'static str],
    /// Bolt speed from the weapon's script (None: hitscan).
    pub projectile: Option<f32>,
    pub gravity: f32,
    /// Bolt offsets (up, right) and their scale.
    pub pellets: Option<(&'static [[f32; 2]], f32)>,
    /// Seconds of barrel spin before firing (Predator Cannon).
    pub spinup: f32,
}

const LEADWALL_BOLTS: [[f32; 2]; 8] = [[0.2, 0.8], [0.2, -0.8], [-0.2, 0.65], [-0.2, -0.65], [0.2, 0.2], [0.2, -0.2], [-0.2, 0.2], [-0.2, -0.2]];

pub const TITAN_ARSENAL: &[TitanGun] = &[
    TitanGun { id: "mp_titanweapon_xo16_shorty", kit: "EXPEDITION", first: &["wpn_xo16_1p_wpnfire_firstshot_core"], shot: &["wpn_xo16_1p_wpnfire_secondshot_core"], tail: &["wpn_xo16_1p_wpnfire_tail_core"], projectile: None, gravity: 0.0, pellets: None, spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_meteor", kit: "SCORCH", first: &[], shot: &["wpn_meteor_1p_shot_lr"], tail: &[], projectile: Some(2200.0), gravity: 0.3, pellets: None, spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_particle_accelerator", kit: "ION", first: &[], shot: &["wpn_particleaccel_1p_firstshot_lr"], tail: &[], projectile: Some(8000.0), gravity: 0.0, pellets: None, spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_sniper", kit: "NORTHSTAR", first: &[], shot: &["wpn_titansniper_1p_wpnfire_level2_6ch"], tail: &["wpn_titansniper_1p_wpnfire_uber_tail"], projectile: Some(10000.0), gravity: 0.0, pellets: None, spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_leadwall", kit: "RONIN", first: &[], shot: &["wpn_leadwall_1p_shot_lr"], tail: &[], projectile: Some(4000.0), gravity: 0.0, pellets: Some((&LEADWALL_BOLTS, 0.05 * 1.45)), spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_sticky_40mm", kit: "TONE", first: &[], shot: &["wpn_40mm_1p_wpnfire_reduced"], tail: &["wpn_40mm_1p_wpnfire_tail"], projectile: Some(6000.0), gravity: 0.1, pellets: None, spinup: 0.0 },
    TitanGun { id: "mp_titanweapon_predator_cannon", kit: "LEGION", first: &["wpn_xo16_1p_wpnfire_firstshot_core"], shot: &["wpn_xo16_1p_wpnfire_secondshot_core"], tail: &["wpn_predator_1p_barrelspin_end"], projectile: None, gravity: 0.0, pellets: None, spinup: 0.45 },
    TitanGun { id: "mp_titanweapon_rocketeer_rocketstream", kit: "BRUTE", first: &[], shot: &["wpn_trl_1p_fire_shot_lr"], tail: &["wpn_trl_1p_fire_rocket_tail"], projectile: Some(2500.0), gravity: 0.0, pellets: None, spinup: 0.0 },
];

/// Smart Core's lock-on cone (half angle, degrees).
const SMART_CORE_CONE: f32 = 20.0;

/// BT's current loadout (index into `TITAN_ARSENAL`), switched with 1-8 in the Titan.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitanKit(pub usize);

/// BT's gun hand (`ja_c_propGun`), where the third-person weapon is attached.
#[derive(Resource, Clone, Copy)]
pub struct BtHand(pub Entity);

/// Weapon state, on the player's Titan entity.
#[derive(Component)]
pub struct Weapon {
    pub def: WeaponDef,
    pub ammo: u32,
    pub reload_left: f32,
    pub cooldown: f32,
    pub spread_kick: f32,
    pub since_fire: f32,
    /// The current spread cone (degrees, full width), for the crosshair.
    pub cur_spread: f32,
    pub ads: f32,
    pub shots: u32,
    pub hits: u32,
    /// A burst's fire sound is playing; its tail plays when the trigger lets go.
    firing_audio: bool,
    rng: u64,
    /// First-person viewmodel: placement entity and actor; BT's arms are bone-merged onto it.
    pub viewmodel: Option<(Entity, Entity)>,
    /// Gun in BT's hand: actor entity (its muzzle bone is used for third-person tracers).
    pub world_gun: Option<Entity>,
    vm_seq: VmSeq,
    vm_time: f32,
    idle_time: f32,
    /// Index into TITAN_ARSENAL.
    pub arsenal: usize,
    trigger_held: bool,
    spin: f32,
    pub charge: Charge,
    /// How far the fire rate has ramped toward `fire_rate_max`.
    pub rate_ramp: f32,
    /// Sway pivots (filled on first use) and sway/bob state of the viewmodel.
    pivots: Option<[(String, Vec3); 2]>,
    motion: crate::vmmotion::VmMotion,
    /// Viewmodel muzzle and shell attachments, and the world gun's muzzle (filled on first use).
    fx_atts: Option<[Option<(String, Transform)>; 3]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VmSeq {
    Draw,
    Idle,
    Fire,
    Reload,
    ReloadEmpty,
    Sprint,
    /// `sprintraise_seq` (ACT_VM_RAISE_FROM_SPRINT) when a sprint ends.
    SprintRaise,
    Melee,
    /// A punch thrown while dashing (`melee_dash_seq`).
    MeleeDash,
    /// Legion's Ammo Swap (`ammo_swap_seq`).
    AmmoSwap,
}

impl Weapon {
    /// The Predator Cannon is in its long-range mode (the `LongRangeAmmo` mod).
    pub fn long_range(&self) -> bool {
        self.def.mods.iter().any(|m| m.eq_ignore_ascii_case("LongRangeAmmo"))
    }
    pub fn play_ammo_swap(&mut self) {
        self.vm_seq = VmSeq::AmmoSwap;
        self.vm_time = 0.0;
    }
    pub fn new(def: WeaponDef, viewmodel: Option<(Entity, Entity)>, world_gun: Option<Entity>) -> Self {
        Self {
            ammo: def.clip,
            def,
            reload_left: 0.0,
            cooldown: 0.0,
            spread_kick: 0.0,
            since_fire: 10.0,
            cur_spread: 0.0,
            ads: 0.0,
            shots: 0,
            firing_audio: false,
            arsenal: 0,
            trigger_held: false,
            spin: 0.0,
            charge: Charge::default(),
            hits: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
            viewmodel,
            world_gun,
            vm_seq: VmSeq::Draw,
            vm_time: 0.0,
            idle_time: 0.0,
            pivots: None,
            motion: Default::default(),
            fx_atts: None,
            rate_ramp: 0.0,
        }
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*, plenty for spread jitter.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }
}

/// BT's stance for spread.
fn titan_stance(s: &tf_sim::titan::TitanState) -> crate::spread::Stance {
    use crate::spread::Stance;
    if !s.on_ground {
        Stance::Air
    } else if s.sprinting {
        Stance::Sprint
    } else if (s.vel.x * s.vel.x + s.vel.y * s.vel.y).sqrt() > 20.0 {
        Stance::Run
    } else {
        Stance::Stand
    }
}

/// View punch (recoil) added on top of the player's aim; springs back to zero.
pub use crate::vmmotion::ViewPunch;

/// Short-lived muzzle and blast lights.
#[derive(Component)]
pub struct Fx {
    pub life: f32,
    pub max: f32,
    /// Scale at spawn; shrinks while fading (tracers keep their length on Z).
    pub base: Vec3,
    pub keep_z: bool,
}

/// Shared effect assets: the translucent discs that stand in for the game's refracting
/// Vortex / Thermal Shield domes (the game draws those with a refraction material).
#[derive(Resource)]
pub struct FxAssets {
    pub vortex_disc: (Handle<Mesh>, Handle<StandardMaterial>),
    pub heat_disc: (Handle<Mesh>, Handle<StandardMaterial>),
}

pub fn setup_fx(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let mesh = meshes.add(Circle::new(crate::combat::VORTEX_RADIUS * crate::player::UNIT));
    let tex = images.add(shield_texture(256));
    let disc = |materials: &mut Assets<StandardMaterial>, base: Color, emissive: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: base,
            base_color_texture: Some(tex.clone()),
            emissive,
            emissive_texture: Some(tex.clone()),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    let vortex = disc(&mut materials, Color::srgb(0.35, 0.75, 1.0), LinearRgba::rgb(0.5, 1.3, 2.6));
    let heat = disc(&mut materials, Color::srgb(1.0, 0.55, 0.2), LinearRgba::rgb(2.6, 0.9, 0.2));
    commands.insert_resource(FxAssets { vortex_disc: (mesh.clone(), vortex), heat_disc: (mesh, heat) });
}

/// The shield discs' look (a stand-in for the game's refracting dome): nearly clear in the
/// middle so the view stays readable, a faint hex lattice, and a bright rim that fades
/// inward, as the dome's edge reads in the game. Greyscale, tinted by the material.
fn shield_texture(n: u32) -> Image {
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            // Rim: a thin bright edge with a soft glow inside it.
            let edge = (-((r - 0.97) / 0.025).powi(2)).exp();
            let glow = ((r - 0.4) / 0.6).clamp(0.0, 1.0).powi(2) * 0.4;
            // Hex lattice: distance to the nearest cell edge of a hexagonal grid.
            let (hx, hy) = (u * 9.0, v * 9.0);
            let q = Vec2::new(hx * 2.0 / 3.0_f32.sqrt(), hy - hx / 3.0_f32.sqrt());
            let f = |a: f32| (a - a.round()).abs();
            let line = f(q.x).min(f(q.y)).min(f(q.x + q.y));
            let hex = (1.0 - (line / 0.07).min(1.0)) * 0.22 * (0.35 + 0.65 * r);
            let base = 0.05;
            let a = if r > 1.0 { 0.0 } else { (edge + glow + hex + base).min(1.0) };
            let c = (a * 255.0) as u8;
            px.extend_from_slice(&[c, c, c, 255]);
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    img
}

/// Shield discs turn slowly and breathe, so they read as energy rather than glass.
#[derive(Component)]
pub struct ShieldShimmer;

pub fn shimmer_shields(time: Res<Time>, mut q: Query<&mut Transform, With<ShieldShimmer>>) {
    let t = time.elapsed_secs();
    for mut tf in &mut q {
        tf.rotation = Quat::from_rotation_z(t * 0.6);
        tf.scale = Vec3::splat(1.0 + 0.015 * (t * 7.0).sin());
    }
}

/// The script's muzzle flash (and first-person shell eject) for a shot: the `_FP` systems
/// at the viewmodel's attachments in the cockpit, the world system at the gun in BT's hand
/// otherwise. False when the game's systems can't play it (the caller falls back).
fn titan_shot_fx(commands: &mut Commands, w: &Weapon, cockpit: bool, actors: &Query<&Actor>, globals: &Query<&GlobalTransform>) -> bool {
    let Some(atts) = w.fx_atts.as_ref() else { return false };
    let at = |actor: Option<Entity>, i: usize| actor.zip(atts[i].as_ref()).and_then(|(a, att)| crate::pilotweapon::vm_attachment(actors, globals, a, att));
    if cockpit {
        let Some((p, r)) = at(w.viewmodel.map(|v| v.1), 0).filter(|_| !w.def.fx.muzzle_view.is_empty()) else { return false };
        crate::pfx::emit_named_vm_frame(commands, &w.def.fx.muzzle_view, p, r);
        if let Some((p, r)) = at(w.viewmodel.map(|v| v.1), 1).filter(|_| !w.def.fx.shell_view.is_empty()) {
            crate::pfx::emit_named_vm_frame(commands, &w.def.fx.shell_view, p, r);
        }
    } else {
        let Some((p, r)) = at(w.world_gun, 2).filter(|_| !w.def.fx.muzzle_world.is_empty()) else { return false };
        crate::pfx::emit_named_frame(commands, &w.def.fx.muzzle_world, p, r);
    }
    true
}

fn muzzle_world(actors: &Query<&Actor>, globals: &Query<&GlobalTransform>, actor: Option<Entity>) -> Option<Vec3> {
    let a = actors.get(actor?).ok()?;
    globals.get(a.joint("muzzle_flash")?).ok().map(|g| g.translation())
}

/// Fire, reload and ADS. Runs after the movement simulation.
#[allow(clippy::too_many_arguments)]
pub fn weapon_fire(
    mut commands: Commands,
    time: Res<Time>,
    mut input: ResMut<PlayerInput>,
    mode: Res<CameraMode>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    fx: Res<FxAssets>,
    mut punch_res: ResMut<ViewPunch>,
    (auto, control, mut scratch, mut enemy_walls, springs, pfx): (Res<crate::autotitan::AutoTitan>, Res<crate::pilotctl::Control>, Local<ViewPunch>, ResMut<crate::titankit::EnemyWalls>, Res<crate::vmmotion::WeaponSprings>, Option<Res<crate::pfx::Pfx>>),
    mut titans: Query<(
        &PlayerTitan,
        &mut Weapon,
        Option<&mut crate::abilities::TitanCore>,
        Option<&crate::combat::Vortex>,
        Option<&crate::combat::Melee>,
        Option<&crate::combat::TitanHealth>,
        Option<&crate::titankit::KitState>,
    )>,
    mut dummies: Query<&mut Enemy>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    hitboxes: Option<Res<crate::hitbox::EnemyHitboxes>>,
) {
    let dt = time.delta_secs();
    let game_fx = pfx.as_deref().is_some_and(|p| p.ready()) && std::env::var_os("TF_OLD_FX").is_none();
    let no_boxes = crate::hitbox::EnemyHitboxes::default();
    let hitboxes = hitboxes.as_deref().unwrap_or(&no_boxes);
    // On foot BT shoots on his own (autotitan.rs) from his own aim; recoil only kicks the
    // view when you're in the cockpit.
    let piloted = *control == crate::pilotctl::Control::Titan;
    let auto_fire = *control == crate::pilotctl::Control::Pilot && auto.fire;
    let punch: &mut ViewPunch = if piloted { &mut punch_res } else { &mut scratch };
    // Recoil springs back.
    if let Ok((_, w, ..)) = titans.single() {
        punch.step(dt, Some(&w.def.kick));
    }
    let Ok((titan, mut w, mut core, vortex, melee, health, kit)) = titans.single_mut() else { return };
    // The left hand is busy during Vortex and punches, Sword Block holds the sword up; dead
    // Titans don't shoot.
    let busy = vortex.is_some_and(|v| v.active) || melee.is_some_and(|m| m.t.is_some()) || health.is_some_and(|h| h.dead_for.is_some()) || kit.is_some_and(|k| k.blocking);
    let frozen = *mode == CameraMode::Free || busy;
    // Burst Core: faster fire, fixed damage, no ammo use.
    let burst = core.as_ref().is_some_and(|c| c.active() && c.kind == crate::titankit::CoreKind::Burst);
    // Smart Core: every shot locks onto the target nearest the crosshair.
    let smart = core.as_ref().is_some_and(|c| c.active() && c.kind == crate::titankit::CoreKind::Smart);
    let s = &titan.state;
    let fire = (input.fire && piloted) || auto_fire;
    let (vyaw, vpitch) = if piloted { (input.yaw, input.pitch) } else { (s.yaw, s.pitch) };

    let ads_target = if input.ads && !s.sprinting && !w.reloading() && !frozen { 1.0 } else { 0.0 };
    // zoom_time_in going in, zoom_time_out coming back out.
    let rate = dt / if ads_target >= w.ads { w.def.zoom_time } else { w.def.zoom_time_out }.max(0.05);
    w.ads = if ads_target > w.ads { (w.ads + rate).min(ads_target) } else { (w.ads - rate).max(ads_target) };

    // Allow at most one frame of catch-up, so idle time doesn't bank shots.
    w.cooldown = (w.cooldown - dt).max(-dt);
    w.since_fire += dt;
    w.spread_kick = w.def.spread.decayed(w.spread_kick, w.since_fire, dt);
    let stance = titan_stance(s);
    w.cur_spread = w.def.spread.base(stance, w.ads) + w.spread_kick;
    // The looping fire sound ends with the weapon's tail (burst_or_looping_fire_sound_end).
    if w.firing_audio && w.since_fire > 1.6 / w.def.fire_rate.max(0.1) {
        w.firing_audio = false;
        audio::cue(&mut commands, Cue::TitanGun(w.arsenal as u8, audio::GunPart::Tail), None);
    }

    if w.reload_left > 0.0 {
        w.reload_left -= dt;
        if w.reload_left <= 0.0 {
            w.ammo = w.def.clip;
        }
    }
    let want_reload = (input.reload && w.ammo < w.def.clip) || (w.ammo == 0 && fire);
    input.reload = false;
    if want_reload && !w.reloading() && !frozen {
        let empty = w.ammo == 0;
        w.reload_left = if empty { w.def.reload_empty_time } else { w.def.reload_time };
        w.vm_seq = if empty { VmSeq::ReloadEmpty } else { VmSeq::Reload };
        // The reload sounds are the viewmodel animation's own sound events (actor_event_sounds).
        w.vm_time = 0.0;
    }

    let gun = &TITAN_ARSENAL[w.arsenal.min(TITAN_ARSENAL.len() - 1)];
    let pulled = fire && !w.trigger_held;
    w.trigger_held = fire;
    // The Predator Cannon's barrels spin up while the trigger is held.
    w.spin = if fire && !frozen { (w.spin + dt).min(gun.spinup) } else { 0.0 };
    // Charge weapons (the Plasma Railgun charges while aiming down sights; charge_time 2.25 s
    // over 5 levels, draining in charge_cooldown_time). Each level ticks.
    let cdef = w.def.charge.clone();
    if cdef.time > 0.0 {
        let can = !frozen && !burst && !w.reloading() && w.ammo > 0 && !s.sprinting && w.cooldown <= 0.0 && w.vm_seq != VmSeq::Draw;
        let charging = can && if cdef.by_ads { input.ads } else { fire };
        if charging && !w.charge.charging {
            audio::cue_for(&mut commands, Cue::RailgunWindUp, None, (1.0 - w.charge.frac) * cdef.time + 0.3);
        } else if !charging && w.charge.charging && w.charge.frac > 0.05 {
            audio::cue_for(&mut commands, Cue::RailgunWindDown, None, w.charge.frac * cdef.cooldown.max(0.3) + 0.2);
        }
        w.charge.charging = charging;
        if let Some(level) = w.charge.update(&cdef, charging, dt) {
            audio::cue(&mut commands, if level >= cdef.levels.max(1) { Cue::RailgunTickFinal } else { Cue::RailgunTick }, None);
        }
    }
    if !fire || frozen || (w.reloading() && !burst) || (w.ammo == 0 && !burst) || s.sprinting || w.vm_seq == VmSeq::Draw {
        return;
    }
    if w.spin < gun.spinup {
        return;
    }
    // Semi-automatic weapons fire once per pull (Burst Core is always automatic).
    if !w.def.automatic && !burst && !pulled {
        return;
    }
    // Trigger-charged weapons only fire once full.
    if cdef.time > 0.0 && !cdef.by_ads && !burst && w.charge.frac < 1.0 {
        return;
    }
    // GetTitanSniperChargeLevel: 1 + completed levels; each adds damage_additional_bullets
    // (FireSniper's bulletsToFire), and the kick grows with it.
    let charge_level = 1 + w.charge.level(&cdef);
    let mut shot_def = w.def.clone();
    if cdef.time > 0.0 && cdef.extra_per_level > 0.0 && !burst {
        let extra = cdef.extra_per_level * charge_level as f32;
        shot_def.damage_near += extra;
        shot_def.damage_far += extra;
    }
    let kick_scale = if cdef.time > 0.0 && cdef.by_ads {
        match charge_level {
            6.. => 1.0,
            5 => 0.75,
            4 => 0.6,
            3 => 0.45,
            2 => 0.3,
            _ => 0.2,
        }
    } else {
        1.0
    };
    w.rate_ramp = w.def.ramp_step(w.rate_ramp, fire && w.ammo > 0, w.ads, dt);
    let rate = if burst { crate::abilities::CORE_FIRE_RATE } else { w.def.rate_at(w.rate_ramp) };
    let interval = 1.0 / rate.max(0.1);
    while w.cooldown <= 0.0 && (w.ammo > 0 || burst) {
        w.cooldown += interval;
        if !burst {
            w.ammo -= 1;
        }
        w.shots += 1;
        let arsenal = w.arsenal as u8;
        if cdef.by_ads && !burst {
            // FireSniper's sound tiers: level > 4, > 3, > 2, else 1.
            let tier = match charge_level {
                5.. => 4,
                4 => 3,
                3 => 2,
                _ => 1,
            };
            audio::cue(&mut commands, Cue::RailgunFire(tier), None);
            log::info!("{} charge level {charge_level}: {:.0} damage", w.def.name, shot_def.damage_near);
            w.charge.frac = 0.0;
        } else {
            audio::cue(&mut commands, Cue::TitanGun(arsenal, if w.firing_audio { audio::GunPart::Shot } else { audio::GunPart::First }), None);
            w.firing_audio = true;
            if cdef.time > 0.0 {
                w.charge.frac = 0.0;
            }
        }
        w.since_fire = 0.0;
        w.vm_seq = VmSeq::Fire;
        w.vm_time = 0.0;

        // Direction: view + recoil, jittered inside the spread cone.
        let ads = w.ads;
        let base = w.def.spread.base(stance, ads);
        let cone = (base + w.spread_kick).to_radians() * 0.5;
        let (r1, r2) = (w.rand(), w.rand());
        let yaw = vyaw + punch.yaw.to_radians() + (r1 - 0.5) * 2.0 * cone;
        let pitch = vpitch + punch.pitch.to_radians() + (r2 - 0.5) * 2.0 * cone;
        let mut dir = Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin());
        let eye = Vec3::from(s.eye(&settings.0).to_array());
        if smart {
            let view = Vec3::new(vpitch.cos() * vyaw.cos(), vpitch.cos() * vyaw.sin(), -vpitch.sin());
            let lock = dummies
                .iter()
                .filter(|d| d.alive())
                .map(|d| d.pos + Vec3::Z * d.height * 0.6 - eye)
                .filter(|to| to.length() < 8000.0 && to.normalize().dot(view) > SMART_CORE_CONE.to_radians().cos())
                .filter(|to| world.0.raycast(SVec3::from(eye.to_array()), SVec3::from(to.normalize().to_array()), to.length() - 100.0).is_none())
                .max_by(|a, b| a.normalize().dot(view).total_cmp(&b.normalize().dot(view)));
            if let Some(to) = lock {
                log::debug!("smart core: shot steered {:.1} degrees onto a target", dir.angle_between(to).to_degrees());
                dir = to.normalize();
            }
        }

        // Projectile weapons (and bolt patterns) fire bolts that fly; see pilotweapon.rs.
        if let (Some(speed), false) = (gun.projectile, burst) {
            let fwd = dir;
            let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
            let up = right.cross(fwd).normalize_or_zero();
            let dirs: Vec<Vec3> = match gun.pellets {
                Some((table, scale)) => table.iter().map(|o| (fwd + up * o[0] * scale * (1.0 - 0.5 * ads) + right * o[1] * scale * (1.0 - 0.5 * ads)).normalize()).collect(),
                // Splitter Rifle in ADS: ADS_SHOT_COUNT_NORMAL (3) bolts along the view's right
                // vector at boltOffsets 0, 0.022, -0.022 (the shared-energy cost isn't modelled).
                None if gun.id == "mp_titanweapon_particle_accelerator" && ads >= 0.99 => [0.0, 0.022, -0.022].iter().map(|o| (fwd + right * *o).normalize()).collect(),
                None => vec![fwd],
            };
            for d in dirs {
                crate::pilotweapon::spawn_bolt(&mut commands, &fx, eye + d * 120.0, d * speed, gun.gravity, &shot_def, true);
            }
            w.spread_kick = w.def.spread.kicked(w.spread_kick, stance, ads);
            let r = [w.rand(), w.rand(), w.rand(), w.rand()];
            punch.kick(&w.def.kick, &springs, ads, false, kick_scale, r);
            if game_fx {
                titan_shot_fx(&mut commands, &w, *mode == CameraMode::Cockpit && piloted, &actors, &globals);
            }
            continue;
        }

        // Nearest of world geometry and dummy Titans.
        let max = 16000.0;
        let mut hit_t = world
            .0
            .raycast(SVec3::from(eye.to_array()), SVec3::from(dir.to_array()), max)
            .map(|h| h.t)
            .unwrap_or(max);
        // An enemy Tone's Particle Wall stops the shot (and takes its damage).
        let mut wall = None;
        if let Some((we, t)) = enemy_walls.hit(eye, dir, hit_t) {
            hit_t = t;
            wall = Some(we);
        }
        let mut hit_dummy = None;
        for (i, d) in dummies.iter().enumerate() {
            if !d.alive() {
                continue;
            }
            if let Some(h) = crate::hitbox::trace(eye, dir, hit_t, &d, hitboxes, &actors, &globals) {
                hit_t = h.t;
                hit_dummy = Some((i, h));
            }
        }
        if let (Some(we), None) = (wall, hit_dummy.as_ref()) {
            enemy_walls.absorbed.push((we, if burst { crate::abilities::CORE_DAMAGE } else { shot_def.damage_near }));
            particles::emit(&mut commands, particles::Effect::Hit { at: to_bevy(eye + dir * hit_t), normal: -to_bevy(dir).normalize_or(Vec3::Y), table: shot_def.impact_table, surface: particles::Surface::Shield, victim: false, scale: 1.0 });
        }
        let end = eye + dir * hit_t;
        if let Some((i, h)) = hit_dummy {
            if let Some(mut d) = dummies.iter_mut().nth(i) {
                let dmg = if burst { crate::abilities::CORE_DAMAGE } else { crate::hitbox::damage(&shot_def, &h, hit_t, d.infantry) };
                let hit = d.damage(dmg, w.def.stops_regen);
                if let Some(c) = core.as_mut() {
                    c.credit_inflicted(hit);
                }
            }
            w.hits += 1;
            log::debug!("hit dummy {i} at {hit_t:.0} units (group {} crit {})", h.group, h.crit);
        }

        // Recoil and bloom.
        w.spread_kick = w.def.spread.kicked(w.spread_kick, stance, ads);
        let r = [w.rand(), w.rand(), w.rand(), w.rand()];
        punch.kick(&w.def.kick, &springs, ads, false, 1.0, r);

        // Tracer from the visible muzzle to the hit point, plus an impact flash.
        let cockpit = *mode == CameraMode::Cockpit && piloted;
        let muzzle = if cockpit { muzzle_world(&actors, &globals, w.viewmodel.map(|v| v.1)) } else { muzzle_world(&actors, &globals, w.world_gun) };
        let start = muzzle.unwrap_or(to_bevy(eye));
        let stop = to_bevy(end);
        let seg = stop - start;
        let len = seg.length();
        let dir_b = to_bevy(dir).normalize_or(Vec3::NEG_Z);
        if !(game_fx && titan_shot_fx(&mut commands, &w, cockpit, &actors, &globals)) {
            particles::emit(&mut commands, particles::Effect::Muzzle { at: start, dir: dir_b, scale: if cockpit { 2.0 } else { 3.0 }, energy: false });
        }
        if len > 0.5 {
            let tracer = if cockpit && !w.def.fx.tracer_view.is_empty() { &w.def.fx.tracer_view } else { &w.def.fx.tracer_world };
            if game_fx && !tracer.is_empty() {
                crate::pfx::emit_beam(&mut commands, tracer, start, stop);
            } else {
                particles::emit(&mut commands, particles::Effect::Tracer { from: start, to: stop, width: 0.16, color: Vec3::new(4.0, 2.2, 0.8) });
            }
        }
        let surface = match hit_dummy.as_ref().and_then(|(i, _)| dummies.iter().nth(*i)) {
            Some(d) if d.infantry => particles::Surface::Flesh,
            Some(_) => particles::Surface::Titan,
            None => particles::Surface::World,
        };
        particles::emit(&mut commands, particles::Effect::Hit { at: stop, normal: -dir_b, table: shot_def.impact_table, surface, victim: false, scale: if hit_dummy.is_some() { 2.2 } else { 1.6 } });
        commands.spawn((
            PointLight { color: Color::srgb(1.0, 0.7, 0.4), intensity: 400_000.0, range: 12.0, ..default() },
            Transform::from_translation(start),
            Fx { life: 0.05, max: 0.05, base: Vec3::ONE, keep_z: false },
        ));
    }
}

/// Ray vs vertical cylinder standing at `base` (game units). Returns the hit distance.
pub fn ray_cylinder(o: Vec3, d: Vec3, base: Vec3, r: f32, h: f32) -> Option<f32> {
    let (ox, oy) = (o.x - base.x, o.y - base.y);
    let a = d.x * d.x + d.y * d.y;
    if a < 1e-8 {
        return None;
    }
    let b = 2.0 * (ox * d.x + oy * d.y);
    let c = ox * ox + oy * oy - r * r;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    // Starting inside the cylinder counts as a hit at the start (a bolt spawned next to a Titan).
    let t = if c < 0.0 { 0.0 } else if t < 0.0 { return None } else { t };
    let z = o.z + d.z * t - base.z;
    (0.0..=h).contains(&z).then_some(t)
}

pub fn update_fx(mut commands: Commands, time: Res<Time>, mut fx: Query<(Entity, &mut Fx, &mut Transform, Option<&mut PointLight>)>) {
    for (e, mut f, mut tf, light) in &mut fx {
        f.life -= time.delta_secs();
        if f.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = f.life / f.max;
        if let Some(mut l) = light {
            l.intensity = 400_000.0 * f.base.x * k;
        } else {
            let s = f.base * (0.4 + 0.6 * k);
            tf.scale = if f.keep_z { Vec3::new(s.x, s.y, f.base.z) } else { s };
        }
    }
}

/// The Titan gun's sway pivots and muzzle/shell attachments, read from its models once
/// (parsing them takes most of a frame, so `prepare_weapon_models` does it while loading).
fn weapon_model_info(gd: &crate::gamedata::GameData, w: &mut Weapon) {
    if w.pivots.is_some() {
        return;
    }
    w.pivots = Some(crate::pilotweapon::sway_pivots(gd, &w.def));
    let model = |p: &str| gd.read_file(p).ok().and_then(|b| tf_assets::mdl::Model::parse(b).ok());
    let (vmm, gun) = (model(&w.def.viewmodel), model(&w.def.playermodel));
    let att = |m: &Option<tf_assets::mdl::Model>, name: &str, dflt: &str| m.as_ref().and_then(|m| crate::pilotweapon::model_attachment(m, if name.is_empty() { dflt } else { name }));
    let fx = w.def.fx.clone();
    w.fx_atts = Some([att(&vmm, &fx.muzzle_attach, "muzzle_flash"), att(&vmm, &fx.shell_attach, "shell"), att(&gun, &fx.muzzle_attach, "muzzle_flash")]);
}

/// Read the Titan gun's model info under the loading screen (and after a kit change).
pub fn prepare_weapon_models(gd: Res<crate::gamedata::GameData>, mut weapons: Query<&mut Weapon, With<PlayerTitan>>) {
    for mut w in &mut weapons {
        if w.pivots.is_none() {
            weapon_model_info(&gd, &mut w);
        }
    }
}

/// Drive the first-person viewmodel: pick its sequence and place it at the eye.
pub fn weapon_viewmodel(
    time: Res<Time>,
    mode: Res<CameraMode>,
    control: Res<crate::pilotctl::Control>,
    settings: Res<TitanSettings>,
    punch: Res<ViewPunch>,
    gd: Res<crate::gamedata::GameData>,
    mut titans: Query<(&PlayerTitan, &mut Weapon, Option<&crate::combat::Melee>)>,
    mut actors: Query<&mut Actor>,
    mut tfs: Query<(&mut Transform, &mut Visibility), Without<PlayerTitan>>,
) {
    let Ok((titan, mut w, melee)) = titans.single_mut() else { return };
    let Some((anchor, vm)) = w.viewmodel else { return };
    weapon_model_info(&gd, &mut w);
    let in_titan = *control == crate::pilotctl::Control::Titan;
    if melee.is_some_and(|m| m.t.is_some()) && !matches!(w.vm_seq, VmSeq::Melee | VmSeq::MeleeDash) {
        w.vm_seq = if titan.state.dash_time > 0.0 { VmSeq::MeleeDash } else { VmSeq::Melee };
        w.vm_time = 0.0;
    }
    let Ok(mut actor) = actors.get_mut(vm) else { return };
    actor.autoplay = false;
    let dt = time.delta_secs();
    w.vm_time += dt;
    let s = &titan.state;

    let clip = |a: &Actor, name: &str| a.clip(name);
    let grid_clip = |a: &Actor, name: &str, i: usize| a.grids.get(name).and_then(|g| g.clips.get(i).copied());
    let dur = |a: &Actor, c: Option<usize>| c.map(|c| a.clips[c].duration).unwrap_or(0.5);

    // One-shot sequences fall back to idle (or sprint) when they finish.
    let (deploy_time, reload_time, reload_empty_time) = (w.def.deploy_time, w.def.reload_time, w.def.reload_empty_time);
    let done = |a: &Actor, seq: VmSeq, t: f32| match seq {
        VmSeq::Draw => t >= deploy_time.max(0.05),
        VmSeq::Fire => t >= dur(a, grid_clip(a, "attack_seq", 0)),
        VmSeq::Reload => t >= reload_time.max(0.05),
        VmSeq::ReloadEmpty => t >= reload_empty_time.max(0.05),
        VmSeq::Melee => t >= dur(a, clip(a, "melee_seq")).min(crate::combat::MELEE_ANIM + 0.2),
        VmSeq::MeleeDash => t >= dur(a, clip(a, "melee_dash_seq").or(clip(a, "melee_seq"))).min(crate::combat::MELEE_ANIM + 0.2),
        VmSeq::SprintRaise => t >= dur(a, clip(a, "sprintraise_seq")) || s.sprinting,
        VmSeq::AmmoSwap => t >= dur(a, clip(a, "ammo_swap_seq")),
        _ => true,
    };
    // Coming out of a sprint plays the raise before idling.
    if w.vm_seq == VmSeq::Sprint && !s.sprinting && clip(&actor, "sprintraise_seq").is_some() {
        w.vm_seq = VmSeq::SprintRaise;
        w.vm_time = 0.0;
    }
    if done(&actor, w.vm_seq, w.vm_time) {
        let next = if s.sprinting { VmSeq::Sprint } else { VmSeq::Idle };
        if next != w.vm_seq {
            w.vm_seq = next;
            w.vm_time = 0.0;
        }
    }
    let ads = w.ads;
    let looped = |a: &Actor, c: usize, t: f32| (t / a.clips[c].duration.max(1e-3)).rem_euclid(1.0);
    let once = |a: &Actor, c: usize, t: f32| (t / a.clips[c].duration.max(1e-3)).min(0.999);
    let mut layers = Vec::new();
    let mut fire_additive = Vec::new();
    match w.vm_seq {
        VmSeq::Idle | VmSeq::Fire => {
            // idle_seq and attack_seq are delta animations (STUDIO_DELTA | STUDIO_POST), so the
            // resting pose is the end of draw_seq (hip) or ads_in_seq (zoomed), with the idle
            // sway and the firing kick added on top, hip and ADS variants blended by zoom.
            let firing = w.vm_seq == VmSeq::Fire;
            for (name, weight) in [("draw_seq", 1.0 - ads), ("ads_in_seq", ads)] {
                if let Some(c) = clip(&actor, name) {
                    layers.push(Layer { clip: c, cycle: 0.999, weight: weight.max(1e-3) });
                }
            }
            for (i, weight) in [(0, 1.0 - ads), (1, ads)] {
                if let Some(c) = grid_clip(&actor, "idle_seq", i) {
                    let cycle = looped(&actor, c, w.idle_time);
                    actor.push_clip(&mut layers, &mut fire_additive, c, cycle, weight);
                }
                if let Some(c) = grid_clip(&actor, "attack_seq", i).filter(|_| firing) {
                    let cycle = once(&actor, c, w.vm_time);
                    actor.push_clip(&mut layers, &mut fire_additive, c, cycle, weight);
                }
            }
        }
        seq => {
            let name = match seq {
                VmSeq::Draw => "draw_seq",
                VmSeq::Reload => "reload_seq",
                VmSeq::ReloadEmpty => "reload_empty_seq",
                VmSeq::Melee => "melee_seq",
                VmSeq::MeleeDash if clip(&actor, "melee_dash_seq").is_some() => "melee_dash_seq",
                VmSeq::MeleeDash => "melee_seq",
                VmSeq::SprintRaise => "sprintraise_seq",
                VmSeq::AmmoSwap => "ammo_swap_seq",
                _ => "sprint_seq",
            };
            if let Some(c) = clip(&actor, name) {
                // Draw and reload play over the script's deploy/reload time, as the engine scales them.
                let script_len = match seq {
                    VmSeq::Draw => Some(deploy_time),
                    VmSeq::Reload => Some(reload_time),
                    VmSeq::ReloadEmpty => Some(reload_empty_time),
                    _ => None,
                };
                let cycle = match script_len {
                    _ if seq == VmSeq::Sprint => looped(&actor, c, w.vm_time),
                    Some(l) => (w.vm_time / l.max(0.05)).min(0.999),
                    None => once(&actor, c, w.vm_time),
                };
                layers.push(Layer { clip: c, cycle, weight: 1.0 });
            }
        }
    }
    actor.layers = layers;
    w.idle_time += dt;
    actor.additive = fire_additive;

    // Place the viewmodel so its camera bone is on the eye, like the cockpit.
    let offset = actor.bone_model_transform("jx_c_camera").map(|t| t.translation).unwrap_or(Vec3::ZERO);
    // Sway, bob and the gun's share of the recoil, about the script's sway pivot.
    let pivots = w.pivots.clone().unwrap_or_default();
    let pivot_of = |(bone, off): &(String, Vec3)| actor.bone_model_transform(bone).map(|t| t.transform_point(*off));
    let hp = pivot_of(&pivots[0]).unwrap_or(offset);
    let pivot = hp.lerp(pivot_of(&pivots[1]).unwrap_or(hp), ads);
    let (cy, sy) = (s.yaw.cos(), s.yaw.sin());
    let vel = Vec3::new(s.vel.x * cy + s.vel.y * sy, -s.vel.x * sy + s.vel.y * cy, s.vel.z);
    let inp = crate::vmmotion::MotionInput { yaw: s.yaw, pitch: s.pitch, vel, on_ground: s.on_ground, bob: !s.sprinting, ads };
    let def = w.def.motion.clone();
    let off = w.motion.step(&def, &inp, dt);
    let rs = crate::vmmotion::angles_quat(off.rot + punch.vm);
    if let Ok((mut tf, mut vis)) = tfs.get_mut(anchor) {
        *vis = if *mode == CameraMode::Cockpit && in_titan { Visibility::Inherited } else { Visibility::Hidden };
        let yaw = s.yaw + punch.yaw.to_radians();
        let pitch = s.pitch + punch.pitch.to_radians();
        let rot = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(punch.roll.to_radians());
        tf.rotation = rot * rs;
        tf.translation = Vec3::from(s.eye(&settings.0).to_array()) - rot * offset + rot * (pivot + off.trans + punch.vm_shake - rs * pivot);
    }
}

/// Swap BT's weapon when the loadout changes: new script, viewmodel (with BT's arms) and gun
/// in his hand.
#[allow(clippy::too_many_arguments)]
pub fn switch_titan_weapon(
    mut commands: Commands,
    mut kit: ResMut<TitanKit>,
    mut input: ResMut<PlayerInput>,
    control: Res<crate::pilotctl::Control>,
    gd: Res<crate::gamedata::GameData>,
    mut cache: ResMut<crate::convert::Cache>,
    root: Res<crate::pilotweapon::WorldRoot>,
    hand: Option<Res<BtHand>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    mut titans: Query<&mut Weapon>,
) {
    use crate::actor::{spawn_actor, ActorSpec, BoneMergeTo};
    // 1-8 pick a loadout while in the Titan; the loadout menu sets `TitanKit` directly.
    if let Some(k) = input.titan_kit.take() {
        if *control == crate::pilotctl::Control::Titan && (k as usize) < TITAN_ARSENAL.len() {
            kit.0 = k as usize;
        }
    }
    let want = kit.0.min(TITAN_ARSENAL.len() - 1);
    let Ok(mut w) = titans.single_mut() else { return };
    if w.arsenal == want {
        return;
    }
    let t = std::time::Instant::now();
    let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    let def = WeaponDef::load_player(TITAN_ARSENAL[want].id, &mut read);
    if let Some((anchor, _)) = w.viewmodel {
        commands.entity(anchor).despawn();
    }
    if let Some(g) = w.world_gun {
        commands.entity(g).despawn();
    }
    let anchor = commands.spawn((Transform::IDENTITY, Visibility::Hidden, crate::vmcam::Viewmodel)).id();
    commands.entity(root.0).add_child(anchor);
    let body_of = |path: &str| gd.read_file(path).ok().and_then(|b| tf_assets::mdl::Model::parse(b).ok()).map(|m| def.body_for(&m)).unwrap_or_default();
    let vm_body = body_of(&def.viewmodel);
    let spec = ActorSpec {
        path: &def.viewmodel,
        sequences: &["draw_seq", "ads_in_seq", "reload_seq", "reload_empty_seq", "sprint_seq", "?sprintraise_seq", "melee_seq", "?melee_dash_seq", "?ammo_swap_seq"],
        grids: &["idle_seq", "attack_seq"],
        body: &vm_body,
    };
    let viewmodel = match spawn_actor(&mut commands, anchor, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
        Ok(vm) => {
            let arms = ActorSpec { path: "models/weapons/arms/buddypov.mdl", sequences: &[], grids: &[], body: &[] };
            if let Ok(a) = spawn_actor(&mut commands, anchor, &gd, &arms, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes) {
                commands.entity(a.entity).insert(BoneMergeTo(vm.entity));
            }
            Some((anchor, vm.entity))
        }
        Err(e) => {
            log::warn!("titan viewmodel {}: {e:#}", def.viewmodel);
            None
        }
    };
    let world_gun = hand.and_then(|h| {
        let gun_body = body_of(&def.playermodel);
        let spec = ActorSpec { path: &def.playermodel, sequences: &[], grids: &[], body: &gun_body };
        spawn_actor(&mut commands, h.0, &gd, &spec, &mut cache, &mut meshes, &mut images, &mut materials, &mut bindposes).ok().map(|g| g.entity)
    });
    log::info!("BT loadout: {} ({}) in {:?}", TITAN_ARSENAL[want].kit, def.name, t.elapsed());
    let mut fresh = Weapon::new(def, viewmodel, world_gun);
    fresh.arsenal = want;
    *w = fresh;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_from_env_spec() {
        let spec = "mp_weapon_r97:pas_fast_reload, extended_ammo;mp_weapon_wingman:pas_fast_ads;*:pas_fast_swap";
        assert_eq!(parse_mods(spec, "mp_weapon_r97"), vec!["pas_fast_reload", "extended_ammo", "pas_fast_swap"]);
        assert_eq!(parse_mods(spec, "mp_weapon_hemlok"), vec!["pas_fast_swap"]);
        assert!(parse_mods("", "mp_weapon_r97").is_empty());
    }
}
