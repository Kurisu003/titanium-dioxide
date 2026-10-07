//! Pilot movement: walk, sprint, crouch, slide, jump, double jump, wall-running and the grapple.
//! Values from `scripts/players/mp/pilot_solo.set` (SP); fields it leaves out use the engine's
//! player-settings defaults from client.dll (acceleration 2500, airSpeed 60, airAcceleration 500)
//! and its convars (sv_friction 4, sv_stopspeed 100).

use crate::collision::CollisionWorld;
use glam::{Vec2, Vec3};

#[derive(Debug, Clone)]
pub struct PilotParams {
    pub walk_speed: f32,
    pub sprint_speed: f32,
    /// sprintStartDelay, sprintStartDuration, sprintStartFastDuration, sprintEndDuration
    /// (pilot_base.set): sprint speed comes in after the delay over the start duration (the
    /// fast one when already moving faster than a walk) and goes over the end duration.
    pub sprint_start_delay: f32,
    pub sprint_start: f32,
    pub sprint_start_fast: f32,
    pub sprint_end: f32,
    pub crouch_speed: f32,
    pub ground_accel: f32,
    pub friction: f32,
    pub air_accel: f32,
    /// Speed the player can adjust to in the air (airSpeed): Source-style air control.
    pub air_speed: f32,
    pub jump_height: f32,
    pub double_jump_height: f32,
    pub double_jump_horz: f32,
    pub gravity: f32,
    pub slide_boost: f32,
    pub slide_min_speed: f32,
    pub slide_decel: f32,
    /// slidevelocitydecay: each slide started soon after the last gets this fraction of the
    /// previous boost (slide-hop chains lose their boost).
    pub slide_velocity_decay: f32,
    /// Seconds off a slide before the full boost is back (client.dll slide_boost_cooldown).
    pub slide_boost_recover: f32,
    /// slideSpeedBoostCap: the boost doesn't take the Pilot past this speed.
    pub slide_boost_cap: f32,
    /// slideStopSpeed: a slide ends below this speed.
    pub slide_stop_speed: f32,
    /// slideJumpHeight: a jump out of a slide.
    pub slide_jump_height: f32,
    pub wallrun_time: f32,
    pub wallrun_max_h: f32,
    pub wallrun_max_v: f32,
    pub wallrun_accel_h: f32,
    /// wallrunAccelerateVertical: how fast vertical speed is pulled toward the wallrun's
    /// slow sag.
    pub wallrun_accel_v: f32,
    /// wallrun_hangTimeLimit: longest wall hang.
    pub wallrun_hang_time: f32,
    /// wallrunAdsType "wallhang" (the MP wall-hang kit): aiming on a wall hangs there. The
    /// campaign's "ADS" type just aims.
    pub wallhang_on_ads: bool,
    /// impactSpeed: landings faster than this are hard landings (no fall damage in the game).
    pub impact_speed: f32,
    pub wallrun_jump_out: f32,
    pub wallrun_jump_up: f32,
    pub wallrun_jump_input: f32,
    pub radius: f32,
    pub height: f32,
    pub crouch_height: f32,
    pub eye_height: f32,
    pub crouch_eye_height: f32,
    pub step_height: f32,
    /// pitchMaxUp / pitchMaxDown: look limits in degrees.
    pub pitch_max_up: f32,
    pub pitch_max_down: f32,
    /// Grapple (pilot_base.set grapple_*, and client.dll grapple_* convars for the reel-in).
    pub grapple: GrappleParams,
    pub zipline: ZiplineParams,
}

/// Zipline riding: player settings ziplineSpeed (pilot_solo/pilot_mp .set 600),
/// ziplineAcceleration, ziplineJumpOffSpeed, mountZiplineTime and useZiplineCooldown (client.dll
/// defaults), and the zipline_use_range convar.
#[derive(Debug, Clone)]
pub struct ZiplineParams {
    pub speed: f32,
    pub accel: f32,
    pub jump_off: f32,
    pub mount_time: f32,
    pub cooldown: f32,
    pub use_range: f32,
    /// Airborne, the Pilot grabs a line this close to the top of the hull (a guess).
    pub grab_radius: f32,
    /// The top of the hull hangs this far under the line (a guess: the eye 28 units under it).
    pub hang: f32,
}

impl Default for ZiplineParams {
    fn default() -> Self {
        Self { speed: 600.0, accel: 400.0, jump_off: 400.0, mount_time: 0.5, cooldown: 1.0, use_range: 120.0, grab_radius: 40.0, hang: 16.0 }
    }
}

