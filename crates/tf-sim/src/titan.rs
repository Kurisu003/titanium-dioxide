//! Titan movement: walk, sprint, dash and gravity against a [`CollisionWorld`].
//! Units are the game's (inches, seconds); Z is up; yaw 0 faces +X.

use crate::collision::{CollisionWorld, CLIP_TITAN};
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
    /// dodgeHeightMax: the dash's vertical speed is cut to reach at most this height.
    pub dash_height_max: f32,
    /// dodgeKeepSpeedFrac: fraction of the old speed kept on top of the dash.
    pub dash_keep_frac: f32,
    pub dash_drain: f32,
    /// airSpeed / airAcceleration (engine defaults; the titan .set files set neither).
    pub air_speed: f32,
    pub air_accel: f32,
    pub power_regen: f32,
    pub power_delay: f32,
    pub step_height: f32,
    pub radius: f32,
    pub height: f32,
    pub eye_height: f32,
    pub gravity: f32,
    /// The crouch stance (titan_buddy.set "crouch": speed 150, acceleration 200, deceleration
    /// 500; titan_base.set: viewheight 125, hull_max z 140).
    pub crouch_speed: f32,
    pub crouch_accel: f32,
    pub crouch_decel: f32,
    pub crouch_height: f32,
    pub crouch_eye_height: f32,
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
            dash_height_max: 25.0,
            dash_keep_frac: 0.0,
            dash_drain: 50.0,
            air_speed: 60.0,
            air_accel: 500.0,
            power_regen: 12.0,
            power_delay: 0.2,
            step_height: 80.0,
            radius: 60.0,
            height: 235.0,
            eye_height: 185.0,
            gravity: 750.0,
            crouch_speed: 150.0,
            crouch_accel: 200.0,
            crouch_decel: 500.0,
            crouch_height: 140.0,
            crouch_eye_height: 125.0,
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
    /// Crouch held (or toggled on).
    pub crouch: bool,
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
    /// Time left in the dash (dodgeDuration): BT is airborne and the ground check is skipped.
    pub dash_time: f32,
    /// Seconds since the last dash started (the dodgeInterval gate and the keyboard grace).
    pub since_dash: f32,
    /// Dash power, 0..100.
    pub power: f32,
    pub power_delay: f32,
    /// Number of dashes started (for effects/animation triggers).
    pub dashes: u32,
    /// Number of landings from the air, and the last one's downward impact speed (units/s), for
    /// the landing viewkick and heavy-landing effects.
    pub landings: u32,
    pub land_speed: f32,
    /// In the crouch stance (its hull, speeds and eye height), and how far the eye has moved
    /// down to the crouch view height (0 standing .. 1 crouched).
    pub crouched: bool,
    pub duck: f32,
    /// What the last move was pushed off (horizontal normal, zero if nothing), for Friction's
    /// wall rule (`walk_move`).
    pub block_normal: Vec3,
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
            since_dash: f32::MAX,
            power: 100.0,
            power_delay: 0.0,
            dashes: 0,
            landings: 0,
            land_speed: 0.0,
            crouched: false,
            duck: 0.0,
            block_normal: Vec3::ZERO,
        }
    }

    /// The eye: the stand view height, eased toward the crouch one by `duck` (Source's
    /// SimpleSpline, 3t^2 - 2t^3).
    pub fn eye(&self, p: &TitanParams) -> Vec3 {
        let d = self.duck.clamp(0.0, 1.0);
        let d = d * d * (3.0 - 2.0 * d);
        self.pos + Vec3::Z * (p.eye_height + (p.crouch_eye_height - p.eye_height) * d)
    }

    /// The hull height of the current stance.
    pub fn height(&self, p: &TitanParams) -> f32 {
        if self.crouched { p.crouch_height } else { p.height }
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.y).length()
    }
}

