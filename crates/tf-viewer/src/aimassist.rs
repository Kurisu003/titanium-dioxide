//! Controller aim assist, from the game's data where it has it:
//! - ADS pull: when you aim down sights, the view pulls onto a target's chest
//!   (`aimassist_adspull_centerRadius` 11 from the AI settings) if it's inside the weapon class's
//!   outer radius; inside the inner radius it lands exactly. The radii are the target's centre
//!   radius times the class scales in `cfg/aimassist/adspull_classes.txt` (single-player
//!   columns), and each weapon names its class (`aimassist_adspull_weaponclass`, "none"
//!   disables it, as do `aimassist_disable_ads`).
//! - Slowdown: look speed drops while the crosshair is over a target (engine-side in the game;
//!   the 0.6 factor here is an approximation).
//!
//! Only applies while the controller is the active input (as in the game on PC).

use crate::player::{to_bevy, MainCamera, PlayerInput};
use crate::targets::Enemy;
use bevy::prelude::*;
use std::collections::HashMap;

/// The grunts' chest radius from their AI settings; Titans have no value in the scripts, so
/// theirs is an estimate scaled to their size.
const CENTER_RADIUS_HUMAN: f32 = 11.0;
const CENTER_RADIUS_TITAN: f32 = 40.0;
/// Seconds the ADS pull takes (roughly the zoom-in time).
const PULL_TIME: f32 = 0.18;
/// Look speed over a target.
const SLOWDOWN: f32 = 0.6;

/// adspull class -> (inner scale, outer scale), SP columns; weapon id -> class.
#[derive(Resource, Default)]
pub struct PullClasses {
    pub classes: HashMap<String, (f32, f32)>,
    pub weapons: HashMap<String, String>,
}

pub fn parse_classes(text: &str) -> HashMap<String, (f32, f32)> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let q: Vec<&str> = line.split('"').collect();
        if q.len() >= 3 {
            let nums: Vec<f32> = q[2].split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if nums.len() >= 4 {
                out.insert(q[1].to_string(), (nums[2], nums[3]));
            }
        }
    }
    out
}

/// Read the class table and every arsenal weapon's class once the game files are open.
pub fn load_pull_classes(gd: Option<Res<crate::gamedata::GameData>>, mut pc: ResMut<PullClasses>, mut done: Local<bool>) {
    let Some(gd) = gd else { return };
    if *done {
        return;
    }
    *done = true;
    let read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    pc.classes = read("cfg/aimassist/adspull_classes.txt").map(|t| parse_classes(&t)).unwrap_or_default();
    let ids = crate::pilotweapon::ARSENAL.iter().map(|e| e.id).chain(crate::weapons::TITAN_ARSENAL.iter().map(|g| g.id));
    for id in ids {
        let Some(t) = read(&format!("scripts/weapons/{id}.txt")) else { continue };
        let get = |k: &str| t.lines().filter(|l| l.contains(&format!("\"{k}\""))).filter_map(|l| l.split('"').nth(3)).last().map(str::to_string);
        let mut class = get("aimassist_adspull_weaponclass").unwrap_or_else(|| "none".into());
        if get("aimassist_disable_ads").as_deref() == Some("1") {
            class = "none".into();
        }
        pc.weapons.insert(id.to_string(), class);
    }
    log::info!("aim assist: {} pull classes, {} weapons", pc.classes.len(), pc.weapons.len());
}

#[derive(Resource, Default)]
pub struct AimAssist {
    /// Multiplier on controller look speed this frame.
    pub slowdown: f32,
    /// Pending ADS pull: remaining yaw/pitch change (radians) and the seconds left to apply it.
    pull: Option<(f32, f32, f32)>,
    was_ads: bool,
}

impl AimAssist {
    /// Take this frame's share of the ADS pull.
    pub fn take_pull(&mut self, dt: f32) -> (f32, f32) {
        let Some((y, p, t)) = self.pull else { return (0.0, 0.0) };
        let k = (dt / t.max(1e-3)).min(1.0);
        let (dy, dp) = (y * k, p * k);
        self.pull = if k >= 1.0 { None } else { Some((y - dy, p - dp, t - dt)) };
        (dy, dp)
    }
}

/// The view direction for a yaw/pitch (game space).
fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin())
}