/// A zipline from `a` to `b` (a map's `move_rope` with `Zipline` 1 and its `NextKey`), sagging
/// `sag` units at the middle (`ZiplineSagHeight`; the parabola is a guess); riders let go
/// `detach` units before the end (`ZiplineAutoDetachDistance`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zipline {
    pub a: Vec3,
    pub b: Vec3,
    pub sag: f32,
    pub detach: f32,
}

impl Zipline {
    pub fn point(&self, t: f32) -> Vec3 {
        self.a.lerp(self.b, t) - Vec3::Z * 4.0 * self.sag * t * (1.0 - t)
    }
    /// d point / d t.
    pub fn tangent(&self, t: f32) -> Vec3 {
        (self.b - self.a) - Vec3::Z * 4.0 * self.sag * (1.0 - 2.0 * t)
    }
    pub fn length(&self) -> f32 {
        (0..16).map(|i| (self.point((i + 1) as f32 / 16.0) - self.point(i as f32 / 16.0)).length()).sum()
    }
    /// The closest point's parameter and distance.
    pub fn closest(&self, p: Vec3) -> (f32, f32) {
        const N: usize = 48;
        let mut best = (0.0, f32::MAX);
        for i in 0..N {
            let (t0, t1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32);
            let (p0, p1) = (self.point(t0), self.point(t1));
            let d = p1 - p0;
            let k = ((p - p0).dot(d) / d.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let dist = (p0 + d * k - p).length();
            if dist < best.1 {
                best = (t0 + (t1 - t0) * k, dist);
            }
        }
        best
    }
}

/// Riding a zipline: where along it, which way, how fast, and the mount blend.
#[derive(Debug, Clone, Copy)]
pub struct ZipRide {
    pub line: Zipline,
    pub t: f32,
    /// +1 toward `b`, -1 toward `a`.
    pub dir: f32,
    pub speed: f32,
    /// Seconds since mounting, and where the Pilot was then.
    pub mount: f32,
    from: Vec3,
}

#[derive(Debug, Clone)]
pub struct GrappleParams {
    /// grapple_accel_human: pull toward the hook.
    pub pull_accel: f32,
    /// grapple_speedRampMin/Max_human over grapple_speedRampTime_human: the reel-in speed
    /// limit grows from min to max over that long after the hook lands.
    pub pull_speed: (f32, f32),
    pub pull_ramp_time: f32,
    /// grapple_gravityFracMin/Max: gravity while reeling, less when pulled upward.
    pub gravity_frac: (f32, f32),
    /// grapple_detachLengthMax: let go this close to the hook.
    pub detach_length: f32,
    /// grapple_impactVerticalBoost / MaxSpeed: hop when arriving at the hook.
    pub impact_boost: f32,
    pub impact_max: f32,
    /// grapple_detachVerticalBoost / MaxSpeed: hop when letting go early.
    pub detach_boost: f32,
    pub detach_max: f32,
    /// grapple_airSpeedMax / grapple_airAccel: steering while hooked.
    pub air_speed: f32,
    pub air_accel: f32,
    /// grapple_detachLowSpeedThreshold / Time / WallTime / GroundTime: give up when stuck
    /// slow this long (in the air, against a wall, on the ground).
    pub low_speed: f32,
    pub low_speed_time: f32,
    pub low_speed_wall_time: f32,
    pub low_speed_ground_time: f32,
    /// Speed lost into a wall that knocks the hook loose (estimate; the engine detaches on
    /// impact).
    pub wall_impact_speed: f32,
}

impl Default for GrappleParams {
    fn default() -> Self {
        Self {
            pull_accel: 1000.0,
            pull_speed: (50.0, 800.0),
            pull_ramp_time: 1.5,
            gravity_frac: (0.25, 0.7),
            detach_length: 50.0,
            impact_boost: 300.0,
            impact_max: 300.0,
            detach_boost: 200.0,
            detach_max: 200.0,
            air_speed: 420.0,
            air_accel: 650.0,
            low_speed: 250.0,
            low_speed_time: 1.5,
            low_speed_wall_time: 1.2,
            low_speed_ground_time: 0.7,
            wall_impact_speed: 150.0,
        }
    }
}

impl Default for PilotParams {
    fn default() -> Self {
        Self {
            walk_speed: 162.5,
            sprint_speed: 243.0, // pilot_solo.set stand.sprintspeed
            sprint_start_delay: 0.2,
            sprint_start: 0.8,
            sprint_start_fast: 0.2,
            sprint_end: 0.15,
            crouch_speed: 80.0,
            ground_accel: 2500.0, // stance `acceleration` default (client.dll)
            friction: 4.0,        // sv_friction (client.dll)
            air_accel: 500.0,     // airAcceleration default; pilot_solo.set has 540 commented out
            air_speed: 60.0,      // airSpeed default; pilot_solo.set has 70 commented out
            jump_height: 60.0,
            double_jump_height: 60.0, // superjumpMaxHeight
            double_jump_horz: 180.0,  // superjumpHorzSpeed
            gravity: 750.0 * 0.75,    // sv_gravity * gravityscale
            // The player-settings defaults (client.dll); pilot_solo/pilot_base.set don't set them.
            slide_boost: 150.0,     // slideSpeedBoost
            slide_min_speed: 200.0, // slideRequiredStartSpeed
            slide_decel: 50.0,        // slidedecel
            slide_velocity_decay: 0.7, // slidevelocitydecay (pilot_base.set)
            slide_boost_recover: 2.0,  // slide_boost_cooldown
            slide_boost_cap: 400.0,    // slideSpeedBoostCap
            slide_stop_speed: 125.0,   // slideStopSpeed
            slide_jump_height: 50.0,   // slideJumpHeight
            wallrun_time: 1.75,
            wallrun_max_h: 340.0,
            wallrun_max_v: 225.0,
            wallrun_accel_h: 1400.0,
            wallrun_accel_v: 360.0,   // wallrunAccelerateVertical
            wallrun_hang_time: 4.0,   // wallrun_hangTimeLimit (pilot_base.set)
            wallhang_on_ads: false,   // pilot_solo.set wallrunAdsType "ADS"
            impact_speed: 380.0,      // impactSpeed
            wallrun_jump_out: 205.0,
            wallrun_jump_up: 230.0,
            wallrun_jump_input: 75.0,
            radius: 16.0,
            height: 72.0,
            crouch_height: 47.0,
            eye_height: 60.0,
            crouch_eye_height: 38.0,
            step_height: 18.0,
            pitch_max_up: 85.0,
            pitch_max_down: 89.0,
            grapple: GrappleParams::default(),
            zipline: ZiplineParams::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PilotInput {
    pub forward: f32,
    pub right: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub sprint: bool,
    pub crouch: bool,
    /// Jump pressed this tick.
    pub jump: bool,
    /// Aiming down sights (wall hang with the wall-hang kit).
    pub ads: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PilotMove {
    Ground,
    Air,
    Slide,
    /// Wall-running along a wall with this outward normal.
    WallRun { normal: Vec3, time: f32 },
    /// Climbing over a ledge (automantle): from, to, seconds in.
    Mantle { from: Vec3, to: Vec3, time: f32 },
    /// Hanging still on a wall (wallrunAdsType "wallhang"): outward normal, seconds in.
    WallHang { normal: Vec3, time: f32 },
}

/// Seconds a mantle takes, by how far the eye is above the ledge (client.dll convars
/// automantle_height_below/level/above -10/10/30 pick the below/level/above/high animation,
/// automantle_duration_* 1.11/1.0/0.5/0.35 s). Measuring from the eye is a reading of the
/// convars' help text.
pub fn mantle_time(eye_above_ledge: f32) -> f32 {
    if eye_above_ledge < -10.0 {
        1.11
    } else if eye_above_ledge < 10.0 {
        1.0
    } else if eye_above_ledge < 30.0 {
        0.5
    } else {
        0.35
    }
}

#[derive(Debug, Clone)]
pub struct PilotState {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub mode: PilotMove,
    pub crouched: bool,
    pub sprinting: bool,
    /// How far sprint speed has come in (0..1) and seconds waited for it to start.
    pub sprint_frac: f32,
    sprint_wait: f32,
    /// Sprint was pressed while already faster than a walk (the fast start).
    sprint_fast: bool,
    pub double_jump_available: bool,
    /// Wall we last ran on; re-running it needs ground contact or a different wall.
    last_wall: Option<Vec3>,
    pub jumps: u32,
    /// Where the grapple hook is attached, while reeling in.
    pub grapple: Option<Vec3>,
    /// Seconds spent reeling below the low-speed threshold.
    grapple_slow: f32,
    grapple_time: f32,
    /// What the reel is dragging against: 0 nothing, 1 a wall, 2 the ground.
    grapple_touch: u8,
    /// Fraction of slideSpeedBoost the next slide gets, and seconds since the last slide.
    slide_boost_scale: f32,
    since_slide: f32,
    /// Downward speed of the latest landing (taken by the camera/sound code).
    pub landed: Option<f32>,
    /// The zipline being ridden, and seconds until another can be used.
    pub zipline: Option<ZipRide>,
    zip_cooldown: f32,
}

impl PilotState {
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            mode: PilotMove::Air,
            crouched: false,
            sprinting: false,
            sprint_frac: 0.0,
            sprint_wait: 0.0,
            sprint_fast: false,
            double_jump_available: true,
            last_wall: None,
            jumps: 0,
            grapple: None,
            grapple_slow: 0.0,
            grapple_time: 0.0,
            grapple_touch: 0,
            slide_boost_scale: 1.0,
            since_slide: 99.0,
            landed: None,
            zipline: None,
            zip_cooldown: 0.0,
        }
    }
    /// The latest landing, if it was a hard one (faster than impactSpeed): its speed.
    pub fn take_hard_landing(&mut self, p: &PilotParams) -> Option<f32> {
        self.landed.take().filter(|v| *v >= p.impact_speed)
    }
    /// Hook onto `anchor` and start reeling in.
    pub fn attach_grapple(&mut self, anchor: Vec3) {
        self.grapple = Some(anchor);
        self.grapple_slow = 0.0;
        self.grapple_time = 0.0;
        if self.on_ground() || matches!(self.mode, PilotMove::WallRun { .. }) {
            self.mode = PilotMove::Air;
            self.pos.z += 2.0;
        }
    }
    /// Let go of the grapple early, with grapple_detachVerticalBoost.
    pub fn release_grapple(&mut self, p: &PilotParams) {
        if self.grapple.take().is_some() {
            let g = &p.grapple;
            self.vel.z = (self.vel.z + g.detach_boost).min(self.vel.z.max(g.detach_max));
        }
    }
    /// Grab a zipline: with the use key, one within zipline_use_range of the eye; in the air,
    /// one touching the top of the hull. Rides the way the Pilot faces along it.
    pub fn try_zipline(&mut self, lines: &[Zipline], use_pressed: bool, p: &PilotParams) -> bool {
        let z = &p.zipline;
        if self.zipline.is_some() || self.zip_cooldown > 0.0 || matches!(self.mode, PilotMove::Mantle { .. }) {
            return false;
        }
        let eye = self.eye(p);
        let top = self.pos + Vec3::Z * p.height;
        let airborne = !self.on_ground();
        let mut best: Option<(Zipline, f32, f32)> = None;
        for line in lines {
            let (t, d) = if use_pressed { line.closest(eye) } else { line.closest(top) };
            let reach = if use_pressed { z.use_range } else { z.grab_radius };
            if (use_pressed || airborne) && d <= reach && best.is_none_or(|b| d < b.2) {
                best = Some((*line, t, d));
            }
        }
        let Some((line, t, _)) = best else { return false };
        let tan = line.tangent(t).normalize_or_zero();
        let fwd = Vec3::new(self.yaw.cos(), self.yaw.sin(), 0.0);
        let mut dir = fwd.dot(tan).signum();
        if fwd.dot(tan).abs() < 0.1 && self.vel.dot(tan).abs() > 1.0 {
            dir = self.vel.dot(tan).signum();
        }
        // Too close to the end it would carry the Pilot toward: not worth grabbing.
        let left = if dir > 0.0 { 1.0 - t } else { t } * line.length();
        if left <= line.detach {
            return false;
        }
        self.zipline = Some(ZipRide { line, t, dir, speed: self.vel.dot(tan * dir).max(0.0), mount: 0.0, from: self.pos });
        self.grapple = None;
        self.mode = PilotMove::Air;
        self.crouched = false;
        self.sprinting = false;
        true
    }
    /// Let go of the zipline (jumping off adds ziplineJumpOffSpeed upward).
    pub fn leave_zipline(&mut self, jump: bool, p: &PilotParams) {
        if self.zipline.take().is_some() {
            self.zip_cooldown = p.zipline.cooldown;
            self.double_jump_available = true;
            if jump {
                self.vel.z = self.vel.z.max(0.0) + p.zipline.jump_off;
            }
        }
    }
    pub fn on_ground(&self) -> bool {
        matches!(self.mode, PilotMove::Ground | PilotMove::Slide)
    }
    pub fn eye(&self, p: &PilotParams) -> Vec3 {
        self.pos + Vec3::Z * if self.crouched { p.crouch_eye_height } else { p.eye_height }
    }
    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.y).length()
    }
}

fn hv(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.y)
}

