//! Pilot movement: walk, sprint, crouch, slide, jump, double jump, mantle, wall-running and
//! wall hang, ziplines and the grapple.
//! Values from `scripts/players/mp/pilot_mp.set` (the base of every multiplayer Pilot class);
//! fields it leaves out use the engine's player-settings defaults from client.dll (acceleration
//! 2500, deceleration -1, airSpeed 60, airAcceleration 500) and its convars.

use crate::collision::{CollisionWorld, Trace, CLIP_PILOT, DIST_EPSILON};
use crate::engine_move;
use crate::smoothstep;
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
    /// Stance `acceleration` (Accelerate, client.dll 0x1801f0ac0: up to this many u/s per
    /// second along the input).
    pub ground_accel: f32,
    /// Stance `deceleration`: how fast Friction (0x1801f8090) takes away the velocity that isn't
    /// along the input, linearly. The settings' -1 means acceleration * 0.6 (GetDeceleration
    /// 0x1801fcf10).
    pub ground_decel: f32,
    pub air_accel: f32,
    /// Speed the player can adjust to in the air (airSpeed): Source-style air control.
    pub air_speed: f32,
    pub jump_height: f32,
    /// sv_gravity: jump speeds are sqrt(2 * sv_gravity * height), without the gravityscale that
    /// `gravity` carries (jump handler 0x180200cf9), so jumps go height / gravityscale high.
    pub jump_gravity: f32,
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
    /// sv_stopspeed: friction works on at least this speed (the wall-run's friction).
    pub stop_speed: f32,
    /// jump_keyboardgraceperiodmin / max, jump_keyboardgrace_strength and jump_keyboardgrace_max
    /// (AirMove 0x1801f1d62): for `max` seconds after a jump the movement keys turn the
    /// horizontal velocity toward their direction at the same speed, by `strength` until `min`
    /// and fading out to `max`, changing it by at most `cap` x the sprint speed per tick.
    pub keyboard_grace: (f32, f32),
    pub keyboard_grace_strength: f32,
    pub keyboard_grace_cap: f32,
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
    /// The landing view kick (client.dll 0x1801f10c0): viewkickFallDistMin / Max are fall
    /// heights in feet (12 units) turned into impact speeds, viewkickFallDistMaxScale the kick's
    /// scale at the max.
    pub viewkick_fall_min: f32,
    pub viewkick_fall_max: f32,
    pub viewkick_fall_max_scale: f32,
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
    /// stepTestDist (player-settings default 10, client.dll): "Steep slopes shorter than this
    /// will be ignored when stepping onto an object".
    pub step_test_dist: f32,
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
        let mut prev = self.point(0.0);
        (1..=16)
            .map(|i| {
                let p = self.point(i as f32 / 16.0);
                let d = (p - prev).length();
                prev = p;
                d
            })
            .sum()
    }
    /// The closest point's parameter and distance.
    pub fn closest(&self, p: Vec3) -> (f32, f32) {
        const N: usize = 48;
        let mut best = (0.0, f32::MAX);
        let mut p0 = self.point(0.0);
        for i in 0..N {
            let (t0, t1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32);
            let p1 = self.point(t1);
            let d = p1 - p0;
            let k = ((p - p0).dot(d) / d.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let dist = (p0 + d * k - p).length();
            if dist < best.1 {
                best = (t0 + (t1 - t0) * k, dist);
            }
            p0 = p1;
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
    /// grapple_shootVel: the hook flies out this fast...
    pub shoot_speed: f32,
    /// ...and comes back at grapple_retractVel when it hit nothing, at grapple_forcedRetractVel
    /// when the grapple ends or is cancelled.
    pub retract_speed: f32,
    pub release_retract_speed: f32,
    /// grapple_pullDelay_human: seconds from the hook landing to the pull starting.
    pub pull_delay: f32,
    /// grapple_lift: the pull aims the eye this far above the hook.
    pub lift: f32,
    /// grapple_accel_human: pull toward the hook.
    pub pull_accel: f32,
    /// grapple_speedRampMin/Max_human over grapple_speedRampTime_human: the reel-in speed
    /// limit grows from min to max over that long after the pull starts.
    pub pull_speed: (f32, f32),
    pub pull_ramp_time: f32,
    /// grapple_decel_human: speed across the cable bleeds off this fast (per second).
    pub decel: f32,
    /// grapple_initialSlowFrac_human / grapple_initialSlowFracVert_human: when the pull starts,
    /// the speed across the cable keeps these fractions (horizontal, vertical)...
    pub initial_slow_frac: (f32, f32),
    /// ...and grapple_initialImpulse_human (at least grapple_initialSpeedMin_human) is added
    /// toward the hook, up to the ramp's starting speed.
    pub initial_impulse: f32,
    pub initial_speed_min: f32,
    /// grapple_initialImpulseOffGround_human: lift off the ground when the pull starts there.
    pub off_ground_impulse: f32,
    /// grapple_letGravityHelpCosAngle: full gravity while the hook is this steeply below.
    pub gravity_help_cos: f32,
    /// grapple_gravityPushUnderContribution: how much pushing under the cable adds gravity.
    pub push_under: f32,
    /// grapple_gravityFracMin/Max: gravity while pulled; more when pulling away or under.
    pub gravity_frac: (f32, f32),
    /// grapple_detachLengthMin / Max: always let go this close to the hook / this close at full
    /// speed.
    pub detach_length: (f32, f32),
    /// grapple_detachAwaySpeed: let go when moving away from the hook this fast.
    pub detach_away_speed: f32,
    /// grapple_impactVerticalBoost / MaxSpeed: hop when arriving at the hook.
    pub impact_boost: f32,
    pub impact_max: f32,
    /// grapple_detachVerticalBoost / MaxSpeed: hop when letting go moving away from it.
    pub detach_boost: f32,
    pub detach_max: f32,
    /// grapple_detachSpeedLoss / grapple_detachSpeedLossMin: speed lost when it lets go, not
    /// below the minimum.
    pub speed_loss: f32,
    pub speed_loss_min: f32,
    /// grapple_airSpeedMax / grapple_airAccel: steering while pulled.
    pub air_speed: f32,
    pub air_accel: f32,
    /// grapple_detachLowSpeedThreshold / Time / WallTime / GroundTime: let go after moving
    /// toward the hook slower than the threshold this long (in the air, on a wall, on the
    /// ground).
    pub low_speed: f32,
    pub low_speed_time: f32,
    pub low_speed_wall_time: f32,
    pub low_speed_ground_time: f32,
}

impl Default for GrappleParams {
    fn default() -> Self {
        Self {
            shoot_speed: 3000.0,
            retract_speed: 6000.0,
            release_retract_speed: 3000.0,
            pull_delay: 0.2,
            lift: 25.0,
            pull_accel: 1000.0,
            pull_speed: (50.0, 800.0),
            pull_ramp_time: 1.5,
            decel: 425.0,
            initial_slow_frac: (1.0, 0.4),
            initial_impulse: 350.0,
            initial_speed_min: 0.0,
            off_ground_impulse: 50.0,
            gravity_help_cos: 0.8,
            push_under: 2.0,
            gravity_frac: (0.25, 0.7),
            detach_length: (33.0, 50.0),
            detach_away_speed: 500.0,
            impact_boost: 300.0,
            impact_max: 300.0,
            detach_boost: 200.0,
            detach_max: 200.0,
            speed_loss: 300.0,
            speed_loss_min: 430.0,
            air_speed: 420.0,
            air_accel: 650.0,
            low_speed: 250.0,
            low_speed_time: 1.5,
            low_speed_wall_time: 1.2,
            low_speed_ground_time: 0.7,
        }
    }
}

/// The grapple hook while it flies out or comes back (not while it's stuck in a surface).
#[derive(Debug, Clone, Copy)]
pub struct Hook {
    pub pos: Vec3,
    /// Flying out: velocity. Coming back: unused.
    pub vel: Vec3,
    pub travelled: f32,
    /// Reach (grapple_maxLength of the weapon).
    pub max: f32,
    /// Coming back at this speed.
    pub back: Option<f32>,
}

/// What the grapple did this tick, for sounds, effects and power.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrappleEvent {
    Hooked { point: Vec3, normal: Vec3 },
    /// Flew its full reach without hitting anything.
    Missed,
    /// The pull started (the engine takes the power now).
    Pulling,
    /// Let go (cancelled or detached).
    Released,
}

impl Default for PilotParams {
    fn default() -> Self {
        Self {
            walk_speed: 173.5,   // pilot_mp.set stand.speed
            sprint_speed: 260.0, // pilot_mp.set stand.sprintspeed
            sprint_start_delay: 0.2,
            sprint_start: 0.8,
            sprint_start_fast: 0.2,
            sprint_end: 0.15,
            sprint_view_offset: -6.0,
            pitch_offset_scale: 9.2,
            crouch_speed: 80.0,
            ground_accel: 2500.0, // stance `acceleration` default (client.dll)
            ground_decel: 1500.0, // stance `deceleration` default -1: acceleration * 0.6
            air_accel: 500.0,     // airAcceleration default
            air_speed: 60.0,      // airSpeed default
            jump_height: 60.0,
            jump_gravity: 750.0, // sv_gravity
            double_jump_height: 60.0, // superjumpMaxHeight
            double_jump_horz: 180.0,  // superjumpHorzSpeed
            gravity: 750.0 * 0.8,     // sv_gravity * gravityscale (pilot_mp.set 0.8)
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
            keyboard_grace: (0.2, 0.5), // jump_keyboardgraceperiodmin / max
            keyboard_grace_strength: 0.7,
            keyboard_grace_cap: 0.7,
            wallrun_time: 1.75,
            wallrun_max_h: 420.0, // pilot_mp.set wallrunMaxSpeedHorizontal
            wallrun_max_v: 225.0,
            wallrun_accel_h: 1500.0,
            wallrun_accel_v: 360.0,   // wallrunAccelerateVertical
            wallrun_hang_time: 4.0,   // wallrun_hangTimeLimit (pilot_base.set)
            wallhang_on_ads: false,   // pilot_mp.set wallrunAdsType "ADS"
            impact_speed: 380.0,      // impactSpeed
            viewkick_fall_min: 10.0,  // viewkickFallDistMin (pilot_mp.set)
            viewkick_fall_max: 70.0,  // viewkickFallDistMax
            viewkick_fall_max_scale: 12.0, // viewkickFallDistMaxScale
            wallrun_jump_out: 205.0,
            wallrun_jump_up: 230.0,
            wallrun_jump_input: 80.0,
            wallrun: WallrunParams::default(),
            radius: 16.0,
            height: 72.0,
            crouch_height: 47.0,
            eye_height: 60.0,
            crouch_eye_height: 32.0, // pilot_mp.set crouch.viewheight
            step_height: 18.0,
            step_test_dist: 10.0,
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
    /// Toggle-crouch mode (the game's +toggle_duck bind): Some(key held). The engine port
    /// then runs the game's own toggle (set by the press, cleared by a jump, a wall run and
    /// the slide rules) and ignores `crouch`; the old sim still reads `crouch`, which the viewer
    /// emulates. None is hold-crouch.
    pub crouch_toggle: Option<bool>,
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
    /// The engine port's toggle-crouch state (duck_toggle), for the viewer to mirror.
    pub crouch_toggled: bool,
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
    /// Seconds since the last landing (for skips).
    since_land: f32,
    /// Seconds since the last jump (ground, double or wall jump): the keyboard grace.
    since_jump: f32,
    pub jumps: u32,
    /// Where the grapple hook is stuck, from landing until it lets go.
    pub grapple: Option<Vec3>,
    /// The hook while it flies out or comes back.
    pub hook: Option<Hook>,
    /// What the grapple did since the viewer last took these.
    pub grapple_events: Vec<GrappleEvent>,
    /// Seconds spent pulled slower than the low-speed threshold.
    grapple_slow: f32,
    /// Seconds since the hook landed, and since the pull started (None: not pulling yet).
    grapple_time: f32,
    grapple_pull: Option<f32>,
    /// The reach of the hook that's out (grapple_maxLength).
    grapple_reach: f32,
    /// What the pull is dragging against: 0 nothing, 1 a wall, 2 the ground.
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
    /// The engine's duck state, which (unlike the view and hull here) carries on in the air:
    /// seconds crouch has been held, and how long it takes to be fully ducked (Duck
    /// 0x1801f5500: m_flDucktime 400 ms, 200 ms when the crouch started a slide).
    duck_held: f32,
    duck_held_time: f32,
    /// Seconds left of the unduck after crouch was let go (m_duckState 3): no jumping.
    unduck: f32,
    /// Last tick's movement key directions (forward/back, right/left as -1, 0, 1): key presses
    /// for the keyboard grace.
    prev_keys: (i8, i8),
    /// Downward speed of the latest landing (taken by the camera/sound code).
    pub landed: Option<f32>,
    /// The zipline being ridden, and seconds until another can be used.
    pub zipline: Option<ZipRide>,
    zip_cooldown: f32,
    /// The normal of the floor stood on (CategorizePosition's trace).
    ground_normal: Vec3,
    /// What the last ground or air move ran into and didn't get past (its trace's plane
    /// normal, zero if nothing): Friction keeps the velocity along it when the input pushes
    /// into it (`walk_move`).
    block_normal: Vec3,
    /// The engine port's move state (`engine_move`), used in place of the move code here
    /// unless TF_ENGINE_MOVE=0 (`engine_enabled`), and its game clock.
    engine: Option<Box<engine_move::Pm>>,
    engine_time: f32,
    /// Whether the last step ran the engine port (a mantle, zipline or grapple step in between
    /// means its ground state is stale).
    engine_ran: bool,
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
            crouch_toggled: false,
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
            since_jump: 99.0,
            jumps: 0,
            grapple: None,
            hook: None,
            grapple_events: Vec::new(),
            grapple_slow: 0.0,
            grapple_time: 0.0,
            grapple_pull: None,
            grapple_reach: f32::MAX,
            grapple_touch: 0,
            since_slide: 99.0,
            slide_boost_given: 0.0,
            slide_long_jump: false,
            duck: 0.0,
            duck_time: 0.4,
            duck_held: 0.0,
            duck_held_time: 0.4,
            unduck: 0.0,
            prev_keys: (0, 0),
            landed: None,
            zipline: None,
            zip_cooldown: 0.0,
            ground_normal: Vec3::Z,
            block_normal: Vec3::ZERO,
            engine: None,
            engine_time: 0.0,
            engine_ran: false,
        }
    }
    /// The latest landing, if it was a hard one (faster than impactSpeed): its speed.
    pub fn take_hard_landing(&mut self, p: &PilotParams) -> Option<f32> {
        self.landed.take().filter(|v| *v >= p.impact_speed)
    }
    /// Fire the hook along `dir` (from the eye) with reach `max_len`, or let go if it's already
    /// out (pressing the tactical again cancels, as the engine's Grapple() does). Returns true
    /// when a hook was fired.
    pub fn fire_grapple(&mut self, dir: Vec3, max_len: f32, p: &PilotParams) -> bool {
        if self.grapple.is_some() || self.hook.is_some_and(|h| h.back.is_none()) {
            self.release_grapple(p);
            return false;
        }
        let dir = dir.normalize_or_zero();
        self.grapple_reach = max_len;
        self.hook = Some(Hook { pos: self.eye(p), vel: dir * p.grapple.shoot_speed, travelled: 0.0, max: max_len, back: None });
        true
    }
    /// The hook's head: stuck in a surface or flying.
    pub fn hook_pos(&self) -> Option<Vec3> {
        self.grapple.or(self.hook.map(|h| h.pos))
    }
    /// Whether the pull has started (after grapple_pullDelay).
    pub fn grapple_pulling(&self) -> bool {
        self.grapple.is_some() && self.grapple_pull.is_some()
    }
    /// The hook lands on `anchor`; the pull starts grapple_pullDelay later.
    pub fn attach_grapple(&mut self, anchor: Vec3) {
        self.grapple = Some(anchor);
        self.hook = None;
        self.grapple_slow = 0.0;
        self.grapple_time = 0.0;
        self.grapple_pull = None;
    }
    /// Let go (cancelled): the hook comes back at grapple_forcedRetractVel, and if the pull had
    /// started the speed drops by grapple_detachSpeedLoss (client.dll 0x1802c8710/0x1802c8870).
    pub fn release_grapple(&mut self, p: &PilotParams) {
        let g = &p.grapple;
        if let Some(anchor) = self.grapple.take() {
            if self.grapple_pull.take().is_some() {
                let sp = self.vel.length();
                if sp > g.speed_loss_min {
                    self.vel *= (sp - g.speed_loss).max(g.speed_loss_min) / sp;
                }
            }
            self.hook = Some(Hook { pos: anchor, vel: Vec3::ZERO, travelled: 0.0, max: 0.0, back: Some(g.release_retract_speed) });
            self.grapple_events.push(GrappleEvent::Released);
        } else if let Some(h) = self.hook.as_mut().filter(|h| h.back.is_none()) {
            h.back = Some(g.release_retract_speed);
            self.grapple_events.push(GrappleEvent::Released);
        }
    }
    /// Grab a zipline: with the use key, one within zipline_use_range of the eye; in the air,
    /// one touching the top of the hull. Rides the way the Pilot faces along it.
    pub fn try_zipline(&mut self, lines: &[Zipline], use_pressed: bool, p: &PilotParams) -> bool {
        let z = &p.zipline;
        if self.zipline.is_some() || self.zip_cooldown > 0.0 || matches!(self.mode, PilotMove::Mantle { .. }) {
            return false;
        }
        // On the ground only the use key grabs.
        if !use_pressed && self.on_ground() {
            return false;
        }
        let eye = self.eye(p);
        let top = self.pos + Vec3::Z * p.height;
        let mut best: Option<(Zipline, f32, f32)> = None;
        for line in lines {
            let (t, d) = if use_pressed { line.closest(eye) } else { line.closest(top) };
            let reach = if use_pressed { z.use_range } else { z.grab_radius };
            if d <= reach && best.is_none_or(|b| d < b.2) {
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
        smoothstep(self.duck.clamp(0.0, 1.0))
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
/// stands in for it here. The trace takes the player's own solid mask (0x18020e3cd) at the
/// default detail level, so it sees what movement sees: player clips and prop hulls.
fn wall_probe(world: &CollisionWorld, p: &PilotParams, origin: Vec3, n: Vec3) -> Option<(f32, Vec3)> {
    let n = Vec3::new(n.x, n.y, 0.0).normalize_or_zero();
    if n == Vec3::ZERO {
        return None;
    }
    let side = Vec3::new(-n.y, n.x, 0.0) * p.radius * 0.9;
    let up = Vec3::Z * p.radius * 0.9;
    let reach = p.radius * 1.2929 + p.wallrun.allowed_wall_dist;
    // One search of the trees for the nine rays (their boxes are padded by a unit or so).
    let corner = side.abs() + up.abs() + Vec3::splat(4.0);
    let (a, b) = (origin, origin - n * reach);
    let region = world.region_moving(a.min(b) - corner, a.max(b) + corner, CLIP_PILOT);
    let mut best: Option<(f32, Vec3)> = None;
    for i in -1..=1 {
        for j in -1..=1 {
            if let Some(h) = region.raycast(origin + side * i as f32 + up * j as f32, -n, reach) {
                if best.is_none_or(|b| h.t - p.radius < b.0) {
                    best = Some((h.t - p.radius, h.normal));
                }
            }
        }
    }
    best
}

/// What CanWallrun says.
enum CanWallrun {
    No,
    /// Not yet: the probes didn't find the wall close enough (retry).
    NotYet,
    Yes,
}

/// client.dll CanWallrun (0x180202090) for an airborne Pilot whose move hit a surface with
/// normal `n` while moving along `dir`. No speed, input or view requirement.
fn can_wallrun(s: &PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput, n: Vec3, dir: Vec3) -> CanWallrun {
    let w = &p.wallrun;
    // Not within the lockout after leaving the ground without a jump.
    if s.since_ground < w.ground_lockout {
        return CanWallrun::No;
    }
    // Walls and overhangs, not floors.
    if n.z > 0.7 || n.z < -0.35 {
        return CanWallrun::No;
    }
    // Crouching only blocks a Pilot who can't stand up; ducked in the air with crouch held the
    // run would end at once (crouch drops off a wall), so it doesn't start.
    if s.crouched && (input.crouch || !Hull::new(world, p, false).fits(s.pos - Vec3::Z * air_duck_shift(p))) {
        return CanWallrun::No;
    }
    let flat = Vec3::new(n.x, n.y, 0.0).normalize_or_zero();
    // The wall last left: only below where that run started.
    if s.last_wall.is_some_and(|last| flat.dot(last) > w.same_wall_dot) && (!w.same_wall_allowed || s.pos.z >= s.wallrun_start.z + w.same_wall_height) {
        return CanWallrun::No;
    }
    // Moving into the wall (wallrun_minAngle_air).
    if Vec3::new(dir.x, dir.y, 0.0).normalize_or_zero().dot(n) >= -(w.min_angle_air * 0.5).to_radians().cos() {
        return CanWallrun::No;
    }
    // The wall must be there at the knees and at the middle of the hull.
    let near = |h: f32| wall_probe(world, p, s.pos + Vec3::Z * h, n).is_some_and(|(d, _)| d <= w.allowed_wall_dist);
    if !near(16.0) || !near(p.height * 0.5 + p.radius) {
        return CanWallrun::NotYet;
    }
    // Pushing and looking squarely into a wall whose top is below the head is a mantle
    // (0x180201e50): the head-height probe must find the wall too.
    let wish = wish_of(s.yaw, input);
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let into = -hv(flat);
    let mantle = wish.length_squared() > 1e-6 && wish.normalize().dot(into) >= w.mantle_push_angle.to_radians().cos() && fwd.dot(into) >= w.mantle_view_angle.to_radians().cos();
    if mantle && !near(p.height - p.radius) {
        return CanWallrun::NotYet;
    }
    CanWallrun::Yes
}

/// TryStartWallrun (client.dll 0x18020b390): start a wall-run on the wall with normal `n` if
/// CanWallrun allows; one that failed only on the probes is tried once more after
/// wallrun_retry_interval. Starting adds wallrun_upWallBoost along the wall's up direction
/// (topping up the speed already going that way), unless the no-boost flag is set.
fn try_start_wallrun(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput, n: Vec3, dir: Vec3) {
    s.wallrun_retry = None;
    match can_wallrun(s, p, world, input, n, dir) {
        CanWallrun::Yes => {
            stand_in_air(s, p, world);
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
        CanWallrun::NotYet => s.wallrun_retry = Some((p.wallrun.retry_interval, s.pos, n)),
        CanWallrun::No => {}
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

/// A jump off the ground or a wall (not the double jump): no wall-run lockout, the next run is
/// boosted, and the keyboard grace starts.
fn took_off(s: &mut PilotState) {
    s.mode = PilotMove::Air;
    s.since_ground = 99.0;
    s.since_jump = 0.0;
    s.wallrun_no_boost = false;
    s.jumps += 1;
}

/// Jumping off a wall: remembered for the same-wall rule.
fn jump_off_wall(s: &mut PilotState, normal: Vec3) {
    took_off(s);
    s.last_wall = Some(normal);
}

/// The wall-run jump's velocity (client.dll 0x180201170), from the horizontal velocity `h` and
/// vertical speed `vz` the Pilot has on the wall. `add` is the input direction times
/// wallrunJumpInputDirSpeed; while sprinting, an input shorter than 1 is filled up with the view
/// direction first (the sprint flag 0x2588). The push-off is wallrunJumpOutwardSpeed, cut by up
/// to 80% when the Pilot both looks and pushes into the wall (the two negative dot products with
/// the normal, 0x1802011f0), and it is the normal component of the whole `add`: `add` is raised
/// along the normal until it has that much, not given it on top. The horizontal velocity is kept
/// and `add` is added to it. Vertically, wallrunJumpUpSpeed is the target: a Pilot rising slower
/// gets `min(vz + 1.5 * up, up)` (falling fast off the wall weakens the jump), one rising faster
/// keeps its speed (0x180201350).
fn wall_jump_velocity(h: Vec2, vz: f32, normal: Vec3, wish: Vec2, fwd: Vec2, pitch: f32, sprinting: bool, p: &PilotParams) -> Vec3 {
    let mut dir = wish;
    if sprinting && dir.length() < 1.0 {
        dir += fwd * (1.0 - dir.length());
    }
    let dir = Vec3::new(dir.x, dir.y, 0.0);
    let mut add = dir * p.wallrun_jump_input;
    // The view direction's dot with the wall normal (the normal is horizontal, so only the pitch's
    // cosine matters), clamped to "into the wall".
    let look_in = (hv(normal).dot(fwd) * pitch.cos()).min(0.0);
    let push_in = normal.dot(dir).min(0.0);
    let outward = p.wallrun_jump_out * (1.0 - 0.8 * look_in * push_in);
    let along = normal.dot(add);
    if along < outward {
        add += normal * (outward - along);
    }
    let up = p.wallrun_jump_up;
    let vz = vz + add.z;
    let vz = if vz < up { (vz + 1.5 * up).min(up) } else { vz };
    Vec3::new(h.x + add.x, h.y + add.y, vz)
}

/// The double jump (client.dll jump handler 0x180200200, superjump branch). Vertical: rising
/// slower than a jump of superjump_min_height_fraction of superjumpMaxHeight would still add,
/// the vertical speed is replaced by a full superjump (sqrt(2 * sv_gravity * height)); rising
/// faster, that fraction's speed is added to it (0x1802007ba). Horizontal: the input times
/// superjumpHorzSpeed is added, and the old velocity loses as much speed as that adds (down to
/// zero), keeping its direction (0x180200f7c).
fn superjump(s: &mut PilotState, wish: Vec2, p: &PilotParams) {
    s.double_jump_available = false;
    let g = p.jump_gravity;
    let mut height = p.double_jump_height;
    let rising = s.vel.z.max(0.0).powi(2) / (2.0 * g);
    if height > height * p.double_jump_min_frac + rising {
        s.vel.z = 0.0;
    } else {
        height *= p.double_jump_min_frac;
    }
    s.vel.z += (2.0 * g * height).sqrt();
    let add = wish * p.double_jump_horz;
    let h = hv(s.vel);
    let keep = if h.length() > add.length() { 1.0 - add.length() / h.length() } else { 0.0 };
    let h = h * keep + add;
    s.vel.x = h.x;
    s.vel.y = h.y;
    s.since_jump = 0.0;
    s.jumps += 1;
}

/// AirMove's keyboard grace (client.dll 0x1801f1d62): within jump_keyboardgraceperiodmax of
/// the last jump, on a tick where a movement key goes down (the move data's pressed buttons,
/// 0x618 = IN_FORWARD|IN_BACK|IN_MOVELEFT|IN_MOVERIGHT), the held movement keys (the buttons,
/// not their analog amount) turn the horizontal velocity toward their direction, keeping its
/// speed: by jump_keyboardgrace_strength up to jump_keyboardgraceperiodmin, fading to nothing
/// at the max, and changing the velocity by at most jump_keyboardgrace_max x the standing
/// sprint speed. It happens once per key press: in the real game's capture, back pressed 0.2 s
/// into a 260 u/s jump takes 182 off the speed on that tick only, then air acceleration slows
/// the rest (the speed passes through zero 0.15 s later), instead of turning the jump around.
/// The game takes the keys along the view's forward and right; flattening them here ignores
/// the view's pitch.
fn keyboard_grace(h: Vec2, fwd: Vec2, right: Vec2, input: &PilotInput, since_jump: f32, p: &PilotParams) -> Vec2 {
    let key = |x: f32| if x > 0.01 { 1.0 } else if x < -0.01 { -1.0 } else { 0.0 };
    let keys = fwd * key(input.forward) + right * key(input.right);
    let speed = h.length();
    if keys == Vec2::ZERO || speed < 1e-3 {
        return h;
    }
    let (min, max) = p.keyboard_grace;
    let frac = ((max - since_jump) / (max - min)).clamp(0.0, 1.0) * p.keyboard_grace_strength;
    let cur = h / speed;
    let dir = cur + (keys.normalize() - cur) * frac;
    let target = if dir.length() > 1e-4 { dir.normalize() * speed } else { dir };
    let delta = (target - h).clamp_length_max(p.sprint_speed * p.keyboard_grace_cap);
    h + delta
}

/// A movement key's direction from its input (-1, 0, 1).
fn key_sign(x: f32) -> i8 {
    if x > 0.01 {
        1
    } else if x < -0.01 {
        -1
    } else {
        0
    }
}

/// StartSlide (client.dll 0x18020af70), run when crouch goes down on the ground (Duck,
/// 0x1801f571a) or on landing while ducking or ducked (OnLand 0x1801fa160, m_duckState 1 or
/// 2): the horizontal speed must be at least slideRequiredStartSpeed, and `on_ground` the
/// movement input within slide_max_angle_dot of the view's forward (no input, a strafe or
/// backpedal is just a crouch). The engine tests the direction only when a ground entity is
/// set, and OnLand calls StartSlide before it sets one, so a crouched landing slides whatever
/// is pressed. After slide_boost_cooldown since the last slide it adds slideSpeedBoost, not
/// past slideSpeedBoostCap, and allows one slide long jump.
fn start_slide(s: &mut PilotState, p: &PilotParams, wish: Vec2, fwd: Vec2, on_ground: bool, dt: f32) {
    let speed = s.horizontal_speed();
    if speed < p.slide_min_speed || (on_ground && wish.dot(fwd) < p.slide_max_angle_dot) {
        return;
    }
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
    if s.duck_held <= dt {
        s.duck_held_time = 0.2;
    }
}

/// Respawn's AirAccelerate (client.dll 0x1801f19a0) on the full velocity: Source air
/// acceleration toward the input up to `air_speed`; already faster along it, the velocity turns
/// toward the input at player_extraairaccelleration (2 u/s^2), the result capped at the larger
/// of the old speed and the wish speed. The cap is on the 3D speed, so while rising or falling
/// that little push trades vertical speed for horizontal (the capture's jumps gain 0.5-1 u/s
/// across while holding forward, 1.5 u/s^2 at 173 across and 290 up).
fn air_accelerate(v: Vec3, wish: Vec2, air_speed: f32, accel: f32, dt: f32) -> Vec3 {
    let dir = wish.normalize_or_zero().extend(0.0);
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

/// AirMove's steering: air acceleration, then the keyboard grace on a key press just after a
/// jump.
#[allow(clippy::too_many_arguments)]
fn air_control(s: &mut PilotState, wish: Vec2, fwd: Vec2, right: Vec2, input: &PilotInput, key_pressed: bool, p: &PilotParams, dt: f32) {
    s.vel = air_accelerate(s.vel, wish, p.air_speed, p.air_accel, dt);
    if key_pressed && s.since_jump < p.keyboard_grace.1 {
        let h = keyboard_grace(hv(s.vel), fwd, right, input, s.since_jump, p);
        s.vel.x = h.x;
        s.vel.y = h.y;
    }
}

/// Half the difference between the standing and crouched hull heights: ducking in the air
/// lifts the feet this much and standing up lowers them, keeping the hull's middle in place
/// (FinishDuck 0x1801f7290 / FinishUnDuck 0x1801f75e0, x 0.5 at 0x1808a2f88).
fn air_duck_shift(p: &PilotParams) -> f32 {
    (p.height - p.crouch_height) * 0.5
}

/// Stand up in the air (the hull's middle kept) if the standing hull fits there.
fn stand_in_air(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld) -> bool {
    if !s.crouched {
        return true;
    }
    let at = s.pos - Vec3::Z * air_duck_shift(p);
    if Hull::new(world, p, false).fits(at) {
        s.pos = at;
        s.crouched = false;
        true
    } else {
        false
    }
}

/// Ducking in the air: once crouch has been held for the duck time (Duck's timer, 0.4 s) the
/// hull turns crouched with the feet lifted (the capture's crouched slide-hop jump rises 12.5
/// units at that moment and lands later); letting go stands back up when the unduck ends, if
/// there is room.
fn air_duck(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput) {
    if input.crouch {
        if !s.crouched && s.duck_held >= s.duck_held_time {
            s.pos.z += air_duck_shift(p);
            s.crouched = true;
        }
    } else if s.unduck <= 0.0 {
        stand_in_air(s, p, world);
    }
}

pub fn step(s: &mut PilotState, input: &PilotInput, p: &PilotParams, world: &CollisionWorld, dt: f32) {
    s.yaw = input.yaw;
    s.pitch = input.pitch;
    // Set again at the end of an engine step: any other way through here (zipline, grapple,
    // mantle, the old move) leaves the port's ground state stale.
    let engine_ran = std::mem::replace(&mut s.engine_ran, false);
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let right = Vec2::new(s.yaw.sin(), -s.yaw.cos());
    let wish = wish_of(s.yaw, input);
    let jump_v = |h: f32| (2.0 * p.jump_gravity * h).sqrt();
    s.zip_cooldown = (s.zip_cooldown - dt).max(0.0);
    s.jump_grace = (s.jump_grace - dt).max(0.0);
    s.since_land += dt;
    s.since_jump += dt;
    s.since_slide += dt;
    s.since_ground += dt;
    if let Some(r) = &mut s.wallrun_retry {
        r.0 -= dt;
    }
    // Movement keys pressed this tick (the move data's pressed buttons, which the keyboard grace
    // reads: AirMove 0x1801f1d62 tests IN_FORWARD|IN_BACK|IN_MOVELEFT|IN_MOVERIGHT there).
    let keys = (key_sign(input.forward), key_sign(input.right));
    let key_pressed = (keys.0 != 0 && keys.0 != s.prev_keys.0) || (keys.1 != 0 && keys.1 != s.prev_keys.1);
    s.prev_keys = keys;
    // The view goes into a crouch over 0.4 s (0.2 s when the crouch starts a slide) and back
    // up over 0.2 s (client.dll Duck, 0x1801f5500: m_flDucktime 400 / 200 ms); in the air it
    // starts going down while crouch is held.
    if s.crouched || (input.crouch && s.mode == PilotMove::Air) {
        if s.duck <= 0.0 {
            s.duck_time = if s.mode == PilotMove::Slide { 0.2 } else { 0.4 };
        }
        s.duck = (s.duck + dt / s.duck_time).min(1.0);
    } else {
        s.duck = (s.duck - dt / 0.2).max(0.0);
    }
    // Letting go of crouch starts the unduck (Duck 0x1801f5847 / 0x1801f5867, m_duckState 3):
    // 200 ms from fully ducked, or 200 ms times the part of the duck done. Until it finishes
    // the Pilot can't jump unless sliding (jump handler 0x1802003f4); the capture of the real
    // game shows a jump pressed together with letting go of crouch out of a slide is lost.
    s.unduck = (s.unduck - dt).max(0.0);
    let released = !input.crouch && s.duck_held > 0.0;
    if !input.crouch {
        if released {
            s.unduck = 0.2 * (s.duck_held / s.duck_held_time).min(1.0);
        }
        s.duck_held = 0.0;
    } else {
        if s.duck_held == 0.0 {
            s.duck_held_time = 0.4;
        }
        s.duck_held += dt;
        s.unduck = 0.0;
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
            let k = smoothstep((r.mount / z.mount_time.max(1e-3)).min(1.0));
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

    // The grapple (client.dll: hook flight 0x1804f5ed0, detach rules 0x1804f7be0, pull
    // 0x1804f43e0, gravity 0x1801fe9e0, steering 0x1801f1bd0).
    update_hook(s, p, world, dt);
    if s.grapple.is_some() && grapple_step(s, input, wish, p, world, dt) {
        return;
    }

    // The engine port runs the whole move (everything but the mantle) when asked for.
    if engine_enabled() && !matches!(s.mode, PilotMove::Mantle { .. }) {
        engine_step(s, input, p, world, dt, engine_ran);
        return;
    }

    // Whether this tick's move is airborne and owes the second half of its gravity.
    let mut half_gravity = false;
    match s.mode {
        PilotMove::Ground | PilotMove::Slide => {
            s.double_jump_available = true;
            s.last_wall = None;
            s.since_ground = 0.0;
            s.jump_grace = 0.0;
            let speed = s.horizontal_speed();
            // StartSlide runs when crouch goes down on the ground (Duck, 0x1801f571a); landing
            // while ducking starts one too (`move_once`).
            let crouch_pressed = input.crouch && !s.crouched;
            if s.mode == PilotMove::Ground && crouch_pressed {
                start_slide(s, p, wish, fwd, true, dt);
            }
            let sliding = s.mode == PilotMove::Slide;
            // Standing up ends a slide only at or below slideMaxStopSpeed; it always ends under
            // slideStopSpeed.
            let want_stop = !input.crouch;
            if sliding && (speed < p.slide_stop_speed || (want_stop && speed <= p.slide_max_stop_speed)) {
                s.mode = PilotMove::Ground;
                // Crouch let go earlier, above slideMaxStopSpeed: the unduck starts now.
                if want_stop && !released {
                    s.unduck = 0.2;
                }
            }
            // Standing up needs room for the standing hull (under something low the Pilot stays
            // crouched).
            s.crouched = input.crouch || s.mode == PilotMove::Slide || (s.crouched && !Hull::new(world, p, false).fits(s.pos));
            // No jump while unducking (m_duckState 3) unless sliding (0x1802003f4); a Pilot who
            // can't stand up stays ducked and can.
            let jump = input.jump && (s.unduck <= 0.0 || s.mode == PilotMove::Slide || s.crouched);
            // CanSprint (client.dll 0x1802c2570): the key or latch (`input.sprint`), a movement
            // input of at least 0.8 within 45 degrees of forward, not ducking or ducked, and
            // moving along the view (so the first tick from rest walks). Off the ground it
            // stops after 0.25 s, which does not matter here.
            let len = input.forward.hypot(input.right);
            s.sprinting = input.sprint && len >= 0.8 && input.forward / len >= 0.7071 && !s.crouched && hv(s.vel).dot(fwd) > 0.0;
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
            if jump {
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
                // Respawn's WalkMove (client.dll 0x18020cc30): Friction keeps the velocity along
                // the input up to the wish speed and takes the rest away at the deceleration
                // (1500 u/s^2, linear: no sv_friction/sv_stopspeed); Accelerate adds up to the
                // acceleration along the input. The pilot settings set no sprintAcceleration,
                // lowAcceleration or sprintDeceleration, so the same two rates hold at any speed.
                h = crate::titan::walk_move(h, wish, max, p.ground_accel, p.ground_decel, s.block_normal, dt);
            }
            s.vel.x = h.x;
            s.vel.y = h.y;
            if jump {
                // A jump out of a slide is slideJumpHeight high and keeps the slide's speed
                // (jump handler 0x180200609). Once per boosted slide it is a long jump: jumping
                // before the Pilot is fully ducked (m_duckState 2) within 0.4 s of the slide's
                // start first takes the slide boost back, and a speed between 85% and 100% of
                // slideMaxJumpSpeed is raised to it. Crouch held through the air keeps the
                // Pilot ducked, so landing into a boosted slide and hopping at once keeps it.
                let mut sp = h.length();
                if sliding && std::mem::take(&mut s.slide_long_jump) && sp > 0.0 {
                    let before = sp;
                    if s.duck_held < s.duck_held_time && s.since_slide < 0.4 {
                        sp = (sp - s.slide_boost_given).max(0.0);
                    }
                    let max = p.slide_max_jump_speed;
                    if before > max * 0.85 && before < max {
                        sp = max;
                    }
                }
                // A skip (jumping within skip_time of landing) is lower and keeps its speed less
                // skip_speed_reduce, not cutting below skip_speed_retain (0x1802009b6: the
                // speed it has then, not the landing speed).
                let skip = s.since_land <= p.skip_time;
                let height = if sliding { p.slide_jump_height } else { p.jump_height };
                // The jump speed is added to the vertical speed (0x180200da4), which already
                // lost StartGravity's half tick of gravity, and the jump finishes that gravity at
                // once: the move runs at the jump speed less a tick of gravity, and after the
                // move's own FinishGravity the tick ends 1.5 ticks of gravity under it (the real
                // game logs 293.7 for a 300 u/s jump at 141 fps).
                s.vel.z = jump_v(height * if skip { p.skip_jump_frac } else { 1.0 }) - p.gravity * dt;
                half_gravity = true;
                if skip {
                    if sp > p.skip_speed_retain {
                        sp = (sp - p.skip_speed_reduce).max(p.skip_speed_retain);
                    }
                }
                let d = h.normalize_or_zero();
                s.vel.x = d.x * sp;
                s.vel.y = d.y * sp;
                // Ducked (crouch held) the Pilot leaves the ground ducked.
                s.crouched &= input.crouch;
                took_off(s);
                // The jump tick then runs AirMove, not WalkMove.
                air_control(s, wish, fwd, right, input, key_pressed, p, dt);
            }
        }
        PilotMove::Air => {
            // StartGravity: half a tick of gravity before the move and half after it
            // (FinishGravity, below), so the move runs at the mid-tick vertical speed.
            s.vel.z -= 0.5 * p.gravity * dt;
            half_gravity = true;
            air_duck(s, p, world, input);
            // The jump comes before AirMove (CheckJumpButton) and finishes its gravity at once.
            if input.jump && s.jump_grace > 0.0 {
                // Just walked off a ledge: still a normal jump (jump_graceperiod).
                s.jump_grace = 0.0;
                s.vel.z = jump_v(p.jump_height) - 0.5 * p.gravity * dt;
                took_off(s);
            } else if input.jump && s.double_jump_available {
                superjump(s, wish, p);
                s.vel.z -= 0.5 * p.gravity * dt;
            }
            air_control(s, wish, fwd, right, input, key_pressed, p, dt);
        }
        PilotMove::Mantle { from, to, time } => {
            let t = time + dt;
            let k = (t / mantle_time(from.z + p.eye_height - to.z)).min(1.0);
            // Up first, then over the edge.
            let up = (k / 0.6).min(1.0);
            let over = ((k - 0.4) / 0.6).clamp(0.0, 1.0);
            let (over, up) = (smoothstep(over), smoothstep(up));
            s.pos = Vec3::new(from.x + (to.x - from.x) * over, from.y + (to.y - from.y) * over, from.z + (to.z - from.z) * up);
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
                s.vel = wall_jump_velocity(Vec2::ZERO, 0.0, normal, wish, fwd, s.pitch, s.sprinting, p);
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
                s.vel = wall_jump_velocity(h, vz, normal, wish, fwd, s.pitch, s.sprinting, p);
                jump_off_wall(s, normal);
            } else if t >= p.wallrun_time || input.crouch || s.push_away >= w.push_away_time {
                fall_off_wall(s, p, normal);
            }
        }
    }

    // Automantle (client.dll 0x180209ed0): pressing into a ledge within reach climbs onto it,
    // but only while jumping: on the ground, or within 0.25 s (0x1808a2f74) of leaving it,
    // the jump button must be down. Running into a ledge on foot just stops the Pilot.
    let mantle_ok = match s.mode {
        PilotMove::Ground | PilotMove::Slide => input.jump,
        PilotMove::Air => input.jump || s.since_ground >= 0.25,
        _ => true,
    };
    if mantle_ok && input.forward > 0.3 && !matches!(s.mode, PilotMove::Mantle { .. }) && s.vel.z < 250.0 {
        if let Some(to) = mantle_target(s, p, world, fwd) {
            s.mode = PilotMove::Mantle { from: s.pos, to, time: 0.0 };
            s.vel = Vec3::ZERO;
            return;
        }
    }
    move_once(s, p, world, input, dt);
    // FinishGravity, unless the move landed (or caught a wall).
    if half_gravity && s.mode == PilotMove::Air {
        s.vel.z -= 0.5 * p.gravity * dt;
    }
}

/// The Pilot and the Titan run on the engine port (`engine_move`), the default since
/// 2026-10-14; TF_ENGINE_MOVE=0 runs the older move code in `pilot.rs` / `titan.rs` instead
/// (comparisons). Unit tests pick per thread with [`engine_for_test`] (the old sim unless set).
pub fn engine_enabled() -> bool {
    #[cfg(test)]
    {
        ENGINE_FOR_TEST.with(|c| c.get())
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| !std::env::var_os("TF_ENGINE_MOVE").is_some_and(|v| v == "0"))
    }
}

#[cfg(test)]
thread_local! {
    static ENGINE_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests: run this thread's moves on the engine port (true) or the old sims (false).
#[cfg(test)]
pub(crate) fn engine_for_test(on: bool) {
    ENGINE_FOR_TEST.with(|c| c.set(on));
}

/// The engine port's player settings from this file's params: the stim and ADS speed scales
/// the viewer applies to the params carry over, the rest are the port's pilot_mp.set values.
fn engine_settings(p: &PilotParams) -> engine_move::Settings {
    let mut st = engine_move::Settings::default();
    st.pose_speed[0] = p.walk_speed;
    st.pose_sprint_speed[0] = p.sprint_speed;
    st.pose_speed[1] = p.crouch_speed;
    st.wallrun_ads_type = if p.wallhang_on_ads { 1 } else { 0 };
    st.sprint_start_delay = p.sprint_start_delay;
    st.sprint_start_duration = p.sprint_start;
    st.sprint_start_fast_duration = p.sprint_start_fast;
    st.sprint_end_duration = p.sprint_end;
    st
}

/// One step on the engine port: the input becomes a usercmd, the port moves its own player
/// state (kept in `s.engine`, synced to `s.pos` / `s.vel` either way so the viewer's teleports,
/// zipline, grapple and mantle still work), and the result is written back into the fields the
/// viewer reads (mode, crouched, sprinting, jumps, landed, double jump) plus the sprint view
/// ramp from the ground branch of `step`.
fn engine_step(s: &mut PilotState, input: &PilotInput, p: &PilotParams, world: &CollisionWorld, dt: f32, engine_ran: bool) {
    let yaw_deg = s.yaw.to_degrees();
    let mut pm = s.engine.take().unwrap_or_else(|| Box::new(engine_move::Pm::new(s.pos, yaw_deg)));
    // Something else moved the Pilot since the port last ran (a teleport, the rodeo placing it
    // on the Titan): its ground state is as stale as after a step that did not run the port.
    let moved = (s.pos - pm.origin).length_squared() > 1e-4;
    pm.origin = s.pos;
    pm.vel = s.vel;
    if !engine_ran || moved {
        // Back from a mantle, zipline, grapple or rodeo (or the first step): nothing under the
        // feet until the move's CategorizePosition says so.
        pm.on_ground = false;
        pm.normal = Vec3::Z;
        pm.sliding = false;
        pm.hanging = false;
        pm.cached = None;
    }
    s.engine_time += dt;
    let st = engine_settings(p);
    let cv = engine_move::Cvars::default();
    let mut b = 0u32;
    if input.forward > 0.0 {
        b |= engine_move::IN_FORWARD;
    } else if input.forward < 0.0 {
        b |= engine_move::IN_BACK;
    }
    if input.right > 0.0 {
        b |= engine_move::IN_MOVERIGHT;
    } else if input.right < 0.0 {
        b |= engine_move::IN_MOVELEFT;
    }
    if input.sprint {
        b |= engine_move::IN_SPEED;
    }
    match input.crouch_toggle {
        Some(held) => {
            if held {
                b |= engine_move::IN_DUCKTOGGLE;
            }
        }
        None => {
            if input.crouch {
                b |= engine_move::IN_DUCK;
            }
        }
    }
    if input.jump {
        b |= engine_move::IN_JUMP;
    }
    if input.ads {
        b |= engine_move::IN_ZOOM;
    }
    let cmd = engine_move::Cmd { buttons: b, fmove: input.forward, smove: input.right, yaw: yaw_deg, pitch: s.pitch.to_degrees() };
    let was_on_ground = pm.on_ground;
    let was_wall = pm.is_wallrunning();
    let vz_before = pm.vel.z;
    let ran_before = engine_ran;
    engine_move::player_move(&mut pm, &cmd, &st, &cv, world, s.engine_time, dt);
    if std::env::var_os("TF_ENGINE_DEBUG").is_some() {
        eprintln!(
            "eng ran {} moved {} g0 {} t {:.3} b {:#x} f {:.2} s {:.2} yaw {:.1} pitch {:.1} | g {} duck {} dt {} spr {} max {:.1} wish {:.3} {:.3} {:.3} | pos {:.1} {:.1} {:.1} vel {:.1} {:.1} {:.1} n {:.2} {:.2} {:.2}",
            ran_before as u8, moved as u8, was_on_ground as u8, pm.now, cmd.buttons, cmd.fmove, cmd.smove, cmd.yaw, cmd.pitch, pm.on_ground as u8, pm.duck_state, pm.ducktime, pm.sprinting as u8, pm.maxspeed,
            pm.wishdir.x, pm.wishdir.y, pm.wishdir.z, pm.origin.x, pm.origin.y, pm.origin.z, pm.vel.x, pm.vel.y, pm.vel.z, pm.normal.x, pm.normal.y, pm.normal.z
        );
    }

    s.pos = pm.origin;
    s.vel = pm.vel;
    s.crouched = pm.ducked_hull();
    s.duck = pm.duck_view();
    s.sprinting = pm.sprinting;
    s.crouch_toggled = pm.duck_toggle;
    s.ground_normal = if pm.on_ground { pm.ground_normal } else { Vec3::Z };
    s.block_normal = pm.block_normal;
    s.double_jump_available = (pm.jumps_used as f32) < st.superjump_limit;
    if pm.jump_time == pm.now {
        s.jumps += 1;
        s.since_jump = 0.0;
        s.since_ground = 99.0;
    }
    if !was_on_ground && !was_wall && pm.on_ground {
        s.landed = Some(-vz_before);
        s.since_land = 0.0;
    }
    s.mode = if pm.hanging {
        PilotMove::WallHang { normal: pm.normal, time: pm.now - pm.hang_start }
    } else if pm.is_wallrunning() {
        PilotMove::WallRun { normal: pm.normal, time: pm.now - pm.wallrun_start_time }
    } else if pm.on_ground {
        if pm.sliding { PilotMove::Slide } else { PilotMove::Ground }
    } else {
        PilotMove::Air
    };
    if s.on_ground() || !matches!(s.mode, PilotMove::Air) {
        s.since_ground = 0.0;
    }
    // The sprint fraction (view offset, the viewmodel's sprintfrac pose parameter) is the
    // engine's GetSprintFrac.
    s.sprint_frac = pm.sprint_frac(&st);
    s.engine = Some(pm);
    s.engine_ran = true;

    // The automantle stays this file's (the engine's is not ported yet): the same trigger as
    // the old path, after the move.
    let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
    let mantle_ok = match s.mode {
        PilotMove::Ground | PilotMove::Slide => input.jump,
        PilotMove::Air => input.jump || s.since_ground >= 0.25,
        _ => true,
    };
    if mantle_ok && input.forward > 0.3 && s.vel.z < 250.0 {
        if let Some(to) = mantle_target(s, p, world, fwd) {
            s.mode = PilotMove::Mantle { from: s.pos, to, time: 0.0 };
            s.vel = Vec3::ZERO;
        }
    }
}

/// A surface is a floor when its normal's up component is at least this (client.dll
/// 0x1808a7ce0, used by CategorizePosition, StayOnGround and StepMove).
const WALKABLE_Z: f32 = 0.7;
/// CategorizePosition (client.dll 0x1801f3160) leaves the ground while rising faster than this
/// (0x1808f8584, Source's NON_JUMP_VELOCITY).
const NON_JUMP_VELOCITY: f32 = 140.0;

/// The Pilot's collision hull: an axis-aligned box, pilot_base.set's stand hull_min/hull_max
/// "-16 -16 0" / "16 16 72" (crouch "16 16 47"), traced against the world and the player clips
/// as the engine's player movement does (client.dll 0x1801fbdc0). It carries the settings the
/// moves read (step height, stepTestDist) and the Pilot's up direction (player+0x2494): +Z,
/// or the wall's normal while wall-running, the wall then being the floor that StepMove and
/// the blocked-move handling measure from (IsWallRunning, 0x1802c9350, is just up.z < 1).
struct Hull<'a> {
    world: &'a CollisionWorld,
    p: &'a PilotParams,
    mins: Vec3,
    maxs: Vec3,
    up: Vec3,
}

impl<'a> Hull<'a> {
    fn new(world: &'a CollisionWorld, p: &'a PilotParams, crouched: bool) -> Self {
        let h = if crouched { p.crouch_height } else { p.height };
        Self { world, p, mins: Vec3::new(-p.radius, -p.radius, 0.0), maxs: Vec3::new(p.radius, p.radius, h), up: Vec3::Z }
    }
    fn trace(&self, a: Vec3, b: Vec3) -> Trace {
        self.world.trace_box(a, b, self.mins, self.maxs, CLIP_PILOT)
    }
    fn fits(&self, at: Vec3) -> bool {
        !self.trace(at, at).start_solid
    }
    /// Out of anything the hull overlaps (a stand-up, a mantle's end, a teleport): the nearest
    /// free spot among small nudges, up first (Source's CheckStuck tries offsets too).
    fn unstick(&self, pos: &mut Vec3) {
        if self.fits(*pos) {
            return;
        }
        for r in [0.125, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
            for d in [Vec3::Z, Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::new(1.0, 1.0, 0.0), Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(-1.0, -1.0, 0.0), -Vec3::Z] {
                if self.fits(*pos + d * r) {
                    *pos += d * r;
                    return;
                }
            }
        }
    }
}

/// The last steep plane (a wall, |normal.z| up to `WALKABLE_Z`) a move's traces ran into: what
/// a wall-run touches.
#[derive(Default)]
struct Touched(Option<Vec3>);

impl Touched {
    fn push(&mut self, n: Vec3) {
        if n.z.abs() <= WALKABLE_Z {
            self.0 = Some(n);
        }
    }
}

/// TryPlayerMove (client.dll 0x1801fa310): move along `vel` for `time`, sliding along what is
/// hit. Up to 8 traces. Each blocking plane takes the velocity's part into it away (from the
/// velocity at the last plane reset) and adds 1/128 unit/s of push-out; a second plane makes it
/// run along the crease of the two (nudged out of both by 1/128 of their crease); a third plane
/// within 30 degrees (cos 0.866) of neither stops it, as does turning against the starting
/// velocity. Each new destination is first kept out of the planes already hit (to 0.001 in
/// front of them), and the planes are forgotten once the hull has moved more than a unit since
/// they were last reset. `first` is a trace already made from `pos` for the first try. The
/// planes hit go to `hits`.
fn try_player_move(h: &Hull, pos: &mut Vec3, vel: &mut Vec3, time: f32, first: Option<Trace>, hits: &mut Touched) {
    let primal = *vel;
    let mut orig = *vel;
    let mut planes = [Vec3::ZERO; 2];
    let mut np = 0;
    let mut left = time;
    let mut last = *pos;
    let mut moved = false;
    let mut first = first;
    for _ in 0..8 {
        let tr = match first.take() {
            Some(t) => t,
            None => {
                let mut end = *pos + *vel * left;
                if (end - *pos).length_squared() < 1e-4 {
                    break;
                }
                for n in &planes[..np] {
                    let d = (end - *pos).dot(*n);
                    if d < 0.0 {
                        end -= *n * (d - 0.001);
                    }
                }
                h.trace(*pos, end)
            }
        };
        if tr.fraction > 0.0 {
            *pos = tr.end;
            moved = true;
            if tr.fraction >= 1.0 {
                return;
            }
            if (*pos - last).length_squared() > 1.0 {
                np = 0;
                orig = *vel;
                last = *pos;
            }
        }
        let n = tr.normal;
        hits.push(n);
        left -= left * tr.fraction;
        *vel = orig + n * (1.0 / 128.0 - orig.dot(n));
        let crease = match np {
            0 => {
                planes[0] = n;
                np = 1;
                false
            }
            1 if planes[0].dot(*vel) >= 0.0 => {
                planes[0] = n;
                false
            }
            1 => {
                planes[1] = n;
                np = 2;
                true
            }
            _ => {
                let (d0, d1) = (planes[0].dot(n), planes[1].dot(n));
                if d0.max(d1) < 0.866 {
                    *vel = Vec3::ZERO;
                    return;
                }
                planes[(d1 > d0) as usize] = n;
                true
            }
        };
        if crease {
            let cr = planes[0].cross(planes[1]);
            let sum = planes[0] + planes[1];
            let k = (cr.length_squared() / (sum.length_squared() + f32::MIN_POSITIVE)).sqrt() / 128.0;
            let dir = cr + sum * k.copysign(cr.dot(orig));
            *vel = dir * (dir.dot(orig) / (dir.length_squared() + f32::MIN_POSITIVE));
        }
        if vel.dot(primal) <= 0.0 {
            *vel = Vec3::ZERO;
            return;
        }
    }
    if !moved {
        *vel = Vec3::ZERO;
    }
}

/// StepMove (client.dll 0x1801fb0e0, Respawn's version of Source's): the move both as it is
/// (TryPlayerMove, with the blocked trace `first`) and lifted by the step height (+
/// DIST_EPSILON) and dropped back down by the height lifted plus the step height. The lifted
/// one is kept when it got further across the ground (more than 1/0.999 of the plain move's
/// distance) and came down on a floor. "Up", "height" and "floor" are along the hull's up
/// direction (the pointer StepMove takes, player+0x2494: on a wall-run the wall's normal, so a
/// wall-run steps out over trims and frames on the wall as a walk steps up a curb). Coming
/// down on something steep is still a step when stepTestDist further along there is floor
/// (steep bits shorter than that, like a bevelled curb, are ignored), or when that spot is not
/// much higher than where the plain move ended (at most about 45 degrees up from it, 0.49 =
/// sin^2), with the first landing no more than twice as high (at least a tenth of the step)
/// as that spot. Returns the height stepped up.
fn step_move(h: &Hull, pos: &mut Vec3, vel: &mut Vec3, time: f32, first: Trace, hits: &mut Touched) -> f32 {
    let p = h.p;
    if *vel == Vec3::ZERO {
        return 0.0;
    }
    let (start, v0, upv) = (*pos, *vel, h.up);
    let planar = |a: Vec3, b: Vec3| {
        let d = a - b;
        (d - upv * d.dot(upv)).length_squared()
    };
    try_player_move(h, pos, vel, time, Some(first), hits);
    let down = *pos;
    let lift = p.step_height + DIST_EPSILON;
    let tr = h.trace(start, start + upv * lift);
    let (top, lifted) = if tr.start_solid { (start, 0.0) } else { (tr.end, tr.fraction * lift) };
    let (mut up, mut up_vel) = (top, v0);
    try_player_move(h, &mut up, &mut up_vel, time, None, &mut Touched::default());
    if planar(down, start) >= planar(up, start) * 0.999 {
        return 0.0;
    }
    let drop = lifted + p.step_height + DIST_EPSILON;
    let tr = h.trace(up, up - upv * drop);
    let land = if tr.start_solid { up } else { tr.end };
    if tr.normal.dot(upv) < WALKABLE_Z {
        let gap = planar(land, down);
        if gap <= 0.0 {
            return 0.0;
        }
        let (spot, nz, tested) = if gap < p.step_test_dist * p.step_test_dist {
            let (mut q, mut qv) = (top, v0 - upv * v0.dot(upv).min(0.0));
            try_player_move(h, &mut q, &mut qv, p.step_test_dist / v0.length(), None, &mut Touched::default());
            let t2 = h.trace(q, q - upv * drop);
            (t2.end, t2.normal.dot(upv), true)
        } else {
            (land, tr.normal.dot(upv), false)
        };
        let mut ok = tested && nz >= WALKABLE_Z;
        if !ok && nz > 0.0 {
            let d = spot - down;
            let dz = d.dot(upv);
            let flat = planar(spot, down);
            ok = dz <= 0.0 || flat / (flat + dz * dz) > 0.49;
        }
        if !ok || (land - start).dot(upv).abs() > (2.0 * (spot - start).dot(upv).abs()).max(0.1 * p.step_height) {
            return 0.0;
        }
    }
    *pos = land;
    *vel = up_vel;
    (land - start).dot(upv)
}

/// What a move does when its trace `tr` hits something (client.dll 0x1802055b0). A floor
/// turns the rest of the move along it, 0.01 up, and traces again: on the ground at the full
/// speed along the slope; in the air keeping the horizontal velocity (the speed at most).
/// Anything else, or a second hit, goes to StepMove, which also climbs in the air. Returns
/// whether the turned move got through, and the height stepped up.
fn handle_blocked(h: &Hull, pos: &mut Vec3, vel: &mut Vec3, time: f32, tr: Trace, on_ground: bool, hits: &mut Touched) -> (bool, f32) {
    let n = tr.normal;
    if n.z > WALKABLE_Z && !tr.start_solid {
        let left = (1.0 - tr.fraction) * time;
        let v = *vel;
        let r = v * n.z - Vec3::Z * n.dot(v);
        let full = (v.length_squared() / r.length_squared().max(1e-12)).sqrt();
        let along = if !on_ground && n.z * n.z * v.length_squared() > r.length_squared() { r * (left / n.z) } else { r * (full * left) };
        let dest = tr.end + along + Vec3::Z * 0.01;
        let t2 = h.trace(tr.end, dest);
        if !t2.hit() {
            *pos = dest;
            return (true, 0.0);
        }
        // The rest from the original position, the first try being the turned move.
        let t = Trace { fraction: tr.fraction + (1.0 - tr.fraction) * t2.fraction, ..t2 };
        return (false, step_move(h, pos, vel, time, t, hits));
    }
    (false, step_move(h, pos, vel, time, tr, hits))
}

/// StayOnGround (client.dll 0x180207860 / 0x1801faed0): down onto a floor within the step
/// height (from 1 unit up when the hull starts stuck), so walking down steps and slopes keeps
/// the Pilot on them.
fn stay_on_ground(h: &Hull, pos: &mut Vec3) {
    let end = *pos - Vec3::Z * h.p.step_height;
    let mut tr = h.trace(*pos, end);
    if tr.start_solid {
        tr = h.trace(*pos + Vec3::Z, end);
        if tr.start_solid {
            return;
        }
    }
    if tr.fraction > 0.0 && tr.fraction < 1.0 && tr.normal.z >= WALKABLE_Z && (tr.end.z - pos.z).abs() > 1.0 / 64.0 {
        *pos = tr.end;
    }
}

/// One tick of movement with collision: client.dll's ground move (0x18020cc30) or AirMove
/// (0x1801f1bd0), then CategorizePosition (0x1801f3160). Returns the speed the move had into
/// the wall that blocked it (along the wall's flattened normal; 0 if none).
fn move_once(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, input: &PilotInput, dt: f32) -> f32 {
    let h = Hull::new(world, p, s.crouched);
    h.unstick(&mut s.pos);
    let vel_before = s.vel;
    let mut hits = Touched::default();
    // The surface the hull ran into (blocked, not stepped over), for the wall-run start.
    let mut wall: Option<Vec3> = None;
    let on_ground_before = s.on_ground();
    if on_ground_before {
        // The velocity turned onto the floor's plane at its full length (the speed counts along
        // slopes), kept out of the floor's plane.
        let (g, v) = (s.ground_normal, s.vel);
        let r = v * g.z - Vec3::Z * g.dot(v);
        let r = if r.length_squared() > 1e-8 { r * (v.length() / r.length()) } else { v };
        let mut dest = s.pos + r * dt;
        let d = (dest - s.pos).dot(g);
        if d < 0.0 {
            dest -= g * (d - 0.001);
        }
        let tr = h.trace(s.pos, dest);
        if !tr.hit() {
            s.pos = dest;
        } else {
            let (done, rise) = handle_blocked(&h, &mut s.pos, &mut s.vel, dt, tr, true, &mut hits);
            if !done {
                wall = Some(tr.normal);
            }
            // Sliding up a step costs slide_step_velocity_reduction per unit of its height (the
            // StepMove wrapper, client.dll 0x180207c6e).
            if rise > 0.0 && s.mode == PilotMove::Slide {
                let hz = hv(s.vel);
                let sp = hz.length();
                if sp > 0.0 {
                    let k = (sp - p.slide_step_loss * rise).max(0.0) / sp;
                    s.vel.x *= k;
                    s.vel.y *= k;
                }
            }
        }
        stay_on_ground(&h, &mut s.pos);
        // On the ground the velocity stays along it (the dispatcher, 0x1801f9693).
        s.vel.z = 0.0;
    } else {
        let dest = s.pos + s.vel * dt;
        let tr = h.trace(s.pos, dest);
        if !tr.hit() {
            s.pos = dest;
        } else if let PilotMove::WallRun { normal, .. } = s.mode {
            // On a wall-run the wall is the ground (the dispatcher, 0x1801f9621, runs the
            // surface move 0x18020cc30 with the Pilot's up vector turned to the wall's normal),
            // so what blocks the run goes through StepMove measured from the wall: trims,
            // frames and pillars up to the step height out from it are stepped over. The plain
            // move inside StepMove is the slide along the wall as before; the stepped one is
            // only taken when it gets further along the wall.
            let hw = Hull { up: normal.normalize_or(Vec3::Z), ..Hull::new(world, p, s.crouched) };
            step_move(&hw, &mut s.pos, &mut s.vel, dt, tr, &mut hits);
        } else {
            // Falling slowly (under 300 u/s across) into something steep, a while after leaving
            // the ground: just slide along it, without climbing, unless there is floor within
            // the step height under the hit (client.dll 0x1801f23cc).
            let n = tr.normal;
            let slow_fall = s.vel.z < 0.0
                && hv(s.vel).length_squared() < 90000.0
                && n.z < WALKABLE_Z
                && s.since_ground > 0.1
                && if n.z <= 0.0 {
                    let under = h.trace(tr.end, tr.end - Vec3::Z * p.step_height);
                    !under.hit() || under.normal.z < WALKABLE_Z
                } else {
                    (1.0 / 128.0 - s.vel.dot(n)) * n.z + s.vel.z < 0.0
                };
            if slow_fall {
                try_player_move(&h, &mut s.pos, &mut s.vel, dt, Some(tr), &mut hits);
                wall = Some(n);
            } else if !handle_blocked(&h, &mut s.pos, &mut s.vel, dt, tr, false, &mut hits).0 {
                wall = Some(n);
            }
        }
    }
    let wall_impact = wall.map_or(0.0, |n| (-vel_before.dot(Vec3::new(n.x, n.y, 0.0).normalize_or_zero())).max(0.0));
    // WalkMove and AirMove clear the stored blocking normal before their trace and keep the
    // plane they ran into (client.dll 0x18020defe / 0x18020e226, 0x1801f2148 / 0x1801f25f9).
    s.block_normal = wall.unwrap_or(Vec3::ZERO);

    // CategorizePosition: a floor within the step height (2 units when airborne) and not rising
    // faster than NON_JUMP_VELOCITY is ground.
    let rising = s.vel.z > NON_JUMP_VELOCITY;
    let reach = if on_ground_before { p.step_height } else { 2.0 };
    let tr = h.trace(s.pos, s.pos - Vec3::Z * reach);
    let floor = !rising && !matches!(s.mode, PilotMove::WallRun { .. } if s.vel.z > 0.0) && tr.hit() && !tr.start_solid && tr.normal.z >= WALKABLE_Z;
    if floor {
        s.ground_normal = tr.normal;
        if !on_ground_before {
            s.landed = Some(-s.vel.z);
            s.mode = PilotMove::Ground;
            s.since_land = 0.0;
            s.wallrun_no_boost = true;
            s.vel.z = 0.0;
            // Landing while ducking or ducked (crouch held) starts a slide on this tick
            // (SetGroundEntity 0x1801fa1c6): the capture's crouched landing at 260 is at 400
            // on the landing frame itself.
            if s.duck_held > 0.0 && matches!(s.mode, PilotMove::Ground) && input.crouch {
                start_slide(s, p, wish_of(s.yaw, input), Vec2::new(s.yaw.cos(), s.yaw.sin()), false, dt);
            }
        }
    } else if on_ground_before {
        // Walked off a ledge: jump_graceperiod to still jump.
        s.mode = PilotMove::Air;
        s.jump_grace = p.jump_grace;
        s.ground_normal = Vec3::Z;
    }

    // Wall-running starts when the airborne move runs into a wall (client.dll AirMove's hull
    // trace, 0x1801f2628: there is no proximity sense): TryStartWallrun with the velocity from
    // before the collision. A start that only failed on the probes is retried once,
    // wallrun_retry_interval later, if the Pilot is still at the wall's plane (0x1801f22b6).
    if s.mode == PilotMove::Air {
        if let Some(n) = wall {
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
        let touched = hits.0.map(|n| Vec3::new(n.x, n.y, 0.0).normalize_or_zero());
        let sensed = wall_probe(world, p, s.pos + Vec3::Z * 16.0, normal)
            .filter(|(d, n)| *d <= p.wallrun.allowed_wall_dist && n.z.abs() <= 0.7)
            .map(|(_, n)| Vec3::new(n.x, n.y, 0.0).normalize_or_zero())
            .or(touched);
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
                    let max = p.wallrun.rotate_max_rate * dt;
                    let turn = a.perp_dot(b).atan2(a.dot(b)).clamp(-max, max);
                    let r = Vec2::from_angle(turn).rotate(a) * hv(normal).length();
                    s.mode = PilotMove::WallRun { normal: Vec3::new(r.x, r.y, normal.z), time };
                }
            }
        }
    }
    wall_impact
}

/// Move the flying hook: out at grapple_shootVel until it hits something (it sticks there) or
/// has flown its reach (it comes back at grapple_retractVel), back toward the eye.
fn update_hook(s: &mut PilotState, p: &PilotParams, world: &CollisionWorld, dt: f32) {
    let Some(mut h) = s.hook else { return };
    let eye = s.eye(p);
    match h.back {
        None => {
            let dir = h.vel.normalize_or_zero();
            let reach = (h.vel.length() * dt).min((h.max - h.travelled).max(0.0));
            if let Some(hit) = world.raycast(h.pos, dir, reach) {
                s.attach_grapple(hit.point);
                s.grapple_events.push(GrappleEvent::Hooked { point: hit.point, normal: hit.normal });
                return;
            }
            h.pos += dir * reach;
            h.travelled += reach;
            if h.travelled >= h.max - 1e-3 {
                h.back = Some(p.grapple.retract_speed);
                s.grapple_events.push(GrappleEvent::Missed);
            }
        }
        Some(speed) => {
            let to = eye - h.pos;
            let d = to.length();
            if d <= speed * dt {
                s.hook = None;
                return;
            }
            h.pos += to / d * speed * dt;
        }
    }
    s.hook = Some(h);
}

/// How the grapple lets go (client.dll 0x1804f7be0): plainly, with the impact hop at the hook,
/// or with the detach hop when moving away from it.
#[derive(Clone, Copy, PartialEq)]
enum Detach {
    Plain,
    Impact,
    Away,
}

/// One tick of a stuck hook. Returns true when the pull moved the Pilot (the normal movement
/// modes are skipped then); before grapple_pullDelay and after letting go they run as usual.
fn grapple_step(s: &mut PilotState, input: &PilotInput, wish: Vec2, p: &PilotParams, world: &CollisionWorld, dt: f32) -> bool {
    let g = &p.grapple;
    let Some(anchor) = s.grapple else { return false };
    s.grapple_time += dt;
    let eye = s.eye(p);
    // The pull aims at grapple_lift above the hook, from the eye (EyePosition, client.dll
    // 0x1804f5240).
    let to = anchor - eye + Vec3::Z * g.lift;
    let len = to.length().max(1e-3);
    let dir = to / len;
    // The engine measures the length as the nearer of the feet and the eye.
    let dist = (anchor - s.pos).length().min((anchor - eye).length());
    let along = s.vel.dot(dir);
    let (sp, cp) = s.pitch.sin_cos();
    let view = Vec3::new(cp * s.yaw.cos(), cp * s.yaw.sin(), -sp);
    let flat = Vec3::new(s.yaw.cos(), s.yaw.sin(), 0.0);
    let pulling = s.grapple_pull.is_some();

    let mut detach = None;
    let (lmin, lmax) = g.detach_length;
    if dist > s.grapple_reach + 50.0 {
        // Past the weapon's reach (max of grapple_maxLength and grapple_maxLengthVert) + 50.
        detach = Some(Detach::Plain);
    } else if dist < lmax && (dist < lmin || lmin + 1.5 * (along / g.pull_speed.1) * (lmax - lmin) > dist) {
        // At the hook: under detachLengthMin always, under detachLengthMax when fast.
        detach = Some(if pulling { Detach::Impact } else { Detach::Plain });
    } else if pulling {
        // Looking away from the hook: the view points away from it; looking down, the flat
        // view must point away from it as well. (The engine also keeps the hook when moving
        // up more steeply than a velocity it keeps in the move data; that test is left out.)
        let looking_away = if view.z >= 0.0 { to.dot(view) < 0.0 } else { to.dot(view) < 0.0 && to.dot(flat) < 0.0 && (anchor - eye).dot(flat) < 0.0 };
        let moving_away = along < 0.0;
        if moving_away && along * along > g.detach_away_speed * g.detach_away_speed {
            detach = Some(Detach::Away);
        } else if looking_away {
            detach = Some(if moving_away { Detach::Away } else { Detach::Plain });
        } else if along > g.low_speed {
            s.grapple_slow = 0.0;
        } else {
            // Slow toward the hook for too long (grapple_detachLowSpeed*Time).
            s.grapple_slow += dt;
            let limit = match s.grapple_touch {
                2 => g.low_speed_ground_time,
                1 => g.low_speed_wall_time,
                _ => g.low_speed_time,
            };
            if s.grapple_slow > limit {
                detach = Some(Detach::Plain);
            }
        }
        // The cable is cut when something comes between the eye and the hook
        // (client.dll 0x1801fee10).
        let d = (anchor - eye).length();
        if detach.is_none() && d > 8.0 && world.raycast(eye, (anchor - eye) / d, d).is_some_and(|h| h.t < d - 4.0) {
            detach = Some(Detach::Plain);
        }
    }
    if let Some(kind) = detach {
        let (boost, max) = match kind {
            Detach::Impact => (g.impact_boost, g.impact_max),
            Detach::Away => (g.detach_boost, g.detach_max),
            Detach::Plain => (0.0, f32::MAX),
        };
        if kind != Detach::Plain && s.vel.z < max {
            s.vel.z = (s.vel.z + boost).min(max);
        }
        s.release_grapple(p);
        return false;
    }

    if !pulling {
        if s.grapple_time < g.pull_delay {
            return false;
        }
        // The pull starts (client.dll 0x1804f4a10): the speed across the cable keeps
        // grapple_initialSlowFrac(Vert), and grapple_initialImpulse is added toward the hook
        // up to the ramp's starting speed.
        let a = along.max(0.0);
        let (fh, fv) = g.initial_slow_frac;
        s.vel = Vec3::new((s.vel.x - dir.x * a) * fh + dir.x * a, (s.vel.y - dir.y * a) * fh + dir.y * a, (s.vel.z - dir.z * a) * fv + dir.z * a);
        let na = s.vel.dot(dir);
        if g.pull_speed.0 > na {
            let target = (na + g.initial_impulse).max(g.initial_speed_min).min(g.pull_speed.0);
            s.vel += dir * (target - na);
        }
        // On the ground: off it, with grapple_initialImpulseOffGround (client.dll 0x1804f4d30).
        if s.on_ground() {
            s.vel.z = s.vel.z.max(0.0) + g.off_ground_impulse;
            s.mode = PilotMove::Air;
            s.pos.z += 2.0;
        }
        s.grapple_pull = Some(0.0);
        s.grapple_slow = 0.0;
        s.grapple_events.push(GrappleEvent::Pulling);
    }
    let t_pull = s.grapple_pull.map_or(0.0, |t| t + dt);
    s.grapple_pull = Some(t_pull);

    // Gravity, scaled (client.dll 0x1801fe9e0): full while the hook is steeply below (cos
    // grapple_letGravityHelpCosAngle); otherwise gravityFracMin, rising toward gravityFracMax
    // when steering away from the hook or holding forward while looking under the cable.
    let (x, y, z) = (to.x, to.y, to.z);
    let frac = if z < 0.0 && z * z / to.length_squared() > g.gravity_help_cos * g.gravity_help_cos {
        1.0
    } else {
        let wish3 = Vec3::new(flat.x, flat.y, 0.0) * input.forward + Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0) * input.right;
        let away = -(x * wish3.x + y * wish3.y);
        // u: perpendicular to the cable in its vertical plane, pointing up (|u| = |h| |to|).
        let u = Vec3::new(-x * z, -y * z, x * x + y * y);
        let fwd = input.forward;
        let under = (-10.0 * fwd * view.z).clamp(0.0, 1.0) * (-fwd * u.dot(view));
        if under <= 0.0 && away <= 0.0 {
            g.gravity_frac.0
        } else {
            let h = (x * x + y * y).sqrt().max(1e-3);
            let k = ((under.max(0.0) / u.length().max(1e-3)) * g.push_under).min(1.0).max(away.max(0.0) / h);
            g.gravity_frac.0 + (g.gravity_frac.1 - g.gravity_frac.0) * k
        }
    };
    s.vel.z -= p.gravity * frac * dt;

    // The pull (client.dll 0x1804f43e0): split the velocity along and across the cable. Across
    // it, falling speed (grapple_dontFightGravity) and the part along the view are kept; the
    // rest bleeds off at grapple_decel. Along it, grapple_accel up to the ramped speed limit
    // (never slowing what's faster). The pull never lowers the vertical speed.
    let old_z = s.vel.z;
    let along = s.vel.dot(dir);
    let mut perp = s.vel - dir * along;
    let fall = perp.z.min(0.0);
    perp.z -= fall;
    let d = view.dot(perp);
    let keep = if d > 0.0 { view * d } else { Vec3::ZERO };
    perp -= keep;
    let pl = perp.length();
    if pl > 1e-4 {
        perp *= (pl - g.decel * dt).max(0.0) / pl;
    }
    let ramp = (t_pull / g.pull_ramp_time.max(1e-3)).min(1.0);
    let limit = g.pull_speed.0 + (g.pull_speed.1 - g.pull_speed.0) * ramp;
    let along = if along < limit { (along + g.pull_accel * dt).min(limit) } else { along };
    s.vel = dir * along + perp + keep + Vec3::Z * fall;
    s.vel.z = s.vel.z.max(old_z);

    // Steering (client.dll 0x1801f19a0 with grapple_airSpeedMax / grapple_airAccel): toward
    // the wish direction up to the max speed along it; past that the direction still turns
    // (player_extraairaccelleration 2) without gaining speed.
    if wish != Vec2::ZERO {
        let w = wish.normalize();
        let w3 = Vec3::new(w.x, w.y, 0.0);
        let add = g.air_speed * wish.length().min(1.0) - s.vel.dot(w3);
        if add > 0.0 {
            s.vel += w3 * (g.air_accel * dt).min(add);
        } else {
            let before = s.vel.length().max(g.air_speed * wish.length().min(1.0));
            s.vel += w3 * 2.0 * dt;
            let now = s.vel.length();
            if now > before {
                s.vel *= before / now;
            }
        }
    }
    // Jumping while pulled is an ordinary (double) jump; the pull goes on (grapple_jumpFrac 1).
    if input.jump && s.double_jump_available {
        superjump(s, Vec2::ZERO, p);
    }
    s.mode = PilotMove::Air;
    if !stand_in_air(s, p, world) {
        s.crouched = false;
    }

    let still = PilotInput { forward: 0.0, ..*input };
    let impact = move_once(s, p, world, &still, dt);
    s.grapple_touch = if s.on_ground() { 2 } else if impact > 0.0 { 1 } else { 0 };
    s.mode = PilotMove::Air;
    true
}

/// A ledge in front of the player that can be mantled onto: a wall at chest height, a flat top
/// no higher than a little above the head, and room to stand there. Movement rays: player
/// clips and prop hulls, not props' render triangles.
fn mantle_target(s: &PilotState, p: &PilotParams, world: &CollisionWorld, fwd: Vec2) -> Option<Vec3> {
    let ray = |o: Vec3, d: Vec3, t: f32| world.raycast_moving(o, d, t, CLIP_PILOT);
    let f = Vec3::new(fwd.x, fwd.y, 0.0);
    let chest = s.pos + Vec3::Z * (p.height * 0.55);
    // Something blocking right in front.
    ray(chest, f, p.radius + 14.0)?;
    // Look down onto the top from above the reach height, just past the wall.
    let reach = p.height + 26.0;
    let probe = s.pos + f * (p.radius + 18.0) + Vec3::Z * (reach + 4.0);
    let hit = ray(probe, -Vec3::Z, reach)?;
    let rise = hit.point.z - s.pos.z;
    if hit.normal.z < 0.7 || rise < p.step_height + 4.0 || rise > reach {
        return None;
    }
    // Head room on top, and a clear path over the edge.
    let top = Vec3::new(probe.x, probe.y, hit.point.z);
    if ray(top + Vec3::Z * 2.0, Vec3::Z, p.height - 4.0).is_some() {
        return None;
    }
    if ray(s.pos + Vec3::Z * (rise + 6.0), f, p.radius + 18.0).is_some() {
        return None;
    }
    Some(top)
}
