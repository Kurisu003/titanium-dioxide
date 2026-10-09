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
    /// (pilot_base.set) time the sprint VIEW (client.dll help: "How long to wait before / take to
    /// transition into sprintViewOffset", the fast one "after landing from a sprinting jump",
    /// and out of it); sprint speed itself just follows the ground acceleration.
    pub sprint_start_delay: f32,
    pub sprint_start: f32,
    pub sprint_start_fast: f32,
    pub sprint_end: f32,
    /// sprintViewOffset: "How far to slide the player's [view] down as they enter a sprinting
    /// state" (pilot_base.set -6).
    pub sprint_view_offset: f32,
    /// pitchOffsetScale: "Max distance to translate the player's view forward as he looks
    /// down" (pilot_base.set 9.2).
    pub pitch_offset_scale: f32,
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
    /// slideDecel: speed lost per second while sliding (a fixed amount).
    pub slide_decel: f32,
    /// slideVelocityDecay: fraction of the speed kept per second while sliding (on top of
    /// slideDecel, so it bites harder at high speed).
    pub slide_velocity_decay: f32,
    /// slide_boost_cooldown: a slide started sooner than this after the last one gets no boost.
    pub slide_boost_cooldown: f32,
    /// slideSpeedBoostCap: the boost doesn't take the Pilot past this speed.
    pub slide_boost_cap: f32,
    /// slideStopSpeed: a slide ends below this speed.
    pub slide_stop_speed: f32,
    /// slideMaxStopSpeed: a slide can't end while faster than this; letting go of crouch above
    /// it slows the slide at slideWantToStopDecel instead of slideDecel until it can.
    pub slide_max_stop_speed: f32,
    pub slide_want_stop_decel: f32,
    /// slideAccel: acceleration from movement input while sliding (steering).
    pub slide_accel: f32,
    /// slideMaxJumpSpeed: a slide's long jump raises a speed within 85% of this to it.
    pub slide_max_jump_speed: f32,
    /// slide_max_angle_dot: crouching only starts a slide with the movement input within this
    /// cosine of the view's forward.
    pub slide_max_angle_dot: f32,
    /// slide_step_velocity_reduction: speed lost per unit of step height climbed while sliding.
    pub slide_step_loss: f32,
    /// slideJumpHeight: a jump out of a slide.
    pub slide_jump_height: f32,
    /// jump_graceperiod: seconds after walking off a ledge during which jump still does a
    /// normal jump (the double jump is kept).
    pub jump_grace: f32,
    /// skip_time, skip_jump_height_fraction, skip_speed_reduce, skip_speed_retain: a jump within
    /// skip_time of landing is a "skip": a lower jump that keeps the landing speed, losing
    /// skip_speed_reduce of it but not going below skip_speed_retain that way.
    pub skip_time: f32,
    pub skip_jump_frac: f32,
    pub skip_speed_reduce: f32,
    pub skip_speed_retain: f32,
    /// superjump_min_height_fraction: a double jump while already rising fast still adds this
    /// fraction of its height.
    pub double_jump_min_frac: f32,
    /// sv_stopspeed: friction works on at least this speed.
    pub stop_speed: f32,
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
    pub wallrun: WallrunParams,
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

