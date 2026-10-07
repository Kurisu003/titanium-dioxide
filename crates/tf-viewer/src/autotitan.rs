//! BT on his own while you're on foot (the auto-Titan): he follows you, or guards a spot
//! you point at (the Titan command key toggles, like MP's `eNPCTitanMode` FOLLOW / STAY with
//! `PrototypeOrderTitanMove`), and engages enemies with his main weapon in NPC bursts
//! (`npc_min_burst`..`npc_max_burst`, resting `npc_rest_min`..`npc_rest_max`, within
//! `npc_max_range`, with the weapon's own spread). The campaign's `_ai_titan` script isn't
//! shipped, so the follow distances are approximations.

use crate::pilotctl::{Control, PlayerPilot, Titanfall};
use crate::player::{CameraMode, Collision, PlayerTitan, TitanSettings};
use crate::targets::Enemy;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::titan::TitanInput;

/// Follow: start walking after you past this distance...
const FOLLOW_START: f32 = 600.0;
/// ...and stop this close; sprint when far behind.
const FOLLOW_STOP: f32 = 350.0;
const FOLLOW_SPRINT: f32 = 1500.0;
/// Guard: walk to within this of the ordered spot.
const GUARD_STOP: f32 = 120.0;
/// Turn rate while tracking (the same as the enemy Titans use), radians per second.
const TURN_RATE: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Follow,
    Guard,
}

#[derive(Resource)]
pub struct AutoTitan {
    pub mode: Mode,
    /// Where he was told to stand (game space).
    pub guard: Vec3,
    /// This frame's movement and aim for `player::simulate`, when he's acting on his own.
    pub input: Option<TitanInput>,
    /// Hold the trigger this frame (`weapons::weapon_fire`).
    pub fire: bool,
    pub target: Option<Entity>,
    burst_left: u32,
    rest: f32,
    shot_timer: f32,
    rng: u64,
}

impl Default for AutoTitan {
    fn default() -> Self {
        Self { mode: Mode::Follow, guard: Vec3::ZERO, input: None, fire: false, target: None, burst_left: 0, rest: 1.0, shot_timer: 0.0, rng: 0x7A11_B7_C0DE }
    }
}

impl AutoTitan {
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
    /// The Titan command key: Follow <-> Guard (the spot you're looking at).
    pub fn toggle(&mut self, commands: &mut Commands, look_at: Vec3) {
        self.mode = match self.mode {
            Mode::Follow => {
                self.guard = look_at;
                crate::audio::event(commands, "Menu_TitanAIMode_Guard");
                Mode::Guard
            }
            Mode::Guard => {
                crate::audio::event(commands, "Menu_TitanAIMode_Follow");
                Mode::Follow
            }
        };
        log::info!("BT: {:?} mode", self.mode);
    }
    pub fn clear(&mut self) {
        self.input = None;
        self.fire = false;
        self.target = None;
        self.burst_left = 0;
    }
}

