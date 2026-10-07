//! Titan health as the campaign runs it, ported from scripts/vscripts/titan/_titan_health.gnut
//! with the `solo` playlist (titan_shield_regen 1, titan_health_regen 0, segmented health off,
//! infinite_doomed_state 1) and the SP constants from sh_consts.gnut / _settings.nut.
//!
//! - Damage hits the shield first (TITAN_SHIELD_PERMAMENT_DAMAGE_FRAC = 0), then health.
//! - A hit that would take health to DOOMED_MIN_HEALTH or below dooms the Titan instead: its
//!   health becomes `healthDoomed` (no drain in the infinite variant) and it ignores damage for
//!   TITAN_DOOMED_INVUL_TIME. The next killshot destroys it.
//! - Shields regenerate to full over TITAN_SHIELD_REGEN_TIME once the regen delay
//!   (CalculateNextRegenTime with variable_regen_delay) has passed; never while doomed.
//! - Health never regenerates.

use tf_assets::settings::PlayerSettings;

const DOOMED_MIN_HEALTH: f32 = 1.0;
pub const DOOMED_INVUL_TIME: f32 = 0.25;
const SHIELD_REGEN_TIME: f32 = 2.0; // [SP]
const REGEN_MIN_DAMAGE: f32 = 70.0; // [SP]
const REGEN_MIN_DAMAGE_DELAY: f32 = 0.5;

// Core meter credit (AddCreditToTitanCoreBuilder*): fractions of a full meter.
pub const CORE_CREDIT_PER_DAMAGE_INFLICTED: f32 = 0.0100 * 0.01; // [SP]
pub const CORE_CREDIT_PER_DAMAGE_RECEIVED: f32 = 0.002 * 0.01;
pub const CORE_CREDIT_DOOM_INFLICTED: f32 = 10.0 * 0.01;

#[derive(Clone, Debug, Default)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub shield: f32,
    pub max_shield: f32,
    pub doomed_health: f32,
    /// `titan_regen_delay` from the Titan's .set file.
    pub regen_delay: f32,
    /// Seconds since becoming doomed.
    pub doomed: Option<f32>,
    /// Seconds until the shield may regenerate (may be negative).
    regen_in: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    /// Damage actually taken by shield and health.
    pub dealt: f32,
    pub doomed_now: bool,
    pub killed: bool,
}

impl Vitals {
    /// From a Titan .set file (health, healthShield, healthDoomed, titan_regen_delay,
    /// start_with_shields). `health` overrides the .set value when the AI settings give one.
    pub fn from_settings(s: &PlayerSettings, health: Option<f32>) -> Self {
        let max_health = health.unwrap_or_else(|| s.f32("global.health", 9000.0));
        let max_shield = s.f32("global.healthshield", 0.0);
        let start = s.f32(".start_with_shields", 0.0) > 0.0;
        Self {
            health: max_health,
            max_health,
            shield: if start { max_shield } else { 0.0 },
            max_shield,
            doomed_health: s.f32("global.healthdoomed", 0.0),
            regen_delay: s.f32(".titan_regen_delay", 8.0),
            doomed: None,
            regen_in: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.health > 0.0
    }

    /// Fraction for a health bar: undoomed health, or what is left of the doomed health.
    pub fn fraction(&self) -> f32 {
        let max = if self.doomed.is_some() { self.doomed_health } else { self.max_health };
        (self.health / max.max(1.0)).clamp(0.0, 1.0)
    }

    /// `stops_regen`: the weapon has DF_STOPS_TITAN_REGEN (or the hit was critical).
    pub fn damage(&mut self, dmg: f32, stops_regen: bool) -> Hit {
        if !self.alive() || dmg <= 0.0 {
            return Hit::default();
        }
        if self.doomed.is_some_and(|t| t < DOOMED_INVUL_TIME) {
            return Hit::default();
        }
        if self.doomed.is_none() {
            self.regen_in = next_regen(dmg, stops_regen, self.regen_in, self.regen_delay);
        }
        let absorbed = dmg.min(self.shield);
        self.shield -= absorbed;
        let rest = dmg - absorbed;
        let mut hit = Hit { dealt: absorbed, ..Hit::default() };
        if self.health - rest <= DOOMED_MIN_HEALTH {
            hit.dealt += self.health;
            if self.doomed.is_none() && self.doomed_health > 0.0 {
                self.doomed = Some(0.0);
                self.health = self.doomed_health;
                hit.doomed_now = true;
            } else {
                self.health = 0.0;
                hit.killed = true;
            }
        } else {
            self.health -= rest;
            hit.dealt += rest;
        }
        hit
    }

    pub fn tick(&mut self, dt: f32) {
        if !self.alive() {
            return;
        }
        if let Some(t) = &mut self.doomed {
            *t += dt;
            return;
        }
        self.regen_in -= dt;
        if self.regen_in <= 0.0 && self.shield < self.max_shield {
            self.shield = (self.shield + self.max_shield / SHIELD_REGEN_TIME * dt).min(self.max_shield);
        }
    }
}

/// CalculateNextRegenTime, as a countdown instead of an absolute time.
fn next_regen(damage: f32, stops_regen: bool, regen_in: f32, max_delay: f32) -> f32 {
    if damage >= REGEN_MIN_DAMAGE || stops_regen {
        let delay = graph_capped(damage, 100.0, 1000.0, 1.0, max_delay);
        let from_now = delay.min(max_delay);
        let from_previous = (regen_in + delay).min(max_delay);
        from_now.max(from_previous)
    } else if regen_in <= REGEN_MIN_DAMAGE_DELAY {
        REGEN_MIN_DAMAGE_DELAY
    } else {
        regen_in
    }
}

/// Respawn's GraphCapped: map `x` from [x0, x1] to [y0, y1], clamped.
pub fn graph_capped(x: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 + (y1 - y0) * t
}

/// SP difficulty (_sp_difficulty.gnut).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, bevy::prelude::Resource)]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
    Master,
}