/// Wall-run tuning that the .set files don't carry: the player setting wallrunFriction and
/// wallrunSameWallAllowed (client.dll defaults) and the wallrun_* convars. The comments are the
/// convars' own help text from client.dll.
#[derive(Debug, Clone)]
pub struct WallrunParams {
    /// wallrunFriction: "Base friction applied while wall running" (Source-style friction on
    /// the velocity in the wall's plane).
    pub friction: f32,
    /// wallrun_upWallBoost: "Initial vertical velocity boost when wall running begins".
    pub up_boost: f32,
    /// wallrun_gravityRampUpTime: "gravity starts at 0 and ramps up to full over this many
    /// seconds".
    pub gravity_ramp: f32,
    /// wallrun_upwardAutoPush: "automatic up-the-wall input applied when the player pushes
    /// forward along the wall".
    pub upward_push: f32,
    /// wallrun_slipstarttime: "Time wall running before you start to slip down".
    pub slip_start: f32,
    /// wallrun_slipduration: "Time it takes for slipping to become completely gravity based".
    pub slip_duration: f32,
    /// wallrun_slipvel: "Impulse downward when slipping starts".
    pub slip_vel: f32,
    /// wallrun_slipslowdown: "Fraction of velocity lost when slipping starts".
    pub slip_slowdown: f32,
    /// wallrun_noInputSlipFrac: "Min fraction of slip behavior when not pushing in any
    /// direction".
    pub no_input_slip: f32,
    /// wallrun_fallAwaySpeed: "Velocity away from the wall when falling off".
    pub fall_away: f32,
    /// wallrun_pushAwayFallOffTime: "Pushing away from the wall for this many seconds causes
    /// you to fall off".
    pub push_away_time: f32,
    /// wallrunSameWallAllowed, wallrun_sameWallDot/Height: running the wall last left (normals
    /// within sameWallDot) again is only allowed below where that wall-run started (+
    /// sameWallHeight), and gets no up-wall boost (CanWallrun 0x180202090). The
    /// sameWallDist/Slope alternative can't pass with the default slope 0, so it is left out.
    pub same_wall_allowed: bool,
    pub same_wall_dot: f32,
    pub same_wall_height: f32,
    /// wallrun_minAngle_air: the velocity must come at the wall within half this angle of
    /// head-on (180: any velocity with a component into the wall).
    pub min_angle_air: f32,
    /// wallrun_allowed_wall_dist: the start probes must find the wall within this distance of
    /// the hull's face.
    pub allowed_wall_dist: f32,
    /// wallrun_retry_interval: a start that failed only on the probes is tried once more this
    /// long after the hit.
    pub retry_interval: f32,
    /// No wall-run until this long after leaving the ground without jumping (or falling off a
    /// wall): the constant at client.dll 0x1802020e4.
    pub ground_lockout: f32,
    /// automantle_maxangle_push / automantle_maxangle_view (degrees): pushing and looking into a
    /// wall this squarely is a mantle, not a wall-run, unless the head-height probe finds the wall
    /// too.
    pub mantle_push_angle: f32,
    pub mantle_view_angle: f32,
    /// wallrun_rotateMaxRate: "Maximum rotation speed around a wall in radians per second;
    /// avoids sticking to walls that do tight curves".
    pub rotate_max_rate: f32,
    /// wallrun_angleChangeMinCos: "Cosine of maximum angle the wall can change away from you
    /// without falling off".
    pub angle_change_min_cos: f32,
}

impl Default for WallrunParams {
    fn default() -> Self {
        Self {
            friction: 4.0,
            up_boost: 250.0,
            gravity_ramp: 1.0,
            upward_push: 0.65,
            slip_start: 1.5,
            slip_duration: 1.0,
            slip_vel: 70.0,
            slip_slowdown: 0.5,
            no_input_slip: 0.7,
            fall_away: 70.0,
            push_away_time: 0.05,
            same_wall_allowed: true,
            same_wall_dot: 0.9,
            same_wall_height: 0.0,
            min_angle_air: 180.0,
            allowed_wall_dist: 13.0,
            retry_interval: 0.07,
            ground_lockout: 0.5,
            mantle_push_angle: 50.0,
            mantle_view_angle: 50.0,
            rotate_max_rate: 3.0,
            angle_change_min_cos: 0.8,
        }
    }
}