pub fn step(s: &mut PilotState, input: &PilotInput, p: &PilotParams, world: &CollisionWorld, dt: f32) {
    s.yaw = input.yaw;
    s.pitch = input.pitch;
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let right = Vec2::new(s.yaw.sin(), -s.yaw.cos());
    let mut wish = fwd * input.forward + right * input.right;
    if wish.length() > 1.0 {
        wish = wish.normalize();
    }
    let jump_v = |h: f32| (2.0 * p.gravity * h).sqrt();
    s.zip_cooldown = (s.zip_cooldown - dt).max(0.0);

    // Riding a zipline: carried along it, accelerating to ziplineSpeed, hanging under it.
    if let Some(mut r) = s.zipline {
        let z = &p.zipline;
        if input.jump || input.crouch {
            s.leave_zipline(input.jump, p);
        } else {
            r.speed = if r.speed < z.speed { (r.speed + z.accel * dt).min(z.speed) } else { (r.speed - z.accel * dt).max(z.speed) };
            let tan = r.line.tangent(r.t);
            let len = tan.length().max(1.0);
            r.t = (r.t + r.dir * r.speed * dt / len).clamp(0.0, 1.0);
            r.mount += dt;
            let hang = r.line.point(r.t) - Vec3::Z * (p.height + z.hang);
            let k = (r.mount / z.mount_time.max(1e-3)).min(1.0);
            let k = k * k * (3.0 - 2.0 * k);
            s.pos = r.from.lerp(hang, k);
            s.vel = r.line.tangent(r.t).normalize_or_zero() * r.dir * r.speed;
            s.mode = PilotMove::Air;
            s.double_jump_available = true;
            s.zipline = Some(r);
            let left = if r.dir > 0.0 { 1.0 - r.t } else { r.t } * r.line.length();
            if left <= r.line.detach {
                s.leave_zipline(false, p);
            }
        }
        return;
    }

    // Reeling in on the grapple replaces the normal movement modes.
    if let Some(anchor) = s.grapple {
        let g = &p.grapple;
        let to = anchor - (s.pos + Vec3::Z * p.height * 0.5);
        let dist = to.length();
        if input.jump {
            s.release_grapple(p);
            s.double_jump_available = true;
        } else if dist <= g.detach_length {
            s.grapple = None;
            s.vel.z = (s.vel.z + g.impact_boost).min(s.vel.z.max(g.impact_max));
        } else {
            let dir = to / dist;
            // Pull toward the hook up to the reel speed, which ramps up after the hook lands.
            s.grapple_time += dt;
            let ramp = (s.grapple_time / g.pull_ramp_time.max(1e-3)).min(1.0);
            let limit = g.pull_speed.0 + (g.pull_speed.1 - g.pull_speed.0) * ramp;
            let along = s.vel.dot(dir);
            if along < limit {
                s.vel += dir * (g.pull_accel * dt).min(limit - along);
            }
            // Steering, capped like Source air control at grapple_airSpeedMax.
            if wish != Vec2::ZERO {
                let d = wish.normalize();
                let add = g.air_speed * wish.length() - hv(s.vel).dot(d);
                if add > 0.0 {
                    let a = d * (g.air_accel * dt).min(add);
                    s.vel.x += a.x;
                    s.vel.y += a.y;
                }
            }
            let up = dir.z.max(0.0);
            let frac = g.gravity_frac.1 + (g.gravity_frac.0 - g.gravity_frac.1) * up;
            s.vel.z -= p.gravity * frac * dt;
            s.mode = PilotMove::Air;
            // Stuck slow: let go sooner against a wall or on the ground
            // (grapple_detachLowSpeedTime / WallTime / GroundTime).
            if s.vel.length() < g.low_speed {
                s.grapple_slow += dt;
                let limit = if s.grapple_touch == 2 {
                    g.low_speed_ground_time
                } else if s.grapple_touch == 1 {
                    g.low_speed_wall_time
                } else {
                    g.low_speed_time
                };
                if s.grapple_slow > limit {
                    s.grapple = None;
                }
            } else {
                s.grapple_slow = 0.0;
            }
        }
        if s.grapple.is_some() {
            let delta = s.vel * dt;
            let steps = ((delta.length() / (p.radius * 0.5)).ceil() as usize).max(1);
            let still = PilotInput { forward: 0.0, ..*input };
            let mut impact = 0.0f32;
            for _ in 0..steps {
                impact = impact.max(move_once(s, p, world, delta / steps as f32, &still));
            }
            s.grapple_touch = if s.on_ground() { 2 } else if impact > 0.0 { 1 } else { 0 };
            // Slamming into a wall knocks the hook loose, like arriving at it; from there the
            // normal modes take over (a wall-run if moving along the wall).
            if impact > g.wall_impact_speed {
                s.grapple = None;
                s.grapple_slow = 0.0;
                s.vel.z = (s.vel.z + g.impact_boost).min(s.vel.z.max(g.impact_max));
                s.mode = PilotMove::Air;
                return;
            }
            if s.grapple.is_some() {
                s.mode = PilotMove::Air;
            }
            return;
        }
    }

    match s.mode {
        PilotMove::Ground | PilotMove::Slide => {
            s.double_jump_available = true;
            s.last_wall = None;
            let speed = s.horizontal_speed();
            if s.mode == PilotMove::Slide {
                s.since_slide = 0.0;
            } else {
                s.since_slide += dt;
                if s.since_slide >= p.slide_boost_recover {
                    s.slide_boost_scale = 1.0;
                }
            }
            // Slide: crouching while moving fast enough (sprinting, or carrying speed in from
            // a jump or fall). Chained slides get a decaying boost (slidevelocitydecay).
            let fast = s.sprinting || speed > p.walk_speed + 10.0;
            if s.mode == PilotMove::Ground && input.crouch && fast && speed >= p.slide_min_speed {
                let dir = hv(s.vel).normalize_or(fwd);
                let v = dir * (speed + p.slide_boost * s.slide_boost_scale).min(speed.max(p.slide_boost_cap));
                s.slide_boost_scale *= p.slide_velocity_decay;
                s.vel.x = v.x;
                s.vel.y = v.y;
                s.mode = PilotMove::Slide;
                s.since_slide = 0.0;
            }
            let sliding = s.mode == PilotMove::Slide;
            if sliding && (!input.crouch || speed < p.slide_stop_speed) {
                s.mode = PilotMove::Ground;
            }
            s.crouched = input.crouch;
            s.sprinting = input.sprint && input.forward > 0.5 && !s.crouched;
            if s.sprinting {
                if s.sprint_wait == 0.0 {
                    s.sprint_fast = speed > p.walk_speed + 10.0;
                }
                s.sprint_wait += dt;
                let fast = s.sprint_fast;
                if fast || s.sprint_frac > 0.0 || s.sprint_wait >= p.sprint_start_delay {
                    let dur = if fast { p.sprint_start_fast } else { p.sprint_start };
                    s.sprint_frac = (s.sprint_frac + dt / dur.max(1e-3)).min(1.0);
                }
            } else {
                s.sprint_wait = 0.0;
                s.sprint_frac = (s.sprint_frac - dt / p.sprint_end.max(1e-3)).max(0.0);
            }
            let mut h = hv(s.vel);
            if input.jump {
                // Jumping skips this tick's friction (Source's CheckJumpButton runs first), so
                // jumping on the tick you land keeps your speed: bunny hops and slide hops.
            } else if s.mode == PilotMove::Slide {
                let sp = (h.length() - p.slide_decel * dt).max(0.0);
                h = h.normalize_or_zero() * sp;
            } else {
                let max = if s.crouched { p.crouch_speed } else { p.walk_speed + (p.sprint_speed - p.walk_speed) * s.sprint_frac };
                // Source-style friction then acceleration toward the wished velocity.
                let sp = h.length();
                if sp > 0.0 {
                    let drop = sp.max(100.0) * p.friction * dt;
                    h *= ((sp - drop) / sp).max(0.0);
                }
                let room = (max - h.dot(wish)).max(0.0);
                if wish != Vec2::ZERO && room > 0.0 {
                    h += wish * (p.ground_accel * dt).min(room);
                }
            }
            s.vel.x = h.x;
            s.vel.y = h.y;
            if input.jump {
                s.vel.z = jump_v(if sliding { p.slide_jump_height } else { p.jump_height });
                s.mode = PilotMove::Air;
                s.crouched = false;
                s.jumps += 1;
            }
        }
        PilotMove::Air => {
            s.crouched = false;
            let mut h = hv(s.vel);
            // Source air acceleration: only the speed along the wish direction is capped (at
            // airSpeed), so momentum carries and strafing curves the path.
            if wish != Vec2::ZERO {
                let dir = wish.normalize();
                let add = p.air_speed * wish.length() - h.dot(dir);
                if add > 0.0 {
                    h += dir * (p.air_accel * dt).min(add);
                }
            }
            s.vel.x = h.x;
            s.vel.y = h.y;
            s.vel.z -= p.gravity * dt;
            if input.jump && s.double_jump_available {
                s.double_jump_available = false;
                s.vel.z = jump_v(p.double_jump_height);
                if wish != Vec2::ZERO {
                    let sp = h.length().max(p.double_jump_horz);
                    let v = wish * sp;
                    s.vel.x = v.x;
                    s.vel.y = v.y;
                }
                s.jumps += 1;
            }
        }
        PilotMove::Mantle { from, to, time } => {
            let t = time + dt;
            let k = (t / mantle_time(from.z + p.eye_height - to.z)).min(1.0);
            // Up first, then over the edge.
            let up = (k / 0.6).min(1.0);
            let over = ((k - 0.4) / 0.6).clamp(0.0, 1.0);
            let ease = |x: f32| x * x * (3.0 - 2.0 * x);
            s.pos = Vec3::new(from.x + (to.x - from.x) * ease(over), from.y + (to.y - from.y) * ease(over), from.z + (to.z - from.z) * ease(up));
            s.vel = Vec3::ZERO;
            if k >= 1.0 {
                s.mode = PilotMove::Ground;
                let f = Vec3::new(fwd.x, fwd.y, 0.0);
                s.vel = f * p.walk_speed * 0.6;
            } else {
                s.mode = PilotMove::Mantle { from, to, time: t };
            }
            return;
        }
        PilotMove::WallHang { normal, time } => {
            let t = time + dt;
            s.double_jump_available = true;
            s.vel = Vec3::ZERO;
            if input.jump {
                let v = normal * p.wallrun_jump_out + Vec3::new(wish.x, wish.y, 0.0) * p.wallrun_jump_input;
                s.vel = Vec3::new(v.x, v.y, p.wallrun_jump_up);
                s.mode = PilotMove::Air;
                s.last_wall = Some(normal);
                s.jumps += 1;
            } else if !input.ads || input.crouch || t >= p.wallrun_hang_time {
                s.mode = PilotMove::Air;
                s.last_wall = Some(normal);
            } else {
                s.mode = PilotMove::WallHang { normal, time: t };
                return;
            }
        }
        PilotMove::WallRun { normal, time } => {
            let t = time + dt;
            s.double_jump_available = true;
            // Run along the wall in the direction the player is looking.
            let n2 = hv(normal);
            let along = Vec2::new(-n2.y, n2.x);
            let along = if along.dot(fwd) >= 0.0 { along } else { -along };
            let mut h = hv(s.vel);
            h -= n2 * h.dot(n2); // no velocity into or away from the wall
            let cur = h.dot(along);
            if input.forward > 0.0 && cur < p.wallrun_max_h {
                h += along * (p.wallrun_accel_h * dt).min(p.wallrun_max_h - cur);
            }
            s.vel.x = h.x - n2.x * 20.0; // slight pull to keep contact
            s.vel.y = h.y - n2.y * 20.0;
            // Vertical speed is pulled (at wallrunAccelerateVertical) toward a sag that grows as
            // the wallrun ages, so an upward entry carries you up the wall for a moment.
            let sag = (t / p.wallrun_time).powi(2);
            let target = -p.wallrun_max_v * sag;
            let dv = (target - s.vel.z).clamp(-p.wallrun_accel_v * dt, p.wallrun_accel_v * dt);
            s.vel.z = (s.vel.z + dv).clamp(-p.wallrun_max_v, p.wallrun_max_v);
            s.mode = PilotMove::WallRun { normal, time: t };
            let wall_jump = input.jump;
            if p.wallhang_on_ads && input.ads && !wall_jump {
                s.mode = PilotMove::WallHang { normal, time: 0.0 };
                s.vel = Vec3::ZERO;
                return;
            }
            if wall_jump {
                let v = normal * p.wallrun_jump_out + Vec3::new(wish.x, wish.y, 0.0) * p.wallrun_jump_input;
                s.vel = Vec3::new(h.x + v.x, h.y + v.y, p.wallrun_jump_up);
                s.mode = PilotMove::Air;
                s.last_wall = Some(normal);
                s.jumps += 1;
            } else if t >= p.wallrun_time || input.crouch {
                s.mode = PilotMove::Air;
                s.last_wall = Some(normal);
            }
        }
    }

    // Automantle: pressing into a ledge within reach climbs onto it.
    if input.forward > 0.3 && !matches!(s.mode, PilotMove::Mantle { .. }) && s.vel.z < 250.0 {
        if let Some(to) = mantle_target(s, p, world, fwd) {
            s.mode = PilotMove::Mantle { from: s.pos, to, time: 0.0 };
            s.vel = Vec3::ZERO;
            return;
        }
    }
    let delta = s.vel * dt;
    let steps = ((delta.length() / (p.radius * 0.5)).ceil() as usize).max(1);
    for _ in 0..steps {
        move_once(s, p, world, delta / steps as f32, input);
    }
}