/// Decide BT's movement, aim and trigger for this frame (before `player::simulate`).
#[allow(clippy::too_many_arguments)]
pub fn auto_titan_think(
    time: Res<Time>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    control: Res<Control>,
    mode: Res<CameraMode>,
    mut auto: ResMut<AutoTitan>,
    titans: Query<(&PlayerTitan, &crate::weapons::Weapon, &crate::combat::TitanHealth, Option<&Titanfall>)>,
    pilots: Query<&PlayerPilot>,
    enemies: Query<(Entity, &Enemy)>,
    rodeo: Res<crate::rodeo::Rodeo>,
) {
    let dt = time.delta_secs().min(0.1);
    let (Ok((titan, weapon, health, titanfall)), Ok(pilot)) = (titans.single(), pilots.single()) else {
        auto.clear();
        return;
    };
    let acting = *control == Control::Pilot && *mode != CameraMode::Free && health.dead_for.is_none() && titan.override_anim.is_none() && !titanfall.is_some_and(|t| t.busy());
    if !acting {
        auto.clear();
        return;
    }
    let s = &titan.state;
    let pos = Vec3::from(s.pos.to_array());
    let eye = Vec3::from(s.eye(&settings.0).to_array());
    let w = &weapon.def;

    // --- Target: the nearest live enemy he can see, within the weapon's NPC range ---
    let mut best: Option<(Entity, Vec3, f32, f32)> = None;
    for (id, e) in &enemies {
        if !e.active || !e.alive() || e.arriving() || rodeo.ride.as_ref().is_some_and(|r| r.target == id) {
            continue;
        }
        let aim_at = e.pos + Vec3::Z * e.height * 0.6;
        let to = aim_at - eye;
        let dist = to.length();
        if dist > w.npc_max_range || best.is_some_and(|b| b.2 <= dist) {
            continue;
        }
        if world.0.raycast(SVec3::from(eye.to_array()), SVec3::from((to / dist).to_array()), dist - e.radius).is_some() {
            continue;
        }
        best = Some((id, aim_at, dist, e.radius));
    }
    auto.target = best.map(|b| b.0);

    // --- Movement: follow you or hold the guard spot; stand to shoot ---
    let pilot_pos = Vec3::from(pilot.state.pos.to_array());
    let (goal, start, stop, sprint_from) = match auto.mode {
        Mode::Follow => (pilot_pos, FOLLOW_START, FOLLOW_STOP, FOLLOW_SPRINT),
        Mode::Guard => (auto.guard, GUARD_STOP, GUARD_STOP, 1200.0),
    };
    let to_goal = (goal - pos).truncate();
    let far = to_goal.length();
    let moving = if auto.input.as_ref().is_some_and(|i| i.forward != 0.0 || i.right != 0.0) { far > stop } else { far > start };
    let mut input = TitanInput { yaw: s.yaw, pitch: 0.0, ..Default::default() };
    if moving {
        let dir = crate::targets::steer(&world, pos, to_goal.normalize_or_zero());
        let f = Vec2::new(s.yaw.cos(), s.yaw.sin());
        let r = Vec2::new(s.yaw.sin(), -s.yaw.cos());
        input.forward = dir.dot(f);
        input.right = dir.dot(r);
        input.sprint = far > sprint_from && best.is_none();
    }
    // Face the target, else the way he's going.
    let want_yaw = match best {
        Some((_, at, _, _)) => (at.y - pos.y).atan2(at.x - pos.x),
        None if moving => to_goal.y.atan2(to_goal.x),
        None => s.yaw,
    };
    let diff = (want_yaw - s.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    input.yaw = s.yaw + diff.clamp(-TURN_RATE * dt, TURN_RATE * dt);
    if let Some((_, at, dist, _)) = best {
        input.pitch = -(at.z - eye.z).atan2(dist.max(1.0));
    }
    auto.input = Some(input);

    // --- Fire in NPC bursts once he's facing the target ---
    let facing = best.is_some_and(|(_, at, _, _)| (at - eye).truncate().normalize_or_zero().dot(Vec2::new(s.yaw.cos(), s.yaw.sin())) > 0.95);
    let can_shoot = facing && !s.sprinting;
    let fire_rate = w.fire_rate.clamp(0.1, 10.0);
    if auto.burst_left == 0 {
        auto.rest -= dt;
        auto.fire = false;
        if auto.rest <= 0.0 && can_shoot {
            let span = (w.npc_max_burst.saturating_sub(w.npc_min_burst) + 1) as f32;
            let extra = (auto.rand() * span) as u32;
            auto.burst_left = (w.npc_min_burst + extra).max(1);
            auto.shot_timer = 0.0;
        }
        return;
    }
    if !can_shoot {
        auto.burst_left = 0;
        auto.rest = w.npc_rest_min;
        auto.fire = false;
        return;
    }
    // The weapon's own rate paces the shots; the burst counts them down.
    auto.fire = true;
    auto.shot_timer -= dt;
    while auto.shot_timer <= 0.0 && auto.burst_left > 0 {
        auto.shot_timer += 1.0 / fire_rate;
        auto.burst_left -= 1;
        if auto.burst_left == 0 {
            let r = auto.rand();
            auto.rest = w.npc_rest_min + (w.npc_rest_max - w.npc_rest_min) * r;
        }
    }
}