/// Input pointing more than this far away from the wall (dot with its normal) counts as pushing
/// away (wallrun_pushAwayFallOffTime; the wall-run tick, client.dll 0x18020e9e0).
const PUSH_AWAY_DOT: f32 = 0.7;

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
            sprint_view_offset: -6.0,
            pitch_offset_scale: 9.2,
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
            slide_decel: 50.0,          // slidedecel (pilot_base.set)
            slide_velocity_decay: 0.7,  // slidevelocitydecay (pilot_base.set)
            slide_boost_cooldown: 2.0,  // slide_boost_cooldown
            slide_boost_cap: 400.0,     // slideSpeedBoostCap
            slide_stop_speed: 125.0,    // slideStopSpeed
            slide_max_stop_speed: 350.0, // slideMaxStopSpeed
            slide_want_stop_decel: 400.0, // slideWantToStopDecel
            slide_accel: 50.0,          // slideAccel
            slide_max_jump_speed: 350.0, // slideMaxJumpSpeed
            slide_max_angle_dot: 0.6,   // slide_max_angle_dot
            slide_step_loss: 10.0,      // slide_step_velocity_reduction
            slide_jump_height: 50.0,    // slideJumpHeight
            jump_grace: 0.2,            // jump_graceperiod
            skip_time: 0.1,             // skip_time
            skip_jump_frac: 0.75,       // skip_jump_height_fraction
            skip_speed_reduce: 12.0,    // skip_speed_reduce
            skip_speed_retain: 450.0,   // skip_speed_retain
            double_jump_min_frac: 0.25, // superjump_min_height_fraction
            stop_speed: 100.0,          // sv_stopspeed
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
            wallrun: WallrunParams::default(),
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
    /// How far the view has gone into sprintViewOffset (0..1), and seconds waited for it.
    pub sprint_frac: f32,
    sprint_wait: f32,
    /// Airborne out of a sprint: landing still sprinting uses sprintStartFastDuration.
    sprint_fast: bool,
    pub double_jump_available: bool,
    /// The (flattened) normal of the wall last left, cleared on landing, and where the last
    /// wall-run started: the same-wall rule.
    last_wall: Option<Vec3>,
    wallrun_start: Vec3,
    /// Seconds since the Pilot last stood on the ground or a wall (a jump counts as long ago):
    /// the wall-run lockout after walking off a ledge.
    since_ground: f32,
    /// The next wall-run starts without wallrun_upWallBoost (set on landing and falling off a
    /// wall, cleared by jumping).
    wallrun_no_boost: bool,
    /// A pending wall-run retry: seconds left, where the Pilot was and the wall's normal.
    wallrun_retry: Option<(f32, Vec3, Vec3)>,
    /// Seconds spent pushing away from the wall while wall-running.
    push_away: f32,
    /// Seconds of jump_graceperiod left after walking off a ledge.
    jump_grace: f32,
    /// Seconds since the last landing, and the horizontal speed then (for skips).
    since_land: f32,
    land_speed: f32,
    pub jumps: u32,
    /// Where the grapple hook is attached, while reeling in.
    pub grapple: Option<Vec3>,
    /// Seconds spent reeling below the low-speed threshold.
    grapple_slow: f32,
    grapple_time: f32,
    /// What the reel is dragging against: 0 nothing, 1 a wall, 2 the ground.
    grapple_touch: u8,
    /// Seconds since the last slide started (slide_boost_cooldown), the speed its boost added,
    /// and whether its long jump is still available.
    since_slide: f32,
    slide_boost_given: f32,
    slide_long_jump: bool,
    /// How far the view is into the crouch (0 standing, 1 crouched; linear, eased by
    /// `duck_frac`), and how long the current crouch takes.
    pub duck: f32,
    duck_time: f32,
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
            wallrun_start: Vec3::ZERO,
            since_ground: 99.0,
            wallrun_no_boost: false,
            wallrun_retry: None,
            push_away: 0.0,
            jump_grace: 0.0,
            since_land: 99.0,
            land_speed: 0.0,
            jumps: 0,
            grapple: None,
            grapple_slow: 0.0,
            grapple_time: 0.0,
            grapple_touch: 0,
            since_slide: 99.0,
            slide_boost_given: 0.0,
            slide_long_jump: false,
            duck: 0.0,
            duck_time: 0.4,
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
    /// The view's crouch fraction: Duck's smoothstep (client.dll 0x1801f8020).
    pub fn duck_frac(&self) -> f32 {
        let k = self.duck.clamp(0.0, 1.0);
        k * k * (3.0 - 2.0 * k)
    }
    /// The eye: the stand and crouch view heights blended by the crouch fraction.
    pub fn eye_offset(&self, p: &PilotParams) -> f32 {
        p.eye_height + (p.crouch_eye_height - p.eye_height) * self.duck_frac()
    }
    pub fn eye(&self, p: &PilotParams) -> Vec3 {
        self.pos + Vec3::Z * self.eye_offset(p)
    }
    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.y).length()
    }
}

fn hv(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.y)
}

/// The movement input as a world direction (length up to 1).
fn wish_of(yaw: f32, input: &PilotInput) -> Vec2 {
    let wish = Vec2::new(yaw.cos(), yaw.sin()) * input.forward + Vec2::new(yaw.sin(), -yaw.cos()) * input.right;
    if wish.length() > 1.0 { wish.normalize() } else { wish }
}

/// One of CanWallrun's wall probes (client.dll 0x18020e2b0): from `origin` toward the wall
/// (along the flattened -`n`), how far the wall is from the hull's face, and its normal. The
/// game traces a cube of the hull's half-width; a 3x3 grid of rays across that cube's face
/// stands in for it here.
fn wall_probe(world: &CollisionWorld, p: &PilotParams, origin: Vec3, n: Vec3) -> Option<(f32, Vec3)> {
    let n = Vec3::new(n.x, n.y, 0.0).normalize_or_zero();
    if n == Vec3::ZERO {
        return None;
    }
    let side = Vec3::new(-n.y, n.x, 0.0) * p.radius * 0.9;
    let up = Vec3::Z * p.radius * 0.9;
    let reach = p.radius * 1.2929 + p.wallrun.allowed_wall_dist;
    let mut best: Option<(f32, Vec3)> = None;
    for i in -1..=1 {
        for j in -1..=1 {
            if let Some(h) = world.raycast(origin + side * i as f32 + up * j as f32, -n, reach) {
                if best.is_none_or(|b| h.t - p.radius < b.0) {
                    best = Some((h.t - p.radius, h.normal));
                }
            }
        }
    }
    best
}