impl Difficulty {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "easy" => Self::Easy,
            "normal" => Self::Normal,
            "hard" => Self::Hard,
            "master" => Self::Master,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Easy => "EASY",
            Self::Normal => "NORMAL",
            Self::Hard => "HARD",
            Self::Master => "MASTER",
        }
    }
    /// DIFFICULTY_*_DAMAGE_SCALAR: damage against the player (Pilot or Titan).
    pub fn damage_to_player(self) -> f32 {
        match self {
            Self::Easy => 0.5,
            Self::Normal => 1.0,
            Self::Hard => 1.5,
            Self::Master => 1.9,
        }
    }
    /// maxDamagePerHit: cap on damage a Pilot takes within 0.25 s.
    pub fn max_pilot_damage_per_hit(self) -> f32 {
        match self {
            Self::Easy => 40.0,
            Self::Normal => 60.0,
            Self::Hard => 70.0,
            Self::Master => 80.0,
        }
    }
    /// NPCSetAimConeFocusParams: (initial extra cone in degrees, seconds to focus).
    pub fn aim_cone_focus(self) -> (f32, f32) {
        match self {
            Self::Easy => (8.0, 3.5),
            Self::Normal => (6.0, 3.0),
            Self::Hard => (3.0, 2.0),
            Self::Master => (0.2, 1.0),
        }
    }
    /// SetTitanProficiency for a regular (non-boss) enemy Titan.
    pub fn titan_proficiency(self) -> Proficiency {
        match self {
            Self::Easy => Proficiency::Poor,
            Self::Normal => Proficiency::Average,
            Self::Hard => Proficiency::Good,
            Self::Master => Proficiency::Perfect,
        }
    }
}

/// eWeaponProficiency.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Proficiency {
    Poor = 0,
    Average,
    Good,
    VeryGood,
    Perfect,
}

impl Proficiency {
    /// One step worse: what enemy Titans drop to while the player's Titan is doomed.
    pub fn lowered(self) -> Self {
        match self {
            Self::Poor | Self::Average => Self::Poor,
            Self::Good => Self::Average,
            Self::VeryGood => Self::Good,
            Self::Perfect => Self::VeryGood,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bt() -> Vitals {
        Vitals { health: 9000.0, max_health: 9000.0, shield: 1000.0, max_shield: 1000.0, doomed_health: 2500.0, regen_delay: 6.0, ..Default::default() }
    }

    #[test]
    fn shield_then_health_then_doom() {
        let mut v = bt();
        let h = v.damage(1500.0, true);
        assert_eq!(v.shield, 0.0);
        assert_eq!(v.health, 8500.0);
        assert_eq!(h.dealt, 1500.0);
        let h = v.damage(9000.0, true);
        assert!(h.doomed_now && !h.killed);
        assert_eq!(v.health, 2500.0);
        // Invulnerable right after dooming.
        assert_eq!(v.damage(5000.0, true).dealt, 0.0);
        v.tick(0.3);
        assert!(v.damage(5000.0, true).killed);
    }

    #[test]
    fn shield_regen_waits_for_delay() {
        let mut v = bt();
        v.damage(1000.0, true); // 1000 damage: full 6 s delay
        v.tick(5.0);
        assert_eq!(v.shield, 0.0);
        v.tick(1.1);
        v.tick(2.0);
        assert_eq!(v.shield, 1000.0);
    }

    #[test]
    fn small_hits_only_delay_half_a_second() {
        let mut v = bt();
        v.damage(500.0, false);
        v.damage(20.0, false);
        // 500 damage gave a 3.2 s delay; the small hit does not extend it.
        v.tick(3.3);
        v.tick(0.1);
        assert!(v.shield > 480.0);
    }
}