/// Moves by `delta` with collision; returns the fastest speed lost into a wall (0 if none).
fn move_once(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, delta: Vec3, input: &PilotInput) -> f32 {
    let mut wall_impact = 0.0f32;
    s.pos += delta;
    let r = p.radius;
    let height = if s.crouched { p.crouch_height } else { p.height };
    let lo = p.step_height + r;
    let hi = (height - r).max(lo);
    let mut wall: Option<Vec3> = None;
    for z in [lo, hi] {
        let (push, normals) = world.push_sphere(s.pos + Vec3::Z * z, r, 0.7);
        if push != Vec3::ZERO {
            let mut push = push;
            if push.z < 0.0 && s.vel.z > 0.0 {
                s.vel.z = 0.0;
            }
            push.z = push.z.min(0.0);
            s.pos += push;
            let n = Vec3::new(normals.x, normals.y, 0.0).normalize_or_zero();
            let into = s.vel.dot(n);
            if into < 0.0 {
                s.vel -= n * into;
                wall_impact = wall_impact.max(-into);
            }
            if n != Vec3::ZERO {
                wall = Some(n);
            }
        }
    }

    // Ground.
    let on_ground_before = s.on_ground();
    let reach_down = if on_ground_before { p.step_height } else { 1.0 };
    let mut ground: Option<f32> = None;
    for off in [Vec3::ZERO, Vec3::X * r * 0.6, -Vec3::X * r * 0.6, Vec3::Y * r * 0.6, -Vec3::Y * r * 0.6] {
        if let Some(hit) = world.raycast(s.pos + off + Vec3::Z * p.step_height, -Vec3::Z, p.step_height + reach_down) {
            if hit.normal.z > 0.7 {
                ground = Some(ground.map_or(hit.point.z, |g: f32| g.max(hit.point.z)));
            }
        }
    }
    match ground {
        Some(z) if s.vel.z <= 0.0 => {
            s.pos.z = z;
            if !on_ground_before {
                s.landed = Some(-s.vel.z);
                s.mode = PilotMove::Ground;
            }
            s.vel.z = 0.0;
        }
        _ => {
            if on_ground_before {
                s.mode = PilotMove::Air;
            }
        }
    }

    // Wall-running: airborne, moving fast enough, pressing forward, next to a steep wall.
    let in_air = matches!(s.mode, PilotMove::Air);
    let wallrunning = matches!(s.mode, PilotMove::WallRun { .. });
    let sensed = world.nearest_wall(s.pos + Vec3::Z * (height * 0.5), r + 6.0, 0.35).map(|(n, _)| n).or(wall);
    if in_air && input.forward > 0.0 && s.horizontal_speed() > 100.0 {
        if let Some(n) = sensed {
            let same_wall = s.last_wall.is_some_and(|w| w.dot(n) > 0.9);
            // Need to be moving along (not straight into) the wall.
            let h = hv(s.vel).normalize_or_zero();
            let along = 1.0 - h.dot(hv(n)).abs();
            if !same_wall && along > 0.3 {
                s.mode = PilotMove::WallRun { normal: n, time: 0.0 };
                s.vel.z = s.vel.z.max(0.0).min(p.wallrun_max_v);
            }
        }
    } else if wallrunning && sensed.is_none() {
        let normal = if let PilotMove::WallRun { normal, .. } = s.mode { normal } else { Vec3::ZERO };
        s.mode = PilotMove::Air;
        s.last_wall = Some(normal);
    }
    wall_impact
}