/// client.dll CanWallrun (0x180202090) for an airborne Pilot whose move hit a surface with
/// normal `n` while moving along `dir`: 0 no, 1 not yet (the probes didn't find the wall close
/// enough: retry), 2 yes. No speed, input or view requirement.
fn can_wallrun(s: &PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput, n: Vec3, dir: Vec3) -> u8 {
    let w = &p.wallrun;
    // Not within the lockout after leaving the ground without a jump.
    if s.since_ground < w.ground_lockout {
        return 0;
    }
    // Walls and overhangs, not floors.
    if n.z > 0.7 || n.z < -0.35 {
        return 0;
    }
    // (Crouching only blocks a Pilot who can't stand up; airborne Pilots aren't crouched here.)
    let flat = Vec3::new(n.x, n.y, 0.0).normalize_or_zero();
    // The wall last left: only below where that run started.
    if s.last_wall.is_some_and(|last| flat.dot(last) > w.same_wall_dot) && (!w.same_wall_allowed || s.pos.z >= s.wallrun_start.z + w.same_wall_height) {
        return 0;
    }
    // Moving into the wall (wallrun_minAngle_air).
    if Vec3::new(dir.x, dir.y, 0.0).normalize_or_zero().dot(n) >= -(w.min_angle_air * 0.5).to_radians().cos() {
        return 0;
    }
    // The wall must be there at the knees and at the middle of the hull.
    let near = |h: f32| wall_probe(world, p, s.pos + Vec3::Z * h, n).is_some_and(|(d, _)| d <= w.allowed_wall_dist);
    if !near(16.0) || !near(p.height * 0.5 + p.radius) {
        return 1;
    }
    // Pushing and looking squarely into a wall whose top is below the head is a mantle
    // (0x180201e50): the head-height probe must find the wall too.
    let wish = wish_of(s.yaw, input);
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let into = -hv(flat);
    let mantle = wish.length_squared() > 1e-6 && wish.normalize().dot(into) >= w.mantle_push_angle.to_radians().cos() && fwd.dot(into) >= w.mantle_view_angle.to_radians().cos();
    if mantle && !near(p.height - p.radius) {
        return 1;
    }
    2
}

/// TryStartWallrun (client.dll 0x18020b390): start a wall-run on the wall with normal `n` if
/// CanWallrun allows; one that failed only on the probes is tried once more after
/// wallrun_retry_interval. Starting adds wallrun_upWallBoost along the wall's up direction
/// (topping up the speed already going that way), unless the no-boost flag is set.
fn try_start_wallrun(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput, n: Vec3, dir: Vec3) {
    s.wallrun_retry = None;
    match can_wallrun(s, p, world, input, n, dir) {
        2 => {
            let w = &p.wallrun;
            let flat = Vec3::new(n.x, n.y, 0.0).normalize_or_zero();
            if s.last_wall.is_some_and(|last| flat.dot(last) > w.same_wall_dot) {
                s.wallrun_no_boost = true;
            }
            let nh = hv(n).length().max(1e-6);
            let up = Vec3::new(-n.z * n.x / nh, -n.z * n.y / nh, nh);
            let boost = if s.wallrun_no_boost { 0.0 } else { w.up_boost };
            let along = s.vel.dot(up);
            if boost > along {
                s.vel += up * (boost - along.max(0.0));
            }
            s.mode = PilotMove::WallRun { normal: flat, time: 0.0 };
            s.push_away = 0.0;
            s.wallrun_start = s.pos;
        }
        1 => s.wallrun_retry = Some((p.wallrun.retry_interval, s.pos, n)),
        _ => {}
    }
}

/// Falling off a wall (not jumping): wallrun_fallAwaySpeed away from it; the wall is
/// remembered for the same-wall rule and the next run gets no up-wall boost.
fn fall_off_wall(s: &mut PilotState, p: &PilotParams, normal: Vec3) {
    s.vel += normal * p.wallrun.fall_away;
    s.mode = PilotMove::Air;
    s.last_wall = Some(normal);
    s.wallrun_no_boost = true;
}