/// Wrap an angle difference into -pi..pi.
fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Work out slowdown and start ADS pulls (runs before input gathering).
#[allow(clippy::too_many_arguments)]
pub fn aim_assist(
    settings: Res<crate::settings::Settings>,
    pad: Res<crate::gamepad::PadState>,
    input: Res<PlayerInput>,
    classes: Res<PullClasses>,
    control: Res<crate::pilotctl::Control>,
    mut assist: ResMut<AimAssist>,
    cams: Query<&GlobalTransform, With<MainCamera>>,
    enemies: Query<&Enemy>,
    pilots: Query<&crate::pilotweapon::PilotLoadout>,
    titans: Query<&crate::weapons::Weapon>,
) {
    assist.slowdown = 1.0;
    let ads = input.ads || input.pilot_ads;
    let rising = ads && !assist.was_ads;
    assist.was_ads = ads;
    if !settings.aim_assist || !pad.active {
        assist.pull = None;
        return;
    }
    let Ok(cam) = cams.single() else { return };
    let eye = cam.translation();
    let fwd = view_dir(input.yaw, input.pitch);
    // Candidates: (yaw, pitch, angular error, centre radius as an angle).
    let mut best: Option<(f32, f32, f32, f32)> = None;
    for e in enemies.iter().filter(|e| e.alive()) {
        let chest = e.pos + Vec3::Z * if e.infantry { 50.0 } else { e.height * 0.6 };
        let to = (to_bevy(chest) - eye) / crate::player::UNIT;
        // Back to game axes (to_bevy is x, z, -y).
        let to = Vec3::new(to.x, -to.z, to.y);
        let dist = to.length().max(1.0);
        let dir = to / dist;
        let err = fwd.dot(dir).clamp(-1.0, 1.0).acos();
        let radius = if e.infantry { CENTER_RADIUS_HUMAN } else { CENTER_RADIUS_TITAN };
        let r_ang = (radius / dist).atan();
        let yaw = dir.y.atan2(dir.x);
        let pitch = -dir.z.asin();
        if best.is_none_or(|b| err < b.2) {
            best = Some((yaw, pitch, err, r_ang));
        }
    }
    let Some((yaw, pitch, err, r_ang)) = best else { return };
    if err < r_ang * 1.5 {
        assist.slowdown = SLOWDOWN;
    }
    if rising {
        let id = if *control == crate::pilotctl::Control::Pilot {
            pilots.single().ok().and_then(|l| l.active()).map(|g| g.def.name.clone())
        } else {
            titans.single().ok().map(|w| w.def.name.clone())
        };
        let class = id.and_then(|i| classes.weapons.get(&i).cloned()).unwrap_or_else(|| "precise_sp".into());
        let (inner, outer) = classes.classes.get(&class).copied().unwrap_or((0.0, 0.0));
        let (inner, outer) = (r_ang * inner, r_ang * outer);
        if outer > 0.0 && err < outer {
            // Full pull inside the inner radius, fading to none at the outer edge.
            let k = if err <= inner { 1.0 } else { 1.0 - (err - inner) / (outer - inner).max(1e-4) };
            assist.pull = Some((wrap(yaw - input.yaw) * k, (pitch - input.pitch) * k, PULL_TIME));
        }
    }
    if !ads {
        assist.pull = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_parse() {
        let t = "//\tName\tInnerScaleMP\n\t\"none\"\t0.0\t0.0\t0.0\t0.0\n\t\"precise_sp\"\t\t1.0\t\t\t\t4.0\t\t\t\t0.73\t\t\t10.0\n";
        let c = parse_classes(t);
        assert_eq!(c.get("precise_sp"), Some(&(0.73, 10.0)));
        assert_eq!(c.get("none"), Some(&(0.0, 0.0)));
    }

    #[test]
    fn pull_is_spread_over_frames() {
        let mut a = AimAssist { slowdown: 1.0, pull: Some((0.2, -0.1, 0.2)), was_ads: true };
        let (y1, _) = a.take_pull(0.1);
        assert!((y1 - 0.1).abs() < 1e-5);
        let (y2, p2) = a.take_pull(0.1);
        assert!((y2 - 0.1).abs() < 1e-5 && (p2 + 0.05).abs() < 1e-5);
        assert_eq!(a.take_pull(0.1), (0.0, 0.0));
    }
}