/// A ledge in front of the player that can be mantled onto: a wall at chest height, a flat top
/// no higher than a little above the head, and room to stand there.
fn mantle_target(s: &PilotState, p: &PilotParams, world: &CollisionWorld, fwd: Vec2) -> Option<Vec3> {
    let f = Vec3::new(fwd.x, fwd.y, 0.0);
    let chest = s.pos + Vec3::Z * (p.height * 0.55);
    // Something blocking right in front.
    world.raycast(chest, f, p.radius + 14.0)?;
    // Look down onto the top from above the reach height, just past the wall.
    let reach = p.height + 26.0;
    let probe = s.pos + f * (p.radius + 18.0) + Vec3::Z * (reach + 4.0);
    let hit = world.raycast(probe, -Vec3::Z, reach)?;
    let rise = hit.point.z - s.pos.z;
    if hit.normal.z < 0.7 || rise < p.step_height + 4.0 || rise > reach {
        return None;
    }
    // Head room on top, and a clear path over the edge.
    let top = Vec3::new(probe.x, probe.y, hit.point.z);
    if world.raycast(top + Vec3::Z * 2.0, Vec3::Z, p.height - 4.0).is_some() {
        return None;
    }
    if world.raycast(s.pos + Vec3::Z * (rise + 6.0), f, p.radius + 18.0).is_some() {
        return None;
    }
    Some(top)
}