/// Jumping off a wall: remembered for the same-wall rule; no lockout, the next run is boosted.
fn jump_off_wall(s: &mut PilotState, normal: Vec3) {
    s.mode = PilotMove::Air;
    s.last_wall = Some(normal);
    s.since_ground = 99.0;
    s.wallrun_no_boost = false;
    s.jumps += 1;
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
    s.jump_grace = (s.jump_grace - dt).max(0.0);
    s.since_land += dt;
    s.since_slide += dt;
    s.since_ground += dt;
    if let Some(r) = &mut s.wallrun_retry {
        r.0 -= dt;
    }
    // The view goes into a crouch over 0.4 s (0.2 s when the crouch starts a slide) and back
    // up over 0.2 s (client.dll Duck, 0x1801f5500: m_flDucktime 400 / 200 ms).
    if s.crouched {
        if s.duck <= 0.0 {
            s.duck_time = if s.mode == PilotMove::Slide { 0.2 } else { 0.4 };
        }
        s.duck = (s.duck + dt / s.duck_time).min(1.0);
    } else {
        s.duck = (s.duck - dt / 0.2).max(0.0);
    }
    // Off the ground the sprint view eases out; a sprinting jump lands into the fast start.
    if !s.on_ground() {
        s.sprint_fast |= s.sprinting;
        s.sprint_frac = (s.sprint_frac - dt / p.sprint_end.max(1e-3)).max(0.0);
    }

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
                impact = impact.max(move_once(s, p, world, delta / steps as f32, &still, dt / steps as f32));
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
            s.since_ground = 0.0;
            s.jump_grace = 0.0;
            let speed = s.horizontal_speed();
            // StartSlide (client.dll 0x18020af70) runs when crouch goes down on the ground (Duck,
            // 0x1801f571a) or on landing already crouched (0x1801fa1c6): the horizontal speed
            // must be at least slideRequiredStartSpeed and the movement input within
            // slide_max_angle_dot of the view's forward (no input, a strafe or backpedal is
            // just a crouch). After slide_boost_cooldown since the last slide it adds
            // slideSpeedBoost, not past slideSpeedBoostCap, and allows one slide long jump.
            let crouch_pressed = input.crouch && !s.crouched;
            if s.mode == PilotMove::Ground && crouch_pressed && speed >= p.slide_min_speed && wish.dot(fwd) >= p.slide_max_angle_dot {
                if s.since_slide > p.slide_boost_cooldown {
                    let boosted = (speed + p.slide_boost).min(p.slide_boost_cap);
                    s.slide_boost_given = (boosted - speed).max(0.0);
                    if boosted > speed {
                        let v = hv(s.vel) * (boosted / speed);
                        s.vel.x = v.x;
                        s.vel.y = v.y;
                    }
                    s.slide_long_jump = true;
                }
                s.mode = PilotMove::Slide;
                s.since_slide = 0.0;
            }
            let sliding = s.mode == PilotMove::Slide;
            // Standing up ends a slide only at or below slideMaxStopSpeed; it always ends under
            // slideStopSpeed.
            let want_stop = !input.crouch;
            if sliding && (speed < p.slide_stop_speed || (want_stop && speed <= p.slide_max_stop_speed)) {
                s.mode = PilotMove::Ground;
            }
            s.crouched = input.crouch || s.mode == PilotMove::Slide;
            s.sprinting = input.sprint && input.forward > 0.5 && !s.crouched;
            if s.sprinting {
                s.sprint_wait += dt;
                let fast = s.sprint_fast;
                if fast || s.sprint_frac > 0.0 || s.sprint_wait >= p.sprint_start_delay {
                    let dur = if fast { p.sprint_start_fast } else { p.sprint_start };
                    s.sprint_frac = (s.sprint_frac + dt / dur.max(1e-3)).min(1.0);
                }
                if s.sprint_frac >= 1.0 {
                    s.sprint_fast = false;
                }
            } else {
                s.sprint_wait = 0.0;
                s.sprint_fast = false;
                s.sprint_frac = (s.sprint_frac - dt / p.sprint_end.max(1e-3)).max(0.0);
            }
            let mut h = hv(s.vel);
            if input.jump {
                // Jumping skips this tick's friction (Source's CheckJumpButton runs first), so
                // jumping on the tick you land keeps your speed: bunny hops and slide hops.
            } else if s.mode == PilotMove::Slide {
                // WalkMove while sliding (client.dll 0x18020d9f9): the part of the input along
                // the slide is dropped, so input only steers across it or brakes against it.
                // Friction (0x1801f8090) takes slideDecel per second (slideWantToStopDecel when
                // crouch is let go or the input pushes against the slide, GetDeceleration
                // 0x1801fcf10) and then keeps slideVelocityDecay of the speed per second;
                // Accelerate (0x1801f0ac0) adds up to slideAccel per second along what is left of
                // the input, without raising the speed.
                let mut steer = wish;
                let along = steer.dot(h);
                if along > 0.0 {
                    steer -= h * (along / h.length_squared());
                }
                let decel = if want_stop || wish.dot(h) < 0.0 { p.slide_want_stop_decel } else { p.slide_decel };
                let sp = (h.length() - decel * dt).max(0.0) * p.slide_velocity_decay.powf(dt);
                h = h.normalize_or_zero() * sp;
                // The wish speed is the input times the stance speed (crouch_speed: an
                // assumption; it only matters for inputs shorter than slideAccel * dt / 80).
                let wish_speed = steer.length().min(1.0) * p.crouch_speed;
                if wish_speed > 0.0 {
                    let dir = steer.normalize();
                    let add = wish_speed - h.dot(dir);
                    if add > 0.0 {
                        let max_sq = h.length_squared().max(wish_speed * wish_speed);
                        h += dir * (p.slide_accel * dt).min(add);
                        if h.length_squared() > max_sq {
                            h *= (max_sq / h.length_squared()).sqrt();
                        }
                    }
                }
            } else {
                let max = if s.crouched {
                    p.crouch_speed
                } else if s.sprinting {
                    p.sprint_speed
                } else {
                    p.walk_speed
                };
                // Source-style friction then acceleration toward the wished velocity.
                let sp = h.length();
                if sp > 0.0 {
                    let drop = sp.max(p.stop_speed) * p.friction * dt;
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
                // A jump out of a slide is slideJumpHeight high and keeps the slide's speed
                // (jump handler 0x180200609). Once per boosted slide it is a long jump: jumping
                // before the crouch has finished (within 0.4 s) first takes the slide boost back,
                // and a speed between 85% and 100% of slideMaxJumpSpeed is raised to it.
                let mut sp = h.length();
                if sliding && std::mem::take(&mut s.slide_long_jump) && sp > 0.0 {
                    let before = sp;
                    if s.duck < 1.0 && s.since_slide < 0.4 {
                        sp = (sp - s.slide_boost_given).max(0.0);
                    }
                    let max = p.slide_max_jump_speed;
                    if before > max * 0.85 && before < max {
                        sp = max;
                    }
                }
                // A skip (jumping within skip_time of landing) is lower and keeps the landing
                // speed less skip_speed_reduce, not cutting below skip_speed_retain.
                let skip = s.since_land <= p.skip_time;
                let height = if sliding { p.slide_jump_height } else { p.jump_height };
                s.vel.z = jump_v(height * if skip { p.skip_jump_frac } else { 1.0 });
                if skip {
                    sp = sp.max(s.land_speed);
                    if sp > p.skip_speed_retain {
                        sp = (sp - p.skip_speed_reduce).max(p.skip_speed_retain);
                    }
                }
                let d = h.normalize_or_zero();
                s.vel.x = d.x * sp;
                s.vel.y = d.y * sp;
                s.mode = PilotMove::Air;
                s.crouched = false;
                s.since_ground = 99.0;
                s.wallrun_no_boost = false;
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
            if input.jump && s.jump_grace > 0.0 {
                // Just walked off a ledge: still a normal jump (jump_graceperiod).
                s.jump_grace = 0.0;
                s.vel.z = jump_v(p.jump_height);
                s.since_ground = 99.0;
                s.wallrun_no_boost = false;
                s.jumps += 1;
            } else if input.jump && s.double_jump_available {
                s.double_jump_available = false;
                // Already rising fast, it still adds superjump_min_height_fraction of its height.
                let keep = (s.vel.z.max(0.0).powi(2) + 2.0 * p.gravity * p.double_jump_height * p.double_jump_min_frac).sqrt();
                s.vel.z = jump_v(p.double_jump_height).max(keep);
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
            s.since_ground = 0.0;
            s.vel = Vec3::ZERO;
            if input.jump {
                let v = normal * p.wallrun_jump_out + Vec3::new(wish.x, wish.y, 0.0) * p.wallrun_jump_input;
                s.vel = Vec3::new(v.x, v.y, p.wallrun_jump_up);
                jump_off_wall(s, normal);
            } else if !input.ads || input.crouch || t >= p.wallrun_hang_time {
                fall_off_wall(s, p, normal);
            } else {
                s.mode = PilotMove::WallHang { normal, time: t };
                return;
            }
        }
        PilotMove::WallRun { normal, time } => {
            let w = &p.wallrun;
            let t = time + dt;
            s.double_jump_available = true;
            s.since_ground = 0.0;
            // Ground movement turned onto the wall's plane: friction (wallrunFriction) on the
            // velocity along the wall and up it, acceleration along the wall from forward
            // input (wallrunAccelerateHorizontal up to wallrunMaxSpeedHorizontal) and up it
            // from wallrun_upwardAutoPush (wallrunAccelerateVertical up to
            // wallrunMaxSpeedVertical), against gravity that ramps in over
            // wallrun_gravityRampUpTime.
            let n2 = hv(normal);
            let along = Vec2::new(-n2.y, n2.x);
            let along = if along.dot(fwd) >= 0.0 { along } else { -along };
            let mut a = hv(s.vel).dot(along);
            let mut vz = s.vel.z;
            // Slipping: from wallrun_slipstarttime the run gives way to gravity over
            // wallrun_slipduration, starting with a loss of speed and a push down; with no
            // input at all at least wallrun_noInputSlipFrac of it applies.
            if time < w.slip_start && t >= w.slip_start {
                a *= 1.0 - w.slip_slowdown;
                vz = vz * (1.0 - w.slip_slowdown) - w.slip_vel;
            }
            let mut slip = ((t - w.slip_start) / w.slip_duration.max(1e-3)).clamp(0.0, 1.0);
            if wish == Vec2::ZERO {
                slip = slip.max(w.no_input_slip);
            }
            let sp = (a * a + vz * vz).sqrt();
            if sp > 0.0 {
                let k = ((sp - sp.max(p.stop_speed) * w.friction * dt) / sp).max(0.0);
                a *= k;
                vz *= k;
            }
            if input.forward > 0.0 && a < p.wallrun_max_h {
                a += (p.wallrun_accel_h * dt).min(p.wallrun_max_h - a);
            }
            let up = input.forward.max(0.0) * w.upward_push * p.wallrun_max_v;
            if vz < up {
                vz += (p.wallrun_accel_v * (1.0 - slip) * dt).min(up - vz);
            }
            let ramp = (t / w.gravity_ramp.max(1e-3)).min(1.0);
            vz -= p.gravity * (ramp + (1.0 - ramp) * slip) * dt;
            vz = vz.clamp(-p.wallrun_max_v, p.wallrun_max_v);
            let h = along * a;
            s.vel.x = h.x - n2.x * 20.0; // slight pull to keep contact
            s.vel.y = h.y - n2.y * 20.0;
            s.vel.z = vz;
            s.mode = PilotMove::WallRun { normal, time: t };
            // Pushing away from the wall for wallrun_pushAwayFallOffTime lets go of it.
            s.push_away = if wish.dot(n2) > PUSH_AWAY_DOT { s.push_away + dt } else { 0.0 };
            let wall_jump = input.jump;
            if p.wallhang_on_ads && input.ads && !wall_jump {
                s.mode = PilotMove::WallHang { normal, time: 0.0 };
                s.vel = Vec3::ZERO;
                return;
            }
            if wall_jump {
                let v = normal * p.wallrun_jump_out + Vec3::new(wish.x, wish.y, 0.0) * p.wallrun_jump_input;
                s.vel = Vec3::new(h.x + v.x, h.y + v.y, p.wallrun_jump_up);
                jump_off_wall(s, normal);
            } else if t >= p.wallrun_time || input.crouch || s.push_away >= w.push_away_time {
                fall_off_wall(s, p, normal);
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
        move_once(s, p, world, delta / steps as f32, input, dt / steps as f32);
    }
}

/// Moves by `delta` with collision; returns the fastest speed lost into a wall (0 if none).
fn move_once(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, delta: Vec3, input: &PilotInput, step_dt: f32) -> f32 {
    let mut wall_impact = 0.0f32;
    let vel_before = s.vel;
    s.pos += delta;
    let r = p.radius;
    let height = if s.crouched { p.crouch_height } else { p.height };
    let lo = p.step_height + r;
    let hi = (height - r).max(lo);
    let mut wall: Option<Vec3> = None;
    // The surface the hull ran into this move (full normal), for the wall-run start.
    let mut hit: Option<Vec3> = None;
    for z in [lo, hi] {
        let (push, normals) = world.push_sphere(s.pos + Vec3::Z * z, r, 0.7);
        if push != Vec3::ZERO {
            hit = hit.or(Some(normals.normalize_or_zero()).filter(|n| *n != Vec3::ZERO));
            let mut push = push;
            // A ceiling stops the rise. (Not float noise: at map coordinates a wall's push
            // carries a z of about 1e-5, which zeroed the wall-run's climb.)
            if push.z < -1e-3 && s.vel.z > 0.0 {
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
    let mut ground: Option<(f32, f32)> = None;
    for off in [Vec3::ZERO, Vec3::X * r * 0.6, -Vec3::X * r * 0.6, Vec3::Y * r * 0.6, -Vec3::Y * r * 0.6] {
        if let Some(hit) = world.raycast(s.pos + off + Vec3::Z * p.step_height, -Vec3::Z, p.step_height + reach_down) {
            if hit.normal.z > 0.7 && ground.is_none_or(|g| hit.point.z > g.0) {
                ground = Some((hit.point.z, hit.normal.z));
            }
        }
    }
    match ground {
        Some((z, flat)) if s.vel.z <= 0.0 => {
            // Sliding up a step costs slide_step_velocity_reduction per unit of its height. A
            // step is a rise onto flat ground; ramps don't count.
            let rise = z - s.pos.z;
            if s.mode == PilotMove::Slide && on_ground_before && rise > 2.0 && flat > 0.99 {
                let h = hv(s.vel);
                let sp = (h.length() - p.slide_step_loss * rise).max(0.0);
                let h = h.normalize_or_zero() * sp;
                s.vel.x = h.x;
                s.vel.y = h.y;
            }
            s.pos.z = z;
            if !on_ground_before {
                s.landed = Some(-s.vel.z);
                s.mode = PilotMove::Ground;
                s.since_land = 0.0;
                s.land_speed = s.horizontal_speed();
                s.wallrun_no_boost = true;
            }
            s.vel.z = 0.0;
        }
        _ => {
            if on_ground_before {
                // Walked off a ledge: jump_graceperiod to still jump.
                s.mode = PilotMove::Air;
                s.jump_grace = p.jump_grace;
            }
        }
    }

    // Wall-running starts when the airborne move runs into a wall (client.dll AirMove's hull
    // trace, 0x1801f2628: there is no proximity sense): TryStartWallrun with the velocity from
    // before the collision. A start that only failed on the probes is retried once,
    // wallrun_retry_interval later, if the Pilot is still at the wall's plane (0x1801f22b6).
    // Here the hull's sphere contacts stand in for the game's box trace.
    if s.mode == PilotMove::Air {
        if let Some(n) = hit {
            try_start_wallrun(s, p, world, input, n, vel_before);
        } else if let Some((_, at, n)) = s.wallrun_retry.filter(|r| r.0 <= 0.0) {
            s.wallrun_retry = None;
            if (s.pos - at).dot(n).abs() < 1.0 {
                try_start_wallrun(s, p, world, input, n, -n);
            }
        }
    } else if let PilotMove::WallRun { normal, time } = s.mode {
        // A running wall is followed by the knee-height probe (wallrun_allowed_wall_dist from
        // the hull's face, 0x18020e9e0); losing it drops the Pilot off.
        let sensed = wall_probe(world, p, s.pos + Vec3::Z * 16.0, normal)
            .filter(|(d, n)| *d <= p.wallrun.allowed_wall_dist && n.z.abs() <= 0.7)
            .map(|(_, n)| Vec3::new(n.x, n.y, 0.0).normalize_or_zero())
            .or(wall);
        match sensed {
            None => fall_off_wall(s, p, normal),
            Some(n) => {
                // Curved walls: the run turns with the wall, at most wallrun_rotateMaxRate,
                // and lets go when the wall turns away by more than wallrun_angleChangeMinCos.
                let (a, b) = (hv(normal).normalize_or_zero(), hv(n).normalize_or_zero());
                let travel = hv(s.vel).normalize_or_zero();
                let turning_away = b.dot(travel) > a.dot(travel);
                if turning_away && a.dot(b) < p.wallrun.angle_change_min_cos {
                    fall_off_wall(s, p, normal);
                } else if a != Vec2::ZERO && b != Vec2::ZERO {
                    let max = p.wallrun.rotate_max_rate * step_dt;
                    let turn = a.perp_dot(b).atan2(a.dot(b)).clamp(-max, max);
                    let r = Vec2::from_angle(turn).rotate(a) * hv(normal).length();
                    s.mode = PilotMove::WallRun { normal: Vec3::new(r.x, r.y, normal.z), time };
                }
            }
        }
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
