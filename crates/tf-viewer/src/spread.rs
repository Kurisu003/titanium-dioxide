//! Weapon spread by stance, from the weapon scripts' `spread_*` keys: a base cone per stance
//! (standing still, moving, sprinting, crouched, in the air, wall-running, wall-hanging) for
//! hip fire and ADS, plus a per-shot kick (`spread_kick_on_fire_*`) capped per stance
//! (`spread_max_kick_*`) that decays after `spread_decay_delay` at `spread_decay_rate`
//! degrees/s. Angles are the cone's full width in degrees.

use tf_assets::settings::PlayerSettings;

fn val(s: &PlayerSettings, k: &str) -> Option<f32> {
    s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).and_then(|v| v.trim().parse().ok())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Stand,
    Run,
    Sprint,
    Crouch,
    Air,
    WallRun,
    WallHang,
}

/// (hip, ads) for one stance.
#[derive(Clone, Copy, Debug, Default)]
struct Pair {
    hip: f32,
    ads: f32,
}

impl Pair {
    fn at(&self, ads: f32) -> f32 {
        self.hip + (self.ads - self.hip) * ads
    }
}

#[derive(Clone, Debug, Default)]
pub struct SpreadDef {
    stand: Pair,
    run: Pair,
    sprint: Pair,
    crouch: Pair,
    air: Pair,
    wallrun: Pair,
    wallhang: Pair,
    /// Kick per shot and its cap, for standing, crouched and in the air.
    kick: [Pair; 3],
    max_kick: [Pair; 3],
    pub decay_rate: f32,
    pub decay_delay: f32,
}

impl SpreadDef {
    pub fn from_settings(s: &PlayerSettings) -> Self {
        let f = |k: &str, d: f32| val(s, k).unwrap_or(d);
        let stand = Pair { hip: f("spread_stand_hip", 1.2), ads: f("spread_stand_ads", 0.4) };
        let crouch = Pair { hip: f("spread_crouch_hip", stand.hip), ads: f("spread_crouch_ads", stand.ads) };
        let air = Pair { hip: f("spread_air_hip", stand.hip), ads: f("spread_air_ads", stand.ads) };
        let run_hip = f("spread_stand_hip_run", stand.hip);
        let kick_stand = Pair { hip: f("spread_kick_on_fire_stand_hip", 0.4), ads: f("spread_kick_on_fire_stand_ads", 0.3) };
        let max_stand = Pair { hip: f("spread_max_kick_stand_hip", 8.0), ads: f("spread_max_kick_stand_ads", 2.0) };
        Self {
            stand,
            // Moving and sprinting only have hip values; ADS uses the standing one.
            run: Pair { hip: run_hip, ads: stand.ads },
            sprint: Pair { hip: f("spread_stand_hip_sprint", run_hip), ads: stand.ads },
            crouch,
            air,
            // Wall-running and hanging have one value each; the air values when missing.
            wallrun: val(s, "spread_wallrunning").map(|v| Pair { hip: v, ads: v.min(air.ads) }).unwrap_or(air),
            wallhang: val(s, "spread_wallhanging").map(|v| Pair { hip: v, ads: v.min(stand.ads) }).unwrap_or(stand),
            kick: [
                kick_stand,
                Pair { hip: f("spread_kick_on_fire_crouch_hip", kick_stand.hip), ads: f("spread_kick_on_fire_crouch_ads", kick_stand.ads) },
                Pair { hip: f("spread_kick_on_fire_air_hip", kick_stand.hip), ads: f("spread_kick_on_fire_air_ads", kick_stand.ads) },
            ],
            max_kick: [
                max_stand,
                Pair { hip: f("spread_max_kick_crouch_hip", max_stand.hip), ads: f("spread_max_kick_crouch_ads", max_stand.ads) },
                Pair { hip: f("spread_max_kick_air_hip", max_stand.hip), ads: f("spread_max_kick_air_ads", max_stand.ads) },
            ],
            decay_rate: f("spread_decay_rate", 6.5),
            decay_delay: f("spread_decay_delay", 0.15),
        }
    }

    /// The stance's base cone.
    pub fn base(&self, stance: Stance, ads: f32) -> f32 {
        let p = match stance {
            Stance::Stand => self.stand,
            Stance::Run => self.run,
            Stance::Sprint => self.sprint,
            Stance::Crouch => self.crouch,
            Stance::Air => self.air,
            Stance::WallRun => self.wallrun,
            Stance::WallHang => self.wallhang,
        };
        p.at(ads)
    }

    fn kick_row(stance: Stance) -> usize {
        match stance {
            Stance::Crouch => 1,
            Stance::Air | Stance::WallRun => 2,
            _ => 0,
        }
    }

    /// The kick after one more shot.
    pub fn kicked(&self, kick: f32, stance: Stance, ads: f32) -> f32 {
        let r = Self::kick_row(stance);
        (kick + self.kick[r].at(ads)).min(self.max_kick[r].at(ads).max(kick))
    }

    /// The kick after `dt` more seconds, `since_fire` seconds after the last shot.
    pub fn decayed(&self, kick: f32, since_fire: f32, dt: f32) -> f32 {
        if since_fire > self.decay_delay {
            (kick - self.decay_rate * dt).max(0.0)
        } else {
            kick
        }
    }
}

/// The Pilot's stance for spread.
pub fn pilot_stance(s: &tf_sim::pilot::PilotState) -> Stance {
    use tf_sim::pilot::PilotMove;
    match s.mode {
        PilotMove::WallRun { .. } => Stance::WallRun,
        PilotMove::WallHang { .. } => Stance::WallHang,
        PilotMove::Slide => Stance::Crouch,
        _ if !s.on_ground() => Stance::Air,
        _ if s.crouched => Stance::Crouch,
        _ if s.sprinting => Stance::Sprint,
        _ if (s.vel.x * s.vel.x + s.vel.y * s.vel.y).sqrt() > 20.0 => Stance::Run,
        _ => Stance::Stand,
    }
}
