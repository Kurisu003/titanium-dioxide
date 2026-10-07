//! Titan movement: walk, sprint, dash and gravity against a [`CollisionWorld`].
//! Units are the game's (inches, seconds); Z is up; yaw 0 faces +X.

use crate::collision::CollisionWorld;
use glam::{Vec2, Vec3};

/// Tuning, normally filled from `titan_*.set` player settings.
#[derive(Debug, Clone)]
pub struct TitanParams {
    pub speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub sprint_speed: f32,
    pub sprint_accel: f32,
    pub sprint_decel: f32,
    /// lowSpeed / lowAcceleration: below this speed BT accelerates at the faster rate.
    pub low_speed: f32,
    pub low_accel: f32,
    pub side_scale: f32,
    pub back_scale: f32,
    pub dash_speed: f32,
    pub dash_duration: f32,
    pub dash_interval: f32,
    pub dash_stop_speed: f32,
    pub dash_height: f32,
    pub dash_drain: f32,
    pub power_regen: f32,
    pub power_delay: f32,
    pub step_height: f32,
    pub radius: f32,
    pub height: f32,
    pub eye_height: f32,
    pub gravity: f32,
}

impl Default for TitanParams {
    fn default() -> Self {
        // titan_buddy.set values.
        Self {
            speed: 280.0,
            accel: 900.0,
            decel: 1500.0,
            sprint_speed: 420.0,
            sprint_accel: 120.0,
            sprint_decel: 400.0,
            low_speed: 200.0,
            low_accel: 1500.0,
            side_scale: 1.0,
            back_scale: 1.0,
            dash_speed: 685.0,
            dash_duration: 0.3,
            dash_interval: 0.2,
            dash_stop_speed: 350.0,
            dash_height: 12.0,
            dash_drain: 50.0,
            power_regen: 12.0,
            power_delay: 0.2,
            step_height: 80.0,
            radius: 60.0,
            height: 235.0,
            eye_height: 185.0,
            gravity: 750.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TitanInput {
    /// -1..1 forward/back and right/left.
    pub forward: f32,
    pub right: f32,
    /// View yaw/pitch in radians.
    pub yaw: f32,
    pub pitch: f32,
    pub sprint: bool,
    /// Dash requested this tick.
    pub dash: bool,
    /// Movement speed multiplier (status effects like a slow trap); 0 means 1.
    pub speed_scale: f32,
    /// Hover flight: (vertical velocity to hold, horizontal speed limit); no gravity while set.
    pub fly: Option<(f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct TitanState {
    /// Feet position.
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub sprinting: bool,
    pub dash_time: f32,
    pub dash_cooldown: f32,
    /// Dash power, 0..100.
    pub power: f32,
    pub power_delay: f32,
    /// Number of dashes started (for effects/animation triggers).
    pub dashes: u32,
}

impl TitanState {
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            on_ground: false,
            sprinting: false,
            dash_time: 0.0,
            dash_cooldown: 0.0,
            power: 100.0,
            power_delay: 0.0,
            dashes: 0,
        }
    }

    pub fn eye(&self, p: &TitanParams) -> Vec3 {
        self.pos + Vec3::Z * p.eye_height
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.y).length()
    }
}

fn approach(cur: Vec2, target: Vec2, rate: f32) -> Vec2 {
    let d = target - cur;
    let len = d.length();
    if len <= rate || len < 1e-5 {
        target
    } else {
        cur + d / len * rate
    }
}

/// Advance one tick.
pub fn step(s: &mut TitanState, input: &TitanInput, p: &TitanParams, world: &CollisionWorld, dt: f32) {
    s.yaw = input.yaw;
    s.pitch = input.pitch;
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let right = Vec2::new(s.yaw.sin(), -s.yaw.cos());
    let mut wish = fwd * input.forward * if input.forward < 0.0 { p.back_scale } else { 1.0 }
        + right * input.right * p.side_scale;
    if wish.length() > 1.0 {
        wish = wish.normalize();
    }

    // Sprinting needs mostly-forward input and ground contact to start.
    s.sprinting = input.sprint && input.forward > 0.5 && (s.on_ground || s.sprinting);
    let scale = if input.speed_scale > 0.0 { input.speed_scale } else { 1.0 };

    // Dash power.
    s.dash_cooldown = (s.dash_cooldown - dt).max(0.0);
    if s.power_delay > 0.0 {
        s.power_delay -= dt;
    } else {
        s.power = (s.power + p.power_regen * dt).min(100.0);
    }

    let mut hvel = Vec2::new(s.vel.x, s.vel.y);
    if input.dash && s.dash_cooldown <= 0.0 && s.power >= p.dash_drain && s.dash_time <= 0.0 {
        let dir = if wish.length() > 0.1 { wish.normalize() } else { fwd };
        hvel = dir * p.dash_speed * scale;
        s.dash_time = p.dash_duration;
        s.dash_cooldown = p.dash_duration + p.dash_interval;
        s.power -= p.dash_drain;
        s.power_delay = p.power_delay;
        s.dashes += 1;
        if s.on_ground {
            s.vel.z = (2.0 * p.gravity * p.dash_height).sqrt();
            s.on_ground = false;
        }
        s.sprinting = false;
    }

    if s.dash_time > 0.0 {
        s.dash_time -= dt;
        if s.dash_time <= 0.0 && hvel.length() > p.dash_stop_speed {
            hvel = hvel.normalize() * p.dash_stop_speed;
        }
    } else if let Some((_, limit)) = input.fly {
        // Hover (FlyerHovers): airSpeed 200 at airAcceleration 540, capped at limit.
        hvel = approach(hvel, wish * (limit - 50.0).max(0.0), 540.0 * dt);
        if hvel.length() > limit {
            hvel = hvel.normalize() * limit;
        }
    } else if s.on_ground {
        let target_speed = if s.sprinting { p.sprint_speed } else { p.speed } * scale;
        let target = wish * target_speed;
        let cur = hvel.length();
        let rate = if target.length() > cur {
            // Above walking speed, sprint uses its own (slower) acceleration.
            if cur >= p.speed - 1.0 && s.sprinting {
                p.sprint_accel
            } else if cur < p.low_speed {
                p.low_accel.max(p.accel)
            } else {
                p.accel
            }
        } else if cur > p.speed + 1.0 {
            p.sprint_decel
        } else {
            p.decel
        };
        hvel = approach(hvel, target, rate * dt);
    } else {
        // Limited air control.
        hvel = approach(hvel, wish * p.speed.max(hvel.length()), p.accel * 0.15 * dt);
    }
    s.vel.x = hvel.x;
    s.vel.y = hvel.y;

    if let Some((vz, _)) = input.fly {
        s.vel.z = vz;
        s.on_ground = false;
    } else if !s.on_ground {
        s.vel.z -= p.gravity * dt;
    }

    // Move in small substeps so fast dashes cannot tunnel through walls.
    let delta = s.vel * dt;
    let steps = ((delta.length() / (p.radius * 0.5)).ceil() as usize).max(1);
    for _ in 0..steps {
        move_once(s, p, world, delta / steps as f32);
    }
}

fn move_once(s: &mut TitanState, p: &TitanParams, world: &CollisionWorld, delta: Vec3) {
    let was_on_ground = s.on_ground;
    s.pos += delta;

    // Body spheres start above the step height, so low obstacles are stepped onto.
    let r = p.radius;
    let lo = p.step_height + r;
    let hi = (p.height - r).max(lo);
    for z in [lo, (lo + hi) * 0.5, hi] {
        let (push, normals) = world.push_sphere(s.pos + Vec3::Z * z, r, 0.7);
        if push != Vec3::ZERO {
            let mut push = push;
            // Hitting a ceiling while rising stops the climb; otherwise push sideways only.
            if push.z < 0.0 && s.vel.z > 0.0 {
                s.vel.z = 0.0;
            }
            push.z = push.z.min(0.0);
            s.pos += push;
            let n = Vec3::new(normals.x, normals.y, 0.0).normalize_or_zero();
            let into = s.vel.dot(n);
            if into < 0.0 {
                s.vel -= n * into;
            }
        }
    }

    // Ground: highest walkable hit under a few points of the footprint.
    let reach_down = if was_on_ground { p.step_height } else { 1.0 };
    let mut ground: Option<f32> = None;
    let probe = r * 0.6;
    for off in [Vec3::ZERO, Vec3::X * probe, -Vec3::X * probe, Vec3::Y * probe, -Vec3::Y * probe] {
        let origin = s.pos + off + Vec3::Z * p.step_height;
        if let Some(hit) = world.raycast(origin, -Vec3::Z, p.step_height + reach_down) {
            if hit.normal.z > 0.7 {
                ground = Some(ground.map_or(hit.point.z, |g: f32| g.max(hit.point.z)));
            }
        }
    }
    match ground {
        Some(z) if s.vel.z <= 0.0 => {
            s.pos.z = z;
            s.vel.z = 0.0;
            s.on_ground = true;
        }
        _ => s.on_ground = false,
    }
}