/// One tick of Respawn's ground movement (client.dll WalkMove): `wish` is the input direction
/// (length up to 1), `max_speed` the stance (or sprint) speed.
///
/// Friction (0x1801f8090) keeps the part of the velocity along the wish direction, up to the
/// wish speed (scaled by min(along * wish_speed / speed^2, 1)), and slows only the rest - the
/// sideways part and any excess speed - by `decel` per second. Accelerate (0x1801f0ac0) then
/// adds up to `accel` per second along the wish direction until the speed along it reaches the
/// wish speed, never raising the total speed above the larger of the old speed and the wish
/// speed. So turning while holding forward bends the path without the sideways slide a single
/// rate toward the wanted velocity gives.
pub fn walk_move(v: Vec2, wish: Vec2, max_speed: f32, accel: f32, decel: f32, block: Vec3, dt: f32) -> Vec2 {
    let dir = wish.normalize_or_zero();
    let wish_speed = wish.length().min(1.0) * max_speed;

    let along = v.dot(dir);
    let keep = if along <= 0.0 {
        Vec2::ZERO
    } else {
        let k = (along * wish_speed / v.length_squared()).min(1.0);
        let d = dir.extend(0.0);
        if d.dot(block) < 0.0 {
            // Pushing into what blocked the last move (`block`, the move trace's plane normal,
            // player+0x2b70, which WalkMove/AirMove store when the blocked handler didn't get
            // through: 0x18020e226, 0x1801f25f9): the kept velocity is along the wish turned
            // onto that surface (plus 1/128 of its normal, 0x1808f8510), so sliding along a
            // wall keeps the wish speed's share along it instead of being braked (Friction
            // 0x1801f81bf).
            let t = (d + block * (1.0 / 128.0 - d.dot(block))).normalize_or_zero();
            (t * t.dot(v.extend(0.0)).max(0.0) * k).truncate()
        } else {
            dir * along * k
        }
    };
    let rest = v - keep;
    let len = rest.length();
    let rest = if len > 1e-6 { rest * ((len - decel * dt).max(0.0) / len) } else { rest };
    let mut v = rest + keep;

    let max_sq = v.length_squared().max(wish_speed * wish_speed);
    let add = wish_speed - v.dot(dir);
    if add > 0.0 {
        v += dir * (accel * dt).min(add);
        if v.length_squared() > max_sq {
            v *= (max_sq / v.length_squared()).sqrt();
        }
    }
    v
}

/// Respawn's AirAccelerate (client.dll 0x1801f19a0): Source air acceleration toward the input
/// up to `air_speed`; already faster along it, the velocity only turns toward the input at
/// player_extraairaccelleration (2 u/s^2) without gaining speed.
pub fn air_move(v: Vec2, wish: Vec2, air_speed: f32, accel: f32, dt: f32) -> Vec2 {
    let dir = wish.normalize_or_zero();
    let wish_speed = wish.length().min(1.0) * air_speed;
    let add = wish_speed - v.dot(dir);
    if add > 0.0 {
        v + dir * (accel * dt).min(add)
    } else {
        let max_sq = v.length_squared().max(wish_speed * wish_speed);
        let n = v + dir * dt * 2.0;
        if n.length_squared() > max_sq { n * (max_sq / n.length_squared()).sqrt() } else { n }
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

/// Seconds the eye takes to move down into the crouch view height and back up. The settings
/// have no key for it; these are Source's gamemovement constants (TIME_TO_DUCK_MSECS 400,
/// TIME_TO_UNDUCK_MSECS 200), a guess for Respawn's engine.
pub const DUCK_TIME: f32 = 0.4;
pub const UNDUCK_TIME: f32 = 0.2;

/// Whether the standing hull fits at `s.pos` (standing up from a crouch under a low ceiling).
fn stand_room(s: &TitanState, p: &TitanParams, world: &CollisionWorld) -> bool {
    let r = p.radius;
    let lo = (p.crouch_height - r).max(p.step_height + r);
    let hi = (p.height - r).max(lo);
    [lo, (lo + hi) * 0.5, hi].iter().all(|&z| world.push_sphere_moving(s.pos + Vec3::Z * z, r * 0.9, 0.7, CLIP_TITAN).0 == Vec3::ZERO)
        && world.raycast_moving(s.pos + Vec3::Z * p.crouch_height, Vec3::Z, p.height - p.crouch_height, CLIP_TITAN).is_none()
}

/// Advance one tick.
pub fn step(s: &mut TitanState, input: &TitanInput, p: &TitanParams, world: &CollisionWorld, dt: f32) {
    s.yaw = input.yaw;
    s.pitch = input.pitch;
    // Stance: crouch at once; stand only where the standing hull fits. The eye follows over
    // DUCK_TIME / UNDUCK_TIME.
    if input.crouch && input.fly.is_none() {
        s.crouched = true;
    } else if s.crouched && stand_room(s, p, world) {
        s.crouched = false;
    }
    s.duck = if s.crouched { (s.duck + dt / DUCK_TIME).min(1.0) } else { (s.duck - dt / UNDUCK_TIME).max(0.0) };
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let right = Vec2::new(s.yaw.sin(), -s.yaw.cos());
    let mut wish = fwd * input.forward * if input.forward < 0.0 { p.back_scale } else { 1.0 }
        + right * input.right * p.side_scale;
    if wish.length() > 1.0 {
        wish = wish.normalize();
    }

    // Sprinting needs mostly-forward input and ground contact to start, and the stand stance.
    s.sprinting = input.sprint && !s.crouched && input.forward > 0.5 && (s.on_ground || s.sprinting);
    let scale = if input.speed_scale > 0.0 { input.speed_scale } else { 1.0 };

    // Dash power.
    s.since_dash = (s.since_dash + dt).min(1e6);
    if s.power_delay > 0.0 {
        s.power_delay -= dt;
    } else {
        s.power = (s.power + p.power_regen * dt).min(100.0);
    }

    let mut hvel = Vec2::new(s.vel.x, s.vel.y);
    // The dash (client.dll jump handler 0x180200200, dodge branch): allowed dodgeInterval after
    // the last one started, with the power for it.
    if input.dash && s.since_dash >= p.dash_interval && s.power >= p.dash_drain {
        // Up along the ground normal by sqrt(2 g dodgeHeight), in the air too
        // (dodge_vertical_in_air 1); dodgeHeightMax caps the result (from the setting's
        // description, the clamp itself isn't traced).
        s.vel.z = (s.vel.z + (2.0 * p.gravity * p.dash_height).sqrt()).min((2.0 * p.gravity * p.dash_height_max).sqrt());
        s.on_ground = false;
        // Along the input; with none, along the view (dodge_no_movement_forward_enable 1: the
        // eye's forward vector, so looking up or down shortens it).
        let dash = if wish.length_squared() > 1e-7 { wish.normalize() } else { fwd * s.pitch.cos() } * p.dash_speed * scale;
        // Faster than the dash, the old velocity keeps what exceeds the dash speed
        // (plus dodgeKeepSpeedFrac of the rest).
        let cur = hvel.length();
        let mut keep = if cur > dash.length() { 1.0 - dash.length() / cur } else { 0.0 };
        keep += (1.0 - keep) * p.dash_keep_frac;
        hvel = hvel * keep + dash;
        s.dash_time = p.dash_duration;
        s.since_dash = 0.0;
        s.power -= p.dash_drain;
        s.power_delay = p.power_delay;
        s.dashes += 1;
        s.sprinting = false;
    }
    // The dash ends after dodgeDuration, or early below dodgeStopSpeed (0x1801f53e0); nothing
    // clamps the speed when it ends.
    if s.dash_time > 0.0 {
        s.dash_time -= dt;
        if hvel.length_squared() + s.vel.z * s.vel.z < p.dash_stop_speed * p.dash_stop_speed {
            s.dash_time = 0.0;
        }
    }

    if let Some((_, limit)) = input.fly {
        // Hover (FlyerHovers): airSpeed 200 at airAcceleration 540, capped at limit.
        hvel = approach(hvel, wish * (limit - 50.0).max(0.0), 540.0 * dt);
        if hvel.length() > limit {
            hvel = hvel.normalize() * limit;
        }
    } else if s.on_ground && s.dash_time <= 0.0 {
        // Respawn's WalkMove (client.dll 0x18020d9e0): the stance's speed, acceleration and
        // deceleration (the crouch stance sets only those three: no low-speed boost and no
        // sprint rates).
        let (speed, accel, decel, sprint_accel, sprint_decel, low_speed, low_accel) = if s.crouched {
            (p.crouch_speed, p.crouch_accel, p.crouch_decel, None, None, None, None)
        } else {
            (p.speed, p.accel, p.decel, Some(p.sprint_accel), Some(p.sprint_decel), Some(p.low_speed), Some(p.low_accel))
        };
        let walk_speed = speed * scale;
        let max_speed = if s.sprinting { p.sprint_speed * scale } else { walk_speed };
        let cur = hvel.length();
        // GetAcceleration (0x1801fcbd0): sprintAcceleration above walk speed - 1, else
        // lowAcceleration below lowSpeed, else acceleration.
        let accel = match (sprint_accel, low_speed.zip(low_accel)) {
            (Some(a), _) if cur > walk_speed - 1.0 => a,
            (_, Some((ls, la))) if cur < ls * scale => la,
            _ => accel,
        };
        // GetDeceleration (0x1801fcf10): sprintDeceleration above walk speed, else deceleration.
        let decel = match sprint_decel {
            Some(d) if cur > walk_speed => d,
            _ => decel,
        };
        hvel = walk_move(hvel, wish, max_speed, accel, decel, s.block_normal, dt);
    } else {
        hvel = air_move(hvel, wish, p.air_speed * scale, p.air_accel, dt);
        // Keyboard grace (AirMove 0x1801f1d62): for jump_keyboardgraceperiodmax (0.5 s) after
        // a jump or dash, the movement keys turn the horizontal velocity toward their direction
        // at the same speed, by jump_keyboardgrace_strength (0.7) fading out from
        // jump_keyboardgraceperiodmin (0.2 s), at most jump_keyboardgrace_max (0.7) x the
        // sprint speed per tick.
        let keys = fwd * input.forward.signum() * (input.forward.abs() > 0.01) as u8 as f32
            + right * input.right.signum() * (input.right.abs() > 0.01) as u8 as f32;
        if s.since_dash < 0.5 && keys != Vec2::ZERO {
            let speed = hvel.length();
            let frac = ((0.5 - s.since_dash) / 0.3).clamp(0.0, 1.0) * 0.7;
            let cur = hvel / speed.max(1e-6);
            let dir = cur + (keys.normalize() - cur) * frac;
            let target = if dir.length() > 1e-4 { dir.normalize() * speed } else { dir };
            let mut delta = target - hvel;
            let max = p.sprint_speed * scale * 0.7;
            if delta.length() > max {
                delta = delta.normalize() * max;
            }
            hvel += delta;
        }
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
    s.block_normal = Vec3::ZERO;
    for _ in 0..steps {
        move_once(s, p, world, delta / steps as f32);
    }
}

/// How far (game units) the body spheres can be pushed within one move and still use the move's
/// `Region` (beyond, a sphere searches the trees itself, with the same result).
const BODY_MARGIN: f32 = 16.0;

fn move_once(s: &mut TitanState, p: &TitanParams, world: &CollisionWorld, delta: Vec3) {
    let was_on_ground = s.on_ground;
    s.pos += delta;

    // Body spheres start above the step height, so low obstacles are stepped onto.
    let r = p.radius;
    let lo = p.step_height + r;
    let hi = (s.height(p) - r).max(lo);
    // Titan clips (the map's TOOLSCLIP_TITAN and player clip brushes) hold BT in too. One
    // search of the trees serves the three spheres, with room for their pushes.
    let m = r + BODY_MARGIN;
    let body = world.region_moving(s.pos + Vec3::new(-m, -m, lo - m), s.pos + Vec3::new(m, m, hi + m), CLIP_TITAN);
    for z in [lo, (lo + hi) * 0.5, hi] {
        let (push, normals) = body.push_sphere(s.pos + Vec3::Z * z, r, 0.7);
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
                s.block_normal = n;
            }
        }
    }

    // Ground: highest walkable hit under a few points of the footprint.
    let reach_down = if was_on_ground { p.step_height } else { 1.0 };
    let mut ground: Option<f32> = None;
    let probe = r * 0.6;
    // One search of the trees for the five rays (their boxes are padded by a unit or so).
    let m = probe + 4.0;
    let feet = world.region_moving(s.pos + Vec3::new(-m, -m, -reach_down - 4.0), s.pos + Vec3::new(m, m, p.step_height + 4.0), CLIP_TITAN);
    for off in [Vec3::ZERO, Vec3::X * probe, -Vec3::X * probe, Vec3::Y * probe, -Vec3::Y * probe] {
        let origin = s.pos + off + Vec3::Z * p.step_height;
        if let Some(hit) = feet.raycast(origin, -Vec3::Z, p.step_height + reach_down) {
            if hit.normal.z > 0.7 {
                ground = Some(ground.map_or(hit.point.z, |g: f32| g.max(hit.point.z)));
            }
        }
    }
    match ground {
        // While dashing the ground check is skipped (CategorizePosition 0x1801f33dc): the floor
        // still holds BT up, but he stays airborne until the dash ends.
        Some(z) if s.vel.z <= 0.0 && s.dash_time > 0.0 => {
            s.pos.z = z;
            s.vel.z = 0.0;
            s.on_ground = false;
        }
        Some(z) if s.vel.z <= 0.0 => {
            if !was_on_ground {
                s.landings += 1;
                s.land_speed = -s.vel.z;
            }
            s.pos.z = z;
            s.vel.z = 0.0;
            s.on_ground = true;
        }
        _ => s.on_ground = false,
    }
}
