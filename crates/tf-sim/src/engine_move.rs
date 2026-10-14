//! Pilot and Titan movement as client.dll runs it: a function-by-function port of the engine's player
//! move code (Titanfall 2 v2.0.11.0, the Ghidra decompile kept outside the repo) for move type 2
//! (walk) and the world only. Every function here names the engine function it mirrors
//! (`0x1802041c0` style) and keeps its order of operations; the state fields carry the engine's
//! `player+0x....` / `mv+0x..` offsets in their comments so the two can be read side by side.
//! See `docs/engine-movement.md` for the prose version.
//!
//! What is left out (no world data for it or not reached by a Pilot on a map alone): other
//! entities (ground velocity, pushaway, repel, traverse / window hints, grapple),
//! mantling (`0x180208bf0`; the caller may run its own), ziplines, sounds, anim events, view
//! punch and the step-smoothing eye offset (`player+0x1e10`).

use crate::collision::{CollisionWorld, Trace, CLIP_PILOT};

use glam::Vec3;

// Button bits (mv+0x18 held, +0x1c old, +0x20 pressed this frame).
pub const IN_ATTACK: u32 = 1;
pub const IN_JUMP: u32 = 2;
pub const IN_DUCK: u32 = 4;
pub const IN_FORWARD: u32 = 8;
pub const IN_BACK: u32 = 0x10;
pub const IN_MOVELEFT: u32 = 0x200;
pub const IN_MOVERIGHT: u32 = 0x400;
pub const IN_SPEED: u32 = 0x8000;
pub const IN_ZOOM: u32 = 0x10000;
pub const IN_ZOOM_TOGGLE: u32 = 0x20000;
pub const IN_DUCKTOGGLE: u32 = 0x4000000;
pub const IN_DODGE: u32 = 1 << 28;

/// The engine's gravity direction (`DAT_180b18718`).
const DOWN: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// The engine's normalize epsilon (`DAT_1811b02a0`): lengths at or under it divide as it.
const NORM_EPS: f32 = 1.1754944e-38;
/// FL_DUCKING in `player+0x8c`.
const FL_DUCKING: u32 = 2;
/// Enables the in-air duck / unduck origin shift from the block normal (`DAT_180c3d87e`; off).
const UNDUCK_BY_BLOCK_NORMAL: bool = false;

#[inline]
fn norm(v: Vec3) -> Vec3 {
    // VectorNormalize as the engine inlines it: 1 / max(|v|, eps).
    v * (1.0 / v.length().max(NORM_EPS))
}

/// Player settings (`settings+0x..` of the player-settings struct, `.set` keys), the per-class
/// block the player keeps a copy of (`player+0x205c..`), the pose blocks (`0x18019ed50`) and the
/// hulls. Defaults: `pilot_mp.set` over the engine's defaults.
#[derive(Debug, Clone)]
pub struct Settings {
    pub jump: bool,            // 0x82
    pub crouch: bool,          // 0x83
    pub slide: bool,           // 0x84
    pub jump_height: f32,      // 0x94
    pub speed_scale_side: f32, // 0x98
    pub speed_scale_back: f32, // 0x9c
    pub step_height: f32,      // 0xa0 (player+0x18a8)
    pub step_test_dist: f32,   // 0xa4
    pub air_speed: f32,        // 0xbc (player+0x2bc8)
    pub air_accel: f32,        // 0xc0 (player+0x2bcc)
    pub air_strafe: bool,      // 0x1c8
    pub superjump: bool,       // player+0x2078
    pub superjump_min_height: f32, // 0x100
    pub superjump_max_height: f32, // 0x104
    pub superjump_limit: f32,      // 0x108
    pub superjump_horz_speed: f32, // 0x10c
    pub superjump_after_wallrun: bool, // 0x110
    pub mantle_angle_scale: f32,   // 0x114
    pub wallstick: bool,           // 0x218
    pub wallrun_enabled: bool,     // player+0x2070
    pub wallrun_ads_type: i32,     // player+0x2074 (1: wallhang, 0: ADS)
    pub wallrun_max_speed_vertical: f32,           // 0x21c
    pub wallrun_max_speed_horizontal_backward: f32, // 0x220
    pub wallrun_friction: f32,                      // 0x224
    pub wallrun_jump_up_speed: f32,                 // 0x228
    pub wallrun_jump_input_dir_speed: f32,          // 0x22c
    pub wallrun_jump_outward_speed: f32,            // 0x230
    pub wallrun_same_wall_allowed: bool,            // 0x234
    pub wallrun_accel_vertical: f32,    // player+0x205c
    pub wallrun_accel_horizontal: f32,  // player+0x2060
    pub wallrun_max_speed_horizontal: f32, // player+0x2064
    pub wallrun_time_limit: f32,        // player+0x2068
    pub wallrun_hang_time_limit: f32,   // player+0x206c
    pub sprint: bool,                   // player+0x2086
    /// The sprint fraction's timing (settings +0x267c..+0x2688): the delay before the slow
    /// ramp, its length, the fast ramp's (a sprint resumed soon after one ended) and the ramp
    /// down.
    pub sprint_start_delay: f32,
    pub sprint_start_duration: f32,
    pub sprint_start_fast_duration: f32,
    pub sprint_end_duration: f32,
    /// The dodge (the Titan dash; `dodge`, player+0x2085) and its numbers: the per-class block
    /// (`powerRegenRate` player+0x2048, `dodgeDuration` +0x204c, `dodgeSpeed` +0x2050,
    /// `dodgePowerDrain` +0x2054, copied from settings+0x2b80.. by `0x1802c2c20`) and the
    /// settings struct's own (`dodgeAnimDuration` 0x238 .. `dodgeKeepSpeedFrac` 0x254).
    pub dodge: bool,
    pub power_regen_rate: f32,
    pub dodge_duration: f32,
    pub dodge_speed: f32,
    pub dodge_power_drain: f32,
    pub dodge_anim_duration: f32,   // 0x238
    pub dodge_interval: f32,        // 0x23c
    pub dodge_stop_speed: f32,      // 0x240
    pub dodge_height: f32,          // 0x244
    pub dodge_vertical_height: f32, // 0x24c
    pub dodge_power_delay: f32,     // 0x250
    pub dodge_keep_speed_frac: f32, // 0x254
    /// Which clip brushes block this player: `CLIP_PILOT` or `CLIP_TITAN`.
    pub clip: usize,
    pub slide_decel: f32,               // 0x258
    pub slide_want_to_stop_decel: f32,  // 0x25c
    pub slide_velocity_decay: f32,      // 0x260
    pub slide_accel: f32,               // 0x264
    pub slide_required_start_speed: f32, // 0x268
    pub slide_stop_speed: f32,          // 0x26c
    pub slide_max_stop_speed: f32,      // 0x270
    pub slide_max_jump_speed: f32,      // 0x274
    pub slide_speed_boost: f32,         // 0x278
    pub slide_speed_boost_cap: f32,     // 0x27c
    pub slide_jump_height: f32,         // 0x280
    /// `gravityscale` (player vtable+0x5d8).
    pub gravity_scale: f32,
    /// Pose blocks [stand, crouch]: `speed` / `sprintspeed` (player+0x20cc + pose*8 / +0x20d0),
    /// `lowSpeed` (pose+0x44), `acceleration` (+0x48), `deceleration` (+0x4c),
    /// `lowAcceleration` (+0x50), `sprintAcceleration` (+0x54), `sprintDeceleration` (+0x58).
    pub pose_speed: [f32; 2],
    pub pose_sprint_speed: [f32; 2],
    pub pose_low_speed: [f32; 2],
    pub pose_accel: [f32; 2],
    pub pose_decel: [f32; 2],
    pub pose_low_accel: [f32; 2],
    pub pose_sprint_accel: [f32; 2],
    pub pose_sprint_decel: [f32; 2],
    /// Standing hull (player+0x2460 / +0x246c) and ducked hull (+0x2478 / +0x2484).
    pub stand_mins: Vec3,
    pub stand_maxs: Vec3,
    pub duck_mins: Vec3,
    pub duck_maxs: Vec3,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            jump: true,
            crouch: true,
            slide: true,
            jump_height: 60.0,
            speed_scale_side: 1.0,
            speed_scale_back: 1.0,
            step_height: 18.0,
            step_test_dist: 10.0,
            air_speed: 60.0,
            air_accel: 500.0,
            air_strafe: false,
            superjump: true,
            superjump_min_height: 60.0,
            superjump_max_height: 60.0,
            superjump_limit: 1.0,
            superjump_horz_speed: 180.0,
            superjump_after_wallrun: true,
            mantle_angle_scale: 1.0,
            wallstick: false,
            wallrun_enabled: true,
            wallrun_ads_type: 0,
            wallrun_max_speed_vertical: 225.0,
            wallrun_max_speed_horizontal_backward: 50.0,
            wallrun_friction: 4.0,
            wallrun_jump_up_speed: 230.0,
            wallrun_jump_input_dir_speed: 80.0,
            wallrun_jump_outward_speed: 205.0,
            wallrun_same_wall_allowed: true,
            wallrun_accel_vertical: 360.0,
            wallrun_accel_horizontal: 1500.0,
            wallrun_max_speed_horizontal: 420.0,
            wallrun_time_limit: 1.75,
            wallrun_hang_time_limit: 4.0,
            sprint: true,
            sprint_start_delay: 0.2,
            sprint_start_duration: 0.8,
            sprint_start_fast_duration: 0.2,
            sprint_end_duration: 0.15,
            dodge: false,
            // The class block's dodge numbers have no .set defaults a Pilot reaches: these are
            // titan_buddy.set's, so a Settings with `dodge` on dashes like BT.
            power_regen_rate: 12.0,
            dodge_duration: 0.3,
            dodge_speed: 685.0,
            dodge_power_drain: 50.0,
            dodge_anim_duration: 0.5,
            dodge_interval: 0.6,
            dodge_stop_speed: 300.0,
            dodge_height: 25.0,
            dodge_vertical_height: 200.0,
            dodge_power_delay: 0.0,
            dodge_keep_speed_frac: 0.0,
            clip: CLIP_PILOT,
            slide_decel: 50.0,
            slide_want_to_stop_decel: 400.0,
            slide_velocity_decay: 0.7,
            slide_accel: 50.0,
            slide_required_start_speed: 200.0,
            slide_stop_speed: 125.0,
            slide_max_stop_speed: 350.0,
            slide_max_jump_speed: 350.0,
            slide_speed_boost: 150.0,
            slide_speed_boost_cap: 400.0,
            slide_jump_height: 50.0,
            gravity_scale: 0.8,
            pose_speed: [173.5, 80.0],
            pose_sprint_speed: [260.0, 80.0],
            pose_low_speed: [-1.0, -1.0],
            pose_accel: [2500.0, 2500.0],
            pose_decel: [-1.0, -1.0],
            pose_low_accel: [-1.0, -1.0],
            pose_sprint_accel: [-1.0, -1.0],
            pose_sprint_decel: [-1.0, -1.0],
            stand_mins: Vec3::new(-16.0, -16.0, 0.0),
            stand_maxs: Vec3::new(16.0, 16.0, 72.0),
            duck_mins: Vec3::new(-16.0, -16.0, 0.0),
            duck_maxs: Vec3::new(16.0, 16.0, 47.0),
        }
    }
}

/// ConVars the move code reads (client.dll defaults).
#[derive(Debug, Clone)]
pub struct Cvars {
    pub sv_gravity: f32,
    pub sv_maxvelocity: f32,
    pub sv_optimizedmovement: bool,
    pub jump_graceperiod: f32,
    pub skip_time: f32,
    pub skip_jump_height_fraction: f32,
    pub skip_speed_reduce: f32,
    pub skip_speed_retain: f32,
    pub skip_replenish_double_jump: bool,
    pub slide_boost_cooldown: f32,
    pub slide_max_angle_dot: f32,
    pub slide_step_velocity_reduction: f32,
    pub slide_while_in_air: bool,
    pub slide_auto_stand: bool,
    pub player_extraairaccelleration: f32,
    pub jump_keyboardgraceperiodmin: f32,
    pub jump_keyboardgraceperiodmax: f32,
    pub jump_keyboardgrace_strength: f32,
    pub jump_keyboardgrace_max: f32,
    pub superjump_min_height_fraction: f32,
    pub superjump_min_power_use: f32,
    pub superjump_max_power_use: f32,
    pub superjump_limitreset_onwallrun: bool,
    pub superjump_powerreset_onground: bool,
    pub dodge_vertical_in_air: bool,
    pub dodge_no_movement_forward_enable: bool,
    pub dodge_vertical_enable: bool,
    pub dodge_vertical_threshold: f32,
    pub dodge_vertical_horzspeedscale: f32,
    pub wallrun_enable: bool,
    pub wallrun_up_wall_boost: f32,
    pub wallrun_gravity_ramp_up_time: f32,
    pub wallrun_upward_auto_push: f32,
    pub wallrun_slip_start_time: f32,
    pub wallrun_slip_duration: f32,
    pub wallrun_no_input_slip_frac: f32,
    pub wallrun_fall_away_speed: f32,
    pub wallrun_push_away_fall_off_time: f32,
    pub wallrun_same_wall_dot: f32,
    pub wallrun_same_wall_height: f32,
    pub wallrun_same_wall_dist: f32,
    pub wallrun_same_wall_slope: f32,
    pub wallrun_min_angle_air: f32,
    pub wallrun_allowed_wall_dist: f32,
    pub wallrun_allowed_wall_dist_wallhang: f32,
    pub wallrun_retry_interval: f32,
    pub wallrun_rotate_max_rate: f32,
    pub wallrun_angle_change_min_cos: f32,
    pub wallrun_strength_loss_start: f32,
    pub wallrun_strength_loss_end: f32,
    pub wallrun_hang_slip_start_time: f32,
    pub wallrun_hang_slip_duration: f32,
    pub wallrun_hang_stop_time: f32,
    pub wallrun_avoid_wall_top_decel: f32,
    pub wallrun_repel_enable: bool,
    pub automantle_maxangle_push: f32,
    pub automantle_maxangle_view: f32,
    pub automantle_wallrun_maxangle_view: f32,
}

impl Default for Cvars {
    fn default() -> Self {
        Self {
            sv_gravity: 750.0,
            sv_maxvelocity: 12100.0,
            sv_optimizedmovement: true,
            jump_graceperiod: 0.2,
            skip_time: 0.1,
            skip_jump_height_fraction: 0.75,
            skip_speed_reduce: 12.0,
            skip_speed_retain: 450.0,
            skip_replenish_double_jump: true,
            slide_boost_cooldown: 2.0,
            slide_max_angle_dot: 0.6,
            slide_step_velocity_reduction: 10.0,
            slide_while_in_air: false,
            slide_auto_stand: false,
            player_extraairaccelleration: 2.0,
            jump_keyboardgraceperiodmin: 0.2,
            jump_keyboardgraceperiodmax: 0.5,
            jump_keyboardgrace_strength: 0.7,
            jump_keyboardgrace_max: 0.7,
            superjump_min_height_fraction: 0.25,
            superjump_min_power_use: 0.0,
            superjump_max_power_use: 100.0,
            superjump_limitreset_onwallrun: true,
            superjump_powerreset_onground: true,
            dodge_vertical_in_air: true,
            dodge_no_movement_forward_enable: true,
            dodge_vertical_enable: false,
            dodge_vertical_threshold: 0.1,
            dodge_vertical_horzspeedscale: 0.5,
            wallrun_enable: true,
            wallrun_up_wall_boost: 250.0,
            wallrun_gravity_ramp_up_time: 1.0,
            wallrun_upward_auto_push: 0.65,
            wallrun_slip_start_time: 1.5,
            wallrun_slip_duration: 1.0,
            wallrun_no_input_slip_frac: 0.7,
            wallrun_fall_away_speed: 70.0,
            wallrun_push_away_fall_off_time: 0.05,
            wallrun_same_wall_dot: 0.9,
            wallrun_same_wall_height: 0.0,
            wallrun_same_wall_dist: 100.0,
            wallrun_same_wall_slope: 0.0,
            wallrun_min_angle_air: 180.0,
            wallrun_allowed_wall_dist: 13.0,
            wallrun_allowed_wall_dist_wallhang: 18.0,
            wallrun_retry_interval: 0.07,
            wallrun_rotate_max_rate: 3.0,
            wallrun_angle_change_min_cos: 0.8,
            wallrun_strength_loss_start: 1000.0,
            wallrun_strength_loss_end: 1000.0,
            wallrun_hang_slip_start_time: 3.0,
            wallrun_hang_slip_duration: 1.0,
            wallrun_hang_stop_time: 0.5,
            wallrun_avoid_wall_top_decel: 3000.0,
            wallrun_repel_enable: true,
            automantle_maxangle_push: 50.0,
            automantle_maxangle_view: 50.0,
            automantle_wallrun_maxangle_view: 45.0,
        }
    }
}

/// The inputs of one frame (the usercmd the engine runs the move with).
#[derive(Debug, Clone, Copy, Default)]
pub struct Cmd {
    /// Held buttons (`IN_*`).
    pub buttons: u32,
    /// Forward / side move in stick units (-1..1): `mv+0x24` / `+0x28`, usercmd +0x34 / +0x38.
    pub fmove: f32,
    pub smove: f32,
    /// View angles in degrees.
    pub yaw: f32,
    pub pitch: f32,
}

/// The player and move data the walk move reads and writes.
#[derive(Debug, Clone)]
pub struct Pm {
    // --- CMoveData ---
    pub origin: Vec3,   // mv+0xd8
    pub vel: Vec3,      // mv+0x8c
    pub saved_vel: Vec3, // mv+0x98 (last frame's final velocity)
    pub buttons: u32,   // mv+0x18
    pub old_buttons: u32, // mv+0x1c
    pub pressed: u32,   // mv+0x20
    pub fmove: f32,     // mv+0x24
    pub smove: f32,     // mv+0x28
    pub yaw: f32,       // mv+0x10 (degrees)
    pub pitch: f32,     // mv+0xc
    pub fwd: Vec3,      // mv+0x30 (the view forward, with pitch)
    pub right: Vec3,    // mv+0x3c
    pub up: Vec3,       // mv+0x48
    /// The view forward and right flattened to the horizontal plane and normalized
    /// (mv+0x54 / +0x60, CPrediction::SetupMove `0x1802d8b40`): the wish direction, the slide's
    /// angle test, the keyboard grace and the wall jump's "looking into the wall" read these.
    pub fwd_flat: Vec3,
    pub right_flat: Vec3,
    pub wishdir: Vec3,  // mv+0x6c: fwd_flat * fmove + right_flat * smove, normalized when over 1
    pub maxspeed: f32,  // mv+0x88
    pub summed_wish: Vec3, // mv+0xb0
    // --- C_Player ---
    pub flags: u32,          // +0x8c (FL_DUCKING = 2)
    pub ducktime: i32,       // +0x188c m_flDucktime, integer milliseconds
    pub duck_state: i32,     // +0x245c (0 standing, 1 ducking, 2 ducked, 3 unducking)
    pub duck_toggle: bool,   // +0x18bc
    pub forced_slide: i32,   // +0x18b8
    pub sliding: bool,       // +0x2600
    pub boost_pending: bool, // +0x2601
    pub slide_start: f32,    // +0x2604
    pub boost_amount: f32,   // +0x2608
    pub normal: Vec3,        // +0x2494 ground / wall normal (the up vector of the move)
    pub prev_normal: Vec3,   // +0x24a0
    pub ground_normal: Vec3, // +0x3b7c (0x1802c71a0): the floor plane of the last ground trace
    pub block_normal: Vec3,  // +0x2b70: what the last move ran into
    pub on_ground: bool,     // ground entity handle +0x38c != -1 (world only here)
    pub time_left_ground: f32, // +0x2b6c
    pub leave_ground_time: f32, // +0x2b90 (0x1802c5860 = now - this): jump_graceperiod reference
    pub grace_normal: Vec3,  // +0x2ba0: the normal the grace jump uses
    pub jump_time: f32,      // +0x2b94
    pub land_time: f32,      // +0x2b98
    pub jumped_since_leaving: bool, // +0x2bac
    pub wallrun_start_time: f32, // +0x2b64
    pub wallrun_end_time: f32,   // +0x2b68
    pub wallrun_start_pos: Vec3, // +0x24ac
    pub wallrun_count: i32,      // +0x24b8
    pub no_boost: bool,          // +0x24bc: the next wall-run starts without the up-wall boost
    pub push_away_timer: f32,    // +0x24c0
    pub wallrun_friction_scale: f32, // +0x24c4
    pub decel_scale: f32,        // +0x24c8
    pub retry_time: f32,         // +0x24cc
    pub retry_pos: Vec3,         // +0x24d0
    pub retry_normal: Vec3,      // +0x24dc
    pub hanging: bool,           // +0x2588 wall hang
    pub hang_start: f32,         // +0x24fc
    pub hang_time: f32,          // +0x2500
    pub last_walls: Vec<Vec3>,   // +0x19f0.. (count +0x1a0c): flattened normals of walls jumped off
    pub last_wall_time: f32,     // +0x1a08
    pub sprinting: bool,
    /// The sprint fraction's bookkeeping (`0x1802cdb90` start / `0x1802cdd10` end): when the
    /// sprint started (+0x243c) and the fraction then (+0x2440, kept to 1/1023), the same for
    /// its end (+0x2444 / +0x2448), and when the latch's attempt began (+0x244c).
    pub sprint_started_time: f32,
    pub sprint_started_frac: f32,
    pub sprint_ended_time: f32,
    pub sprint_ended_frac: f32,
    pub sticky_sprint_start: f32,         // +0x2434
    pub sprint_window: f32,      // +0x2428
    pub sprint_latch: bool,      // +0x2610
    pub zoom_toggle: bool,       // +0x1699
    pub zoomed: bool,            // +0x1698
    pub friction_factor: f32,    // +0x2f50
    pub move_scale: f32,         // +0x18c0
    pub impulse_time: f32,       // +0x2450
    pub jumps_used: i32,         // +0x1890
    pub jump_power: f32,         // +0x189c
    pub dodge_power: f32,        // +0x1898
    pub power_delay_end: f32,    // +0x2424: no dodge power regen before this time
    pub dodge_time: f32,         // +0x2b80: when the last dodge started
    pub dodging: bool,           // +0x2bad
    pub dodge_flag: bool,        // +0x2b7c
    /// The clip set of the frame's traces (from `Settings::clip`).
    pub clip: usize,
    pub fall_speed: f32,         // +0x18a4
    pub step_height: f32,        // +0x18a8
    pub push_off: Vec3,          // +0x3d44
    /// The ground trace StayOnGround cached (`gm+0x1b0`, its end `gm+0x198`, valid `gm+0x220`).
    pub cached: Option<(Trace, Vec3)>,
    /// Game time (`gpGlobals->curtime`) of the frame being moved.
    pub now: f32,
}

impl Pm {
    pub fn new(origin: Vec3, yaw: f32) -> Self {
        Self {
            origin,
            vel: Vec3::ZERO,
            saved_vel: Vec3::ZERO,
            buttons: 0,
            old_buttons: 0,
            pressed: 0,
            fmove: 0.0,
            smove: 0.0,
            yaw,
            pitch: 0.0,
            fwd: Vec3::X,
            right: -Vec3::Y,
            up: Vec3::Z,
            fwd_flat: Vec3::X,
            right_flat: -Vec3::Y,
            wishdir: Vec3::ZERO,
            maxspeed: 0.0,
            summed_wish: Vec3::ZERO,
            flags: 0,
            ducktime: 0,
            duck_state: 0,
            duck_toggle: false,
            forced_slide: 0,
            sliding: false,
            boost_pending: false,
            slide_start: -100.0,
            boost_amount: 0.0,
            normal: Vec3::Z,
            prev_normal: Vec3::Z,
            ground_normal: Vec3::ZERO,
            block_normal: Vec3::ZERO,
            on_ground: false,
            time_left_ground: -100.0,
            leave_ground_time: -100.0,
            grace_normal: Vec3::Z,
            jump_time: -100.0,
            land_time: -100.0,
            jumped_since_leaving: false,
            wallrun_start_time: -100.0,
            wallrun_end_time: -100.0,
            wallrun_start_pos: Vec3::ZERO,
            wallrun_count: 0,
            no_boost: false,
            push_away_timer: 0.0,
            wallrun_friction_scale: 1.0,
            decel_scale: 1.0,
            retry_time: 0.0,
            retry_pos: Vec3::ZERO,
            retry_normal: Vec3::Z,
            hanging: false,
            hang_start: 0.0,
            hang_time: 0.0,
            last_walls: Vec::new(),
            last_wall_time: 0.0,
            sprinting: false,
            sprint_started_time: -100.0,
            sprint_started_frac: 0.0,
            sprint_ended_time: -100.0,
            sprint_ended_frac: 0.0,
            sticky_sprint_start: -100.0,
            sprint_window: 0.0,
            sprint_latch: false,
            zoom_toggle: false,
            zoomed: false,
            friction_factor: 1.0,
            move_scale: 1.0,
            impulse_time: 0.0,
            jumps_used: 0,
            jump_power: 100.0,
            dodge_power: 100.0,
            power_delay_end: 0.0,
            dodge_time: -100.0,
            dodging: false,
            dodge_flag: false,
            clip: CLIP_PILOT,
            fall_speed: 0.0,
            step_height: 18.0,
            push_off: Vec3::ZERO,
            cached: None,
            now: 0.0,
        }
    }

    /// GetSprintFrac (`0x1802c1550`), 0..1: sprinting, the lesser of the fast ramp since the
    /// sprint started (over sprintStartFastDuration) and the slow one from the latch's attempt
    /// plus sprintStartDelay (over sprintStartDuration), plus the fraction the sprint started
    /// at; otherwise the fraction the sprint ended at, ramping down over sprintEndDuration. The
    /// engine feeds it to the view (sprintViewOffset) and the viewmodel's `sprintfrac` pose
    /// parameter.
    pub fn sprint_frac(&self, s: &Settings) -> f32 {
        let ramp = |t: f32, dur: f32| if dur == 0.0 { 1.0 } else { (t / dur).clamp(0.0, 1.0) };
        if self.sprinting {
            let fast = ramp(self.now - self.sprint_started_time, s.sprint_start_fast_duration);
            let slow = ramp(self.now - (s.sprint_start_delay + self.sticky_sprint_start), s.sprint_start_duration);
            (fast.min(slow.max(0.0)) + self.sprint_started_frac).clamp(0.0, 1.0)
        } else {
            (self.sprint_ended_frac - ramp(self.now - self.sprint_ended_time, s.sprint_end_duration)).clamp(0.0, 1.0)
        }
    }
    /// IsWallRunning (`0x1802c9350`): the move's up vector is not straight up.
    pub fn is_wallrunning(&self) -> bool {
        self.normal.z < 1.0
    }
    /// FL_DUCKING: the hull is the ducked one.
    pub fn ducked_hull(&self) -> bool {
        self.flags & FL_DUCKING != 0
    }
    /// The current hull (`0x1801f02c0`): mins and maxs by FL_DUCKING.
    fn hull(&self, s: &Settings) -> (Vec3, Vec3) {
        if self.ducked_hull() {
            (s.duck_mins, s.duck_maxs)
        } else {
            (s.stand_mins, s.stand_maxs)
        }
    }
    /// `mv+0x7c`: the current hull's top; `mv+0x84`: its half width.
    fn hull_height(&self, s: &Settings) -> f32 {
        self.hull(s).1.z
    }
    fn hull_half_width(&self, s: &Settings) -> f32 {
        self.hull(s).1.x
    }
    /// Half of (standing size - ducked size) along each axis: the origin shift of an in-air
    /// duck / unduck (`0x1801f7290` / `0x1801f75e0`).
    fn duck_shift(s: &Settings) -> Vec3 {
        ((s.stand_maxs - s.stand_mins) - (s.duck_maxs - s.duck_mins)) * 0.5
    }
    /// GetMaxSpeed by pose (`0x1802c7030`): `speed` of the pose (the status / weapon
    /// multipliers are 1).
    fn pose_speed(&self, s: &Settings, pose: usize) -> f32 {
        s.pose_speed[pose]
    }
    /// `0x1802c70b0`: `sprintspeed` of the pose.
    fn pose_sprint_speed(&self, s: &Settings, pose: usize) -> f32 {
        s.pose_sprint_speed[pose]
    }
    /// GetPose (`0x1802c5ac0`): 1 while the hull is ducked, else 0.
    fn pose(&self) -> usize {
        if self.ducked_hull() {
            1
        } else {
            0
        }
    }
    /// GetMaxSpeed (`0x1802c6920`): sprintspeed while sprinting, else speed, of the stand pose
    /// (`player+0x20cc` / `+0x20d0`).
    fn get_max_speed(&self, s: &Settings) -> f32 {
        if self.sprinting {
            s.pose_sprint_speed[0]
        } else {
            s.pose_speed[0]
        }
    }
    /// The impulse (knockback) ramp of GetDeceleration and the wall-run friction
    /// (`player+0x2450`): 0 until half a second before it, then up to 1.
    fn impulse_ramp(&self) -> f32 {
        if self.impulse_time <= self.now {
            1.0
        } else {
            let t0 = self.impulse_time - 0.5;
            if self.now <= t0 {
                0.0
            } else {
                (self.now - t0) * 2.0
            }
        }
    }
    /// Wall-run strength by the number of wall-runs (`wallrun_strengthLossStart/End`), 0 with
    /// the no-boost flag: the up-wall boost, the wall jump's up speed and the gravity ramp.
    fn wallrun_strength_by_count(&self, cv: &Cvars) -> f32 {
        if self.no_boost {
            return 0.0;
        }
        let n = self.wallrun_count as f32;
        if n <= cv.wallrun_strength_loss_start {
            1.0
        } else if n < cv.wallrun_strength_loss_end {
            (n - cv.wallrun_strength_loss_start) / (cv.wallrun_strength_loss_end - cv.wallrun_strength_loss_start)
        } else {
            0.0
        }
    }
    /// The engine's hull trace (`0x1801fbdc0`) with the current hull.
    fn trace(&self, w: &CollisionWorld, s: &Settings, start: Vec3, end: Vec3) -> Trace {
        let (mins, maxs) = self.hull(s);
        w.trace_box(start, end, mins, maxs, self.clip)
    }
    /// SetGroundEntity (`0x1802abd50`) for the world: the handle and FL_ONGROUND, plus what
    /// the engine records when the ground is lost (`+0x2b90`, `+0x2ba0`: the grace jump).
    fn set_ground(&mut self, ground: bool) {
        if self.on_ground && !ground {
            self.leave_ground_time = self.now;
            self.grace_normal = self.normal;
        }
        self.on_ground = ground;
    }
}

/// The trace result's "hit an entity" (`tr+0x60 != 0`): the world counts.
#[inline]
fn tr_ent(t: &Trace) -> bool {
    t.fraction < 1.0 || t.start_solid
}
/// `tr+0x3a` allsolid: a trace that could not move at all.
#[inline]
fn tr_allsolid(t: &Trace) -> bool {
    t.start_solid && t.fraction <= 0.0
}

/// AngleVectors for the engine's view angles (pitch, yaw in degrees).
pub fn angle_vectors(pitch: f32, yaw: f32) -> (Vec3, Vec3, Vec3) {
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let fwd = Vec3::new(cp * cy, cp * sy, -sp);
    let right = Vec3::new(sy, -cy, 0.0);
    let up = Vec3::new(sp * cy, sp * sy, cp);
    (fwd, right, up)
}

/// PlayerMove (`0x1802041c0`) for move type 2 (walk) on the world: one client frame of `dt`
/// seconds at game time `now` with the inputs of `cmd`.
pub fn player_move(p: &mut Pm, cmd: &Cmd, s: &Settings, cv: &Cvars, w: &CollisionWorld, now: f32, dt: f32) {
    p.now = now;
    // The usercmd into the move data (CPrediction::SetupMove): buttons, pressed, view vectors
    // and the wish vector of the frame.
    p.old_buttons = p.buttons;
    p.buttons = cmd.buttons;
    p.pressed = cmd.buttons & !p.old_buttons;
    p.fmove = cmd.fmove;
    p.smove = cmd.smove;
    p.yaw = cmd.yaw;
    p.pitch = cmd.pitch;
    let (fwd, right, up) = angle_vectors(cmd.pitch, cmd.yaw);
    p.fwd = fwd;
    p.right = right;
    p.up = up;
    // SetupMove (0x1802d8b40): the wish is in the horizontal plane, from the flattened view
    // vectors, and never longer than 1.
    p.fwd_flat = norm(Vec3::new(fwd.x, fwd.y, 0.0));
    p.right_flat = norm(Vec3::new(right.x, right.y, 0.0));
    let mut wish = p.fwd_flat * cmd.fmove + p.right_flat * cmd.smove;
    if wish.length_squared() > 1.0 {
        wish = norm(wish);
    }
    p.wishdir = wish;
    p.summed_wish = Vec3::ZERO;
    p.step_height = s.step_height;
    p.clip = s.clip;
    update_power(p, s, dt);
    // 0x180204303: m_flDucktime counts down in whole milliseconds.
    if p.ducktime > 0 {
        p.ducktime -= (dt * 1000.0) as i32;
        if p.ducktime < 0 {
            p.ducktime = 0;
        }
    }
    // 0x1802044f8: with sv_optimizedmovement, no CategorizePosition before the move; a Pilot
    // moving up faster than 200 along the ground normal loses the ground entity.
    if cv.sv_optimizedmovement {
        if p.vel.dot(p.normal) > 200.0 {
            on_land(p, None, s, cv);
        }
    } else if !(p.on_ground && stay_on_ground(p, s, w)) {
        categorize_position(p, false, s, cv, w, dt);
    }
    // 0x1802046c4: fall speed while airborne.
    if !p.on_ground {
        p.fall_speed = DOWN.dot(p.vel);
    }
    duck(p, s, cv, w, dt);
    sprint_update(p, cmd, s);
    // 0x180204807: mv.maxspeed = GetMaxSpeed x status multiplier (1).
    p.maxspeed = p.get_max_speed(s);
    full_walk_move(p, s, cv, w, dt);
    p.saved_vel = p.vel;
}

/// FullWalkMove (`0x1801f93c0`).
fn full_walk_move(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    dodge_update(p, s);
    start_gravity(p, s, cv, dt);
    check_velocity(p, cv);
    check_jump_button(p, s, cv, w, dt);
    wallrun_update(p, s, cv, w, dt);
    // 0x1801fd7a0 (touching entities), 0x180207e00 (tether), 0x1801ff7e0 (triggers): nothing
    // without entities.
    check_velocity(p, cv);
    let old_vel = p.vel;
    if !p.on_ground {
        if p.is_wallrunning() {
            end_wallrun(p);
        }
        air_move(p, s, cv, w, dt);
    } else {
        walk_move(p, s, cv, w, dt);
    }
    let _ = old_vel; // 0x1801f45a0 impact: sounds / events only.
    categorize_position(p, false, s, cv, w, dt);
    finish_gravity(p, s, cv, dt);
    if p.on_ground {
        let vn = p.vel.dot(p.normal);
        p.vel -= p.normal * vn;
    }
    check_velocity(p, cv);
    // 0x1801f40b0: landing events; on the ground the fall speed is cleared.
    if p.on_ground {
        p.fall_speed = 0.0;
    }
}

/// Dodge bookkeeping (`0x1801f53e0`): the dodge ends `dodgeDuration` after it started, the
/// anim flag `dodgeAnimDuration` after; both end early below `dodgeStopSpeed`.
fn dodge_update(p: &mut Pm, s: &Settings) {
    if p.dodging {
        if p.now < p.dodge_time + s.dodge_duration {
            if p.vel.length_squared() < s.dodge_stop_speed * s.dodge_stop_speed {
                p.dodging = false;
                p.dodge_flag = false;
            }
        } else {
            p.dodging = false;
        }
    }
    if p.dodge_flag && (p.dodge_time + s.dodge_anim_duration <= p.now || p.vel.length_squared() < s.dodge_stop_speed * s.dodge_stop_speed) {
        p.dodge_flag = false;
    }
}

/// The dodge power regen (`0x1802ce140`, run before the move from `0x1802cad80`): once the
/// delay after a dodge is over, `powerRegenRate` per second (x 1 - the status effect
/// `0x180328f30`, x player+0x18c4, both 1) up to 100. (player+0x18b4 nonzero drains instead:
/// nothing here sets it.)
fn update_power(p: &mut Pm, s: &Settings, dt: f32) {
    if p.dodge_power < 100.0 && p.power_delay_end <= p.now {
        p.dodge_power = (p.dodge_power + s.power_regen_rate * dt).min(100.0);
    }
}

/// Half a tick of gravity (`0x1801f93c0` top / FinishGravity `0x1801f7410`): along DOWN, times
/// the wall-run gravity ramp; while sliding on the ground the same amount pulls down the slope.
fn half_gravity(p: &mut Pm, s: &Settings, cv: &Cvars, dt: f32) {
    let mut mult = 1.0;
    if p.is_wallrunning() {
        mult = gravity_ramp(p, s, cv, p.now - p.wallrun_start_time);
    }
    let g = s.gravity_scale * cv.sv_gravity * 0.5 * dt * mult;
    p.vel += DOWN * g;
    if p.sliding && p.on_ground {
        let n = p.ground_normal;
        p.vel.x += g * n.x * n.z;
        p.vel.y += g * n.y * n.z;
    }
}
fn start_gravity(p: &mut Pm, s: &Settings, cv: &Cvars, dt: f32) {
    half_gravity(p, s, cv, dt);
}
fn finish_gravity(p: &mut Pm, s: &Settings, cv: &Cvars, dt: f32) {
    half_gravity(p, s, cv, dt);
}

/// CheckVelocity (`0x1801f4370`): NaN components to 0, each component within sv_maxvelocity
/// (the whole vector scaled when one is over).
fn check_velocity(p: &mut Pm, cv: &Cvars) {
    let max = cv.sv_maxvelocity;
    for i in 0..3 {
        if p.vel[i].is_nan() {
            p.vel[i] = 0.0;
        }
        if p.origin[i].is_nan() {
            p.origin[i] = 0.0;
        }
        p.origin[i] = p.origin[i].clamp(-65536.0, 65536.0);
        let c = p.vel[i];
        if c > max {
            p.vel *= max / c;
        } else if -c > max {
            p.vel *= -(max / c);
        }
    }
}

/// Duck (`0x1801f5500`).
fn duck(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let _ = dt;
    let old = p.old_buttons;
    let cur = p.buttons;
    let was_ducking = p.duck_state == 1 || p.duck_state == 2;
    if (old ^ cur) & IN_DUCK == 0 {
        if p.pressed & IN_DUCKTOGGLE != 0 && !p.is_wallrunning() {
            p.duck_toggle = !p.duck_toggle;
        }
    } else {
        p.duck_toggle = false;
    }
    if p.forced_slide == 2 {
        p.duck_toggle = true;
    } else if p.forced_slide == 3 && p.on_ground {
        p.duck_toggle = true;
        if !p.sliding && p.on_ground && was_ducking {
            let n = p.ground_normal;
            start_slide(p, n, s, cv);
        }
    }
    let mut want = cur & IN_DUCK != 0 || p.duck_toggle;
    if p.sliding && s.slide_max_stop_speed * s.slide_max_stop_speed < p.vel.x * p.vel.x + p.vel.y * p.vel.y {
        want = true;
    }
    if p.forced_slide == 1 {
        want = false;
        p.duck_toggle = false;
    }
    if p.is_wallrunning() && p.forced_slide == 0 {
        want = false;
    }
    if !s.crouch {
        want = false;
    }
    if want {
        if !was_ducking {
            let n = p.ground_normal;
            start_slide(p, n, s, cv);
        }
        if p.duck_state == 0 {
            p.duck_state = 1;
            p.ducktime = if p.sliding { 200 } else { 400 };
        } else if p.duck_state == 3 {
            p.duck_state = 1;
            // 0x1801f8060: how far the unduck got (of its 200 ms) becomes the duck's time left.
            let frac = ((200 - p.ducktime) as f32 / 200.0).clamp(0.0, 1.0);
            p.ducktime = (frac * if p.sliding { 200.0 } else { 400.0 }) as i32;
        }
        if p.duck_state == 1 && p.ducktime < 1 {
            finish_duck(p, s, cv, w, dt);
        }
    } else if p.duck_state != 0 {
        if !can_unduck(p, s, w) {
            p.duck_state = 2;
            p.flags |= FL_DUCKING;
        } else {
            if p.duck_state == 2 {
                p.duck_state = 3;
                p.ducktime = 200;
            } else if p.duck_state == 1 {
                p.duck_state = 3;
                let frac = duck_fraction(p.ducktime, p.sliding);
                p.ducktime = ((1.0 - frac) * 200.0) as i32;
            }
            if p.duck_state == 3 && p.ducktime < 1 {
                finish_unduck(p, s, cv, w, dt);
            }
        }
    }
    if !(p.duck_state == 1 || p.duck_state == 2) && was_ducking {
        end_slide(p, cv);
    }
}

impl Pm {
    /// The view's crouch (0 standing, 1 ducked) from the duck state and its ms left: ducking
    /// counts 400 ms (200 when sliding) down, unducking 200 ms.
    pub fn duck_view(&self) -> f32 {
        match self.duck_state {
            1 => 1.0 - duck_fraction(self.ducktime, self.sliding),
            2 => 1.0,
            3 => (self.ducktime as f32 / 200.0).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// `0x1801f8020`: how far into the duck, by the ms left (400 ms, 200 ms when sliding).
fn duck_fraction(ms: i32, sliding: bool) -> f32 {
    let f = ms as f32 / if sliding { 200.0 } else { 400.0 };
    f.clamp(0.0, 1.0)
}

/// FinishDuck (`0x1801f7290`): the hull turns ducked; in the air the origin moves up by half
/// the height difference (the feet come up), then the hull is unstuck and the position
/// re-categorized.
fn finish_duck(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let was_ducked = p.ducked_hull();
    p.flags |= FL_DUCKING;
    p.duck_state = 2;
    if !p.on_ground && !was_ducked {
        p.origin += Pm::duck_shift(s);
    }
    set_hull_unstick(p, s, w, true);
    categorize_position(p, false, s, cv, w, dt);
}

/// FinishUnDuck (`0x1801f75e0`): the hull turns standing; in the air the origin moves down by
/// half the height difference.
fn finish_unduck(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let mut pos = p.origin;
    if !p.on_ground && p.ducked_hull() && (!UNDUCK_BY_BLOCK_NORMAL || p.block_normal.z <= 0.0) {
        let mut d = -Pm::duck_shift(s) * 2.0;
        if !UNDUCK_BY_BLOCK_NORMAL || p.block_normal.z == 0.0 {
            d *= 0.5;
        }
        pos += d;
    }
    p.flags &= !FL_DUCKING;
    p.duck_state = 0;
    p.origin = pos;
    categorize_position(p, false, s, cv, w, dt);
}

/// `0x1801f2cd0`: the standing hull fits where the unduck would put it.
fn can_unduck(p: &mut Pm, s: &Settings, w: &CollisionWorld) -> bool {
    let mut pos = p.origin;
    if !p.on_ground && p.ducked_hull() && (!UNDUCK_BY_BLOCK_NORMAL || p.block_normal.z <= 0.0) {
        let mut d = -Pm::duck_shift(s) * 2.0;
        if !UNDUCK_BY_BLOCK_NORMAL || p.block_normal.z == 0.0 {
            d *= 0.5;
        }
        pos += d;
    }
    let tr = w.trace_box(p.origin, pos, s.stand_mins, s.stand_maxs, p.clip);
    !tr.start_solid && tr.fraction == 1.0
}

/// `0x1801f77f0`: a hull stuck in the world is moved up a unit at a time (up to 36) until it
/// is free; if it never is, the origin is left where it was.
fn set_hull_unstick(p: &mut Pm, s: &Settings, w: &CollisionWorld, nudge: bool) {
    let tr = p.trace(w, s, p.origin, p.origin);
    if !(tr.start_solid && tr_ent(&tr)) {
        return;
    }
    let saved = p.origin;
    let f = if nudge { 1.0 } else { 0.0 };
    for _ in 0..36 {
        p.origin += -DOWN * f;
        let tr = p.trace(w, s, p.origin, p.origin);
        if !(tr.start_solid && tr_ent(&tr)) {
            return;
        }
    }
    p.origin = saved;
}

/// The sprint update (`0x1802c89a0`): the 3 s press window / 0.1 s hold window, the latch
/// (`0x1802caa60`) and CanSprint (`0x1802c2570`).
fn sprint_update(p: &mut Pm, cmd: &Cmd, s: &Settings) {
    let pressed = p.pressed & IN_SPEED != 0;
    let held = p.buttons & IN_SPEED != 0;
    if pressed && !p.zoomed {
        p.duck_toggle = false;
    }
    if pressed {
        p.sprint_window = p.sprint_window.max(p.now + 3.0);
    } else if held {
        p.sprint_window = p.sprint_window.max(p.now + 0.1);
    }
    if !p.sprint_latch {
        if p.now <= p.sprint_window && can_latch_sprint(p, cmd) {
            p.sprint_latch = true;
            p.sticky_sprint_start = p.now;
        }
    } else if !can_latch_sprint(p, cmd) {
        p.sprint_latch = false;
        p.sprint_window = 0.0;
    }
    let sprint = (held || p.sprint_latch) && can_sprint(p, cmd, s);
    if sprint != p.sprinting {
        // The fraction carries over: a sprint picks up from where the last one's ramp down got
        // to, and the ramp down starts from where the sprint got to.
        let frac = (p.sprint_frac(s) * 1023.0 + 0.5).floor() / 1023.0;
        if sprint {
            p.sprint_started_frac = frac;
            p.sprint_started_time = p.now;
        } else {
            p.sprint_ended_frac = frac;
            p.sprint_ended_time = p.now;
        }
    }
    p.sprinting = sprint;
}

/// `0x1802c9160`: the stick is at least sqrt(a) long and its forward share at least b.
fn stick_check(a: f32, b: f32, cmd: &Cmd) -> bool {
    let l2 = cmd.fmove * cmd.fmove + cmd.smove * cmd.smove;
    if a <= l2 {
        let f = if l2 != 0.0 { cmd.fmove / l2.sqrt() } else { 0.0 };
        return b <= f;
    }
    false
}

/// `0x1802caa60`: the latch takes a stick of at least 0.8 pointing no further back than 135
/// degrees, with no zoom, while not ducking or ducked.
fn can_latch_sprint(p: &Pm, cmd: &Cmd) -> bool {
    if !stick_check(0.64, -0.707, cmd) {
        return false;
    }
    if p.zoomed || p.buttons & (IN_ZOOM | IN_ZOOM_TOGGLE) != 0 {
        return false;
    }
    !(p.duck_state == 1 || p.duck_state == 2)
}

/// CanSprint (`0x1802c2570`).
fn can_sprint(p: &Pm, cmd: &Cmd, s: &Settings) -> bool {
    if !s.sprint {
        return false;
    }
    if !p.on_ground && 0.25 < p.now - p.time_left_ground {
        return false;
    }
    if p.zoom_toggle || p.pressed & IN_ZOOM_TOGGLE != 0 || p.buttons & IN_ZOOM != 0 {
        return false;
    }
    if !(p.normal.z >= 1.0) {
        return false;
    }
    if !stick_check(0.64, 0.707, cmd) {
        return false;
    }
    if p.duck_state == 1 || p.duck_state == 2 {
        return false;
    }
    let (sy, cy) = cmd.yaw.to_radians().sin_cos();
    0.0 < p.vel.x * cy + p.vel.y * sy
}

/// CheckJumpButton (`0x180200200`). Returns true when a jump happened.
fn check_jump_button(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) -> bool {
    let _ = w;
    if p.pressed & IN_JUMP == 0 {
        p.old_buttons &= !IN_JUMP;
        if p.pressed & IN_DODGE == 0 {
            return false;
        }
    }
    let dodge_press = p.pressed & IN_JUMP == 0;
    if p.duck_toggle {
        p.duck_toggle = false;
        if p.on_ground && !p.sliding {
            return false;
        }
    }
    let mut up = p.normal;
    let ground_jump = if !p.on_ground {
        if cv.jump_graceperiod <= p.now - p.leave_ground_time || p.jumped_since_leaving {
            false
        } else {
            up = p.grace_normal;
            true
        }
    } else {
        true
    };
    let mut double = false;
    let mut dodge = dodge_press;
    if !ground_jump && !dodge {
        if !s.superjump {
            dodge = true;
        } else {
            double = true;
        }
    }
    if p.duck_state == 3 && !p.sliding {
        return false;
    }
    if double {
        if s.superjump_limit == 0.0 {
            return false;
        }
        // 0x1802c2760: power left and jumps left.
        if !(cv.superjump_min_power_use <= p.jump_power && (p.jumps_used as f32) < s.superjump_limit) {
            p.old_buttons |= IN_JUMP;
            return false;
        }
    } else if dodge {
        // The dodge gate: standing (duck state 0), `dodge` enabled (player+0x28f4 & 0x10 is a
        // flag nothing here sets), `dodgeInterval` since the last one, the power for it, and
        // some input unless dodge_no_movement_forward_enable.
        if p.duck_state != 0 || !s.dodge {
            return false;
        }
        if p.now < p.dodge_time + s.dodge_interval || p.dodge_power < s.dodge_power_drain {
            return false;
        }
        if !cv.dodge_no_movement_forward_enable && p.wishdir.length_squared() <= 1e-7 {
            return false;
        }
        // 0x1802cdfc0: the drain; 0x1802cdfa0: the regen delay (x player+0x18c8, 1) never
        // shortens one already pending.
        p.dodge_power = (p.dodge_power - s.dodge_power_drain).max(0.0);
        p.power_delay_end = p.power_delay_end.max(p.now + s.dodge_power_delay);
    } else {
        if !s.jump {
            return false;
        }
        p.time_left_ground = 0.0;
    }
    on_land(p, None, s, cv);
    let mult = 1.0; // status effect jump multiplier (player+0x2f48 -> +0x40)
    if p.sliding {
        let h2 = p.vel.x * p.vel.x + p.vel.y * p.vel.y;
        if p.boost_pending && 0.0 < h2 {
            let sp = h2.sqrt();
            let mut sp2 = sp;
            if p.duck_state != 2 && p.now - 0.4 < p.slide_start {
                sp2 = (sp - p.boost_amount).max(0.0);
                p.vel *= sp2 / sp;
            }
            let m = s.slide_max_jump_speed;
            if m * 0.85 * m * 0.85 < h2 && h2 < m * m {
                p.vel *= m / sp2;
            }
            p.boost_pending = false;
        }
    }
    let wall = !(up.z >= 1.0) && !dodge;
    let height;
    let mut vertical = false;
    if double {
        let f = 0.0; // 0x1802c7720: the double-jump power fraction is 0
        let h = s.superjump_min_height + (s.superjump_max_height - s.superjump_min_height) * f;
        let vz = p.vel.z;
        let cur = if vz >= 0.0 { vz * vz / (cv.sv_gravity * 2.0) } else { 0.0 };
        let min_h = h * cv.superjump_min_height_fraction;
        if min_h + cur < h {
            p.vel.z = 0.0;
            height = h;
        } else {
            height = min_h;
        }
        let use_power = cv.superjump_min_power_use + (cv.superjump_max_power_use - cv.superjump_min_power_use) * f;
        p.jump_power = (p.jump_power - use_power).max(0.0);
        p.jumps_used += 1;
    } else if dodge {
        // `dodgeHeight`, or `dodgeVerticalHeight` for a vertical dodge (dodge_vertical_enable
        // with the stick under dodge_vertical_threshold; off).
        let th = cv.dodge_vertical_threshold;
        if !cv.dodge_vertical_enable || th * th <= p.wishdir.length_squared() {
            height = s.dodge_height;
        } else {
            vertical = true;
            height = s.dodge_vertical_height;
        }
    } else {
        let mut h = if p.sliding { s.slide_jump_height } else { s.jump_height };
        if p.now - p.land_time < cv.skip_time && !p.is_wallrunning() {
            h *= cv.skip_jump_height_fraction;
            let r = cv.skip_speed_retain;
            let h2 = p.vel.x * p.vel.x + p.vel.y * p.vel.y;
            if r * r < h2 {
                let sp = h2.sqrt();
                let keep = r.max(sp - cv.skip_speed_reduce);
                p.vel.x *= keep / sp;
                p.vel.y *= keep / sp;
            }
        }
        height = h;
    }
    let jump_speed = (cv.sv_gravity * 2.0 * height).sqrt();
    p.jumped_since_leaving = true;
    if wall {
        wall_jump(p, up, s, cv);
    } else {
        p.wallrun_count = 0;
        p.no_boost = false;
        if cv.dodge_vertical_in_air || !dodge || ground_jump {
            p.vel += up * (mult * jump_speed);
        }
        if p.wishdir.dot(p.block_normal) < 0.0 {
            p.vel -= p.block_normal * 30.0;
        }
        if double || dodge {
            // The horizontal push: the double jump's along the wish as it is, the dodge's along
            // the normalized wish, or the eye's forward (x, y) with no input
            // (dodge_no_movement_forward_enable); none for a vertical dodge. The old speed
            // keeps what exceeds the push (x dodge_vertical_horzspeedscale when vertical), plus
            // `dodgeKeepSpeedFrac` of the rest for a dodge.
            let mut dir = p.wishdir;
            if vertical {
                dir = Vec3::ZERO;
            } else if dodge {
                if !cv.dodge_no_movement_forward_enable || 1e-7 < dir.length_squared() {
                    dir = norm(dir);
                } else {
                    dir = p.fwd;
                }
            }
            // 0x1802c6720: dodgeSpeed x (1 - the status effect 0x180328f70, 0).
            let speed = if double { s.superjump_horz_speed } else { s.dodge_speed };
            let add = Vec3::new(dir.x, dir.y, 0.0) * speed;
            let cur = (p.vel.x * p.vel.x + p.vel.y * p.vel.y).sqrt();
            let a = (add.x * add.x + add.y * add.y).sqrt();
            let mut keep = 0.0;
            if a < cur {
                keep = 1.0 - a / cur;
            }
            if vertical {
                keep *= cv.dodge_vertical_horzspeedscale;
            }
            if dodge {
                keep += (1.0 - keep) * s.dodge_keep_speed_frac;
            }
            p.vel.x = keep * p.vel.x + add.x;
            p.vel.y = keep * p.vel.y + add.y;
            if dodge {
                p.dodge_time = p.now;
                p.dodging = true;
                p.dodge_flag = true;
            }
        }
        // (0x18020efe0, the jump's anim / event call, is skipped for a dodge.)
    }
    end_slide(p, cv);
    finish_gravity(p, s, cv, dt);
    check_velocity(p, cv);
    p.jump_time = p.now;
    p.old_buttons |= IN_JUMP;
    true
}

/// The wall jump (`0x180201170`) off the wall with normal `n`.
fn wall_jump(p: &mut Pm, n: Vec3, s: &Settings, cv: &Cvars) {
    let mut wd = p.wishdir;
    if p.hanging {
        p.zoom_toggle = false;
        let l = wd.length();
        if l < 1.0 {
            wd += p.fwd * (1.0 - l);
        }
    }
    let mut add = wd * s.wallrun_jump_input_dir_speed;
    let a = n.dot(p.fwd_flat).min(0.0);
    let b = n.dot(wd).min(0.0);
    let out = (1.0 - a * b * 0.8) * s.wallrun_jump_outward_speed;
    let d = add.dot(n);
    if d < out {
        add += n * (out - d);
    }
    remember_wall(p, n, cv);
    p.vel += add;
    p.no_boost = false;
    let up = p.wallrun_strength_by_count(cv) * s.wallrun_jump_up_speed;
    if p.vel.z < up {
        p.vel.z = (p.vel.z + up * 1.5).min(up);
    }
}

/// `0x1801f0c60`: the wall jumped off is remembered (flattened normal) for the same-wall rule.
fn remember_wall(p: &mut Pm, n: Vec3, cv: &Cvars) {
    if cv.wallrun_repel_enable {
        let flat = norm(Vec3::new(n.x, n.y, 0.0));
        p.last_walls.clear();
        p.last_walls.push(flat);
        p.last_wall_time = p.now;
    }
}

/// OnLand / the SetGroundEntity wrapper (`0x1801fa160`) with the ground trace or none.
fn on_land(p: &mut Pm, tr: Option<&Trace>, s: &Settings, cv: &Cvars) {
    let new_ground = tr.is_some_and(tr_ent);
    if p.on_ground != new_ground {
        if new_ground {
            if !p.on_ground {
                p.land_time = p.now;
            }
            if p.duck_state == 1 || p.duck_state == 2 {
                let n = tr.unwrap().normal;
                start_slide(p, n, s, cv);
            }
        }
        p.set_ground(new_ground);
    }
    if new_ground {
        if cv.skip_replenish_double_jump || p.now - p.land_time > cv.skip_time || p.is_wallrunning() {
            replenish(p, s, cv);
        }
        let vn = p.vel.dot(p.normal);
        p.vel -= p.normal * vn;
    }
}

/// `0x1802ce340`: touching the ground (or a wall) gives the jumps back.
fn replenish(p: &mut Pm, s: &Settings, cv: &Cvars) {
    p.jumped_since_leaving = false;
    p.retry_time = 0.0;
    if p.normal.z < 1.0 {
        if !s.superjump_after_wallrun {
            p.jumps_used = s.superjump_limit as i32;
        } else if cv.superjump_limitreset_onwallrun {
            p.jumps_used = 0;
        }
    } else {
        p.last_walls.clear();
        p.jumps_used = 0;
        p.no_boost = true;
        if cv.superjump_powerreset_onground {
            p.jump_power = 100.0;
        }
    }
}

/// StartSlide (`0x18020af70`) on the plane with normal `n`.
fn start_slide(p: &mut Pm, n: Vec3, s: &Settings, cv: &Cvars) {
    if !(n.z >= 0.7 || cv.slide_while_in_air) {
        return;
    }
    let h2;
    if p.forced_slide == 3 {
        h2 = (p.vel.x * p.vel.x + p.vel.y * p.vel.y).max(0.01);
    } else {
        if !s.slide {
            return;
        }
        h2 = p.vel.x * p.vel.x + p.vel.y * p.vel.y;
        if h2 < s.slide_required_start_speed * s.slide_required_start_speed {
            return;
        }
        if p.on_ground && p.fwd_flat.dot(p.wishdir) < cv.slide_max_angle_dot {
            return;
        }
    }
    p.sliding = true;
    p.flags |= FL_DUCKING;
    if p.now - p.slide_start > cv.slide_boost_cooldown {
        let sp = h2.sqrt();
        let boosted = (sp + s.slide_speed_boost).min(s.slide_speed_boost_cap);
        if boosted <= sp {
            p.boost_amount = 0.0;
        } else {
            p.boost_amount = boosted - sp;
            p.vel.x *= boosted / sp;
            p.vel.y *= boosted / sp;
        }
        p.boost_pending = true;
    }
    p.slide_start = p.now;
}

/// EndSlide (`0x1801f5a20`).
fn end_slide(p: &mut Pm, cv: &Cvars) {
    if p.sliding {
        p.sliding = false;
        if cv.slide_auto_stand {
            p.duck_toggle = false;
        }
    }
}

/// GetDeceleration (`0x1801fcf10`).
fn get_deceleration(p: &Pm, s: &Settings) -> f32 {
    let pose = p.pose();
    let mut d = -1.0;
    let sp = p.pose_speed(s, pose);
    if sp * sp < p.vel.length_squared() {
        if s.pose_sprint_decel[pose] >= 0.0 {
            d = s.pose_sprint_decel[pose];
        } else if s.pose_sprint_accel[pose] >= 0.0 {
            d = s.pose_sprint_accel[pose];
        }
    }
    let mut ramp_only = false;
    if p.sliding {
        if p.forced_slide == 3 {
            d = 0.0;
            ramp_only = true;
        } else if (!p.duck_toggle && p.buttons & IN_DUCK == 0) || p.wishdir.dot(p.vel) < 0.0 {
            d = s.slide_want_to_stop_decel;
        } else {
            d = s.slide_decel;
        }
    }
    if !ramp_only && d < 0.0 {
        d = s.pose_decel[pose];
        if d < 0.0 {
            d = s.pose_accel[pose] * 0.6;
        }
    }
    p.impulse_ramp() * d * p.move_scale
}

/// GetAcceleration (`0x1801fcbd0`).
fn get_acceleration(p: &Pm, s: &Settings) -> f32 {
    let pose = p.pose();
    let h2 = p.vel.x * p.vel.x + p.vel.y * p.vel.y;
    let a = if !p.sliding {
        let sp = p.pose_speed(s, pose);
        if s.pose_sprint_accel[pose] < 0.0 || h2 <= (sp - 1.0) * (sp - 1.0) {
            let low = s.pose_low_speed[pose];
            if low * low <= h2 || s.pose_low_accel[pose] < 0.0 {
                s.pose_accel[pose]
            } else {
                s.pose_low_accel[pose]
            }
        } else {
            s.pose_sprint_accel[pose]
        }
    } else {
        s.slide_accel
    };
    a * p.move_scale
}

/// Friction (`0x1801f8090`): the velocity along the wish is kept (reflected off the block
/// normal when the wish pushes into it); the rest loses GetDeceleration x dt, linearly, and
/// while sliding decays by slideVelocityDecay^dt as well.
fn friction(p: &mut Pm, wishdir: Vec3, wishspeed: f32, s: &Settings, dt: f32) {
    let v = p.vel;
    let wv = wishdir.dot(v);
    let (rest, keep);
    if wv <= 0.0 {
        rest = v;
        keep = Vec3::ZERO;
    } else {
        let k = (wv * wishspeed / v.length_squared()).min(1.0);
        if wishdir.dot(p.block_normal) < 0.0 {
            let bn = p.block_normal;
            let wd = norm(wishdir + bn * (0.0078125 - bn.dot(wishdir)));
            let kv = wd.dot(v).max(0.0) * k;
            keep = wd * kv;
        } else {
            keep = wishdir * (k * wv);
        }
        rest = v - keep;
    }
    let sp = rest.length();
    let decel = get_deceleration(p, s);
    let mut nsp = (sp - decel * p.friction_factor * p.decel_scale * dt).max(0.0);
    if p.sliding {
        nsp *= s.slide_velocity_decay.powf(dt);
    }
    if nsp < sp {
        let f = nsp / sp;
        p.vel = rest * f + keep;
        p.summed_wish -= rest * ((1.0 - f) * f);
    }
}

/// Accelerate (`0x1801f0ac0`).
fn accelerate(p: &mut Pm, wishdir: Vec3, wishspeed: f32, accel: f32, dt: f32) {
    let cap2 = p.vel.length_squared().max(wishspeed * wishspeed);
    let add = wishspeed - wishdir.dot(p.vel);
    if add > 0.0 {
        let step = (accel * dt * p.friction_factor).min(add);
        p.vel += wishdir * step;
        let l2 = p.vel.length_squared();
        if cap2 < l2 {
            p.vel *= (cap2 / l2).sqrt();
        }
    }
}

/// AirAccelerate (`0x1801f19a0`).
fn air_accelerate(p: &mut Pm, wishdir: Vec3, wishspeed: f32, accel: f32, cv: &Cvars, dt: f32) {
    let add = wishspeed - wishdir.dot(p.vel);
    if add > 0.0 {
        let step = (accel * dt).min(add);
        p.vel += wishdir * step;
        p.summed_wish += wishdir * step;
    } else {
        let cap2 = p.vel.length_squared().max(wishspeed * wishspeed);
        p.vel += wishdir * (dt * cv.player_extraairaccelleration);
        let l2 = p.vel.length_squared();
        if cap2 < l2 {
            p.vel *= (cap2 / l2).sqrt();
        }
    }
}

/// WalkMove (`0x18020cc30`): on the ground and on walls.
fn walk_move(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let n = p.normal;
    let vn = p.vel.dot(n);
    p.vel -= n * vn;
    p.fall_speed = 0.0;
    let old_vel = p.vel;
    let (fwd, right, _up) = angle_vectors(p.pitch, p.yaw);
    // The move axes: f (forward key) and r (side key) in the plane of the normal.
    let (f, r);
    if !p.is_wallrunning() || right.dot(n).abs() >= 0.7 {
        let rr = norm(fwd.cross(n));
        r = rr;
        f = n.cross(rr);
    } else {
        let ff = norm(n.cross(right));
        f = ff;
        r = ff.cross(n);
    }
    let mut fmove = p.fmove;
    let mut smove = p.smove;
    if !p.is_wallrunning() {
        if p.sprinting {
            // 0x1802c7600: sprint input scale 0.8.
            fmove *= 1.0 / 0.8;
            smove *= 1.0 / 0.8;
        }
        let mut cap = 1.0;
        if p.duck_state == 1 || p.duck_state == 2 {
            let s1 = p.pose_speed(s, 1);
            let s0 = p.pose_speed(s, 0);
            if s0.abs() <= 0.001 {
                if s1 == 0.0 {
                    cap = 0.0;
                }
            } else {
                cap = s1 / s0;
            }
        }
        let l2 = fmove * fmove + smove * smove;
        if l2 <= cap {
            if l2 > 0.0 {
                let l = l2.sqrt();
                let k = ((2.0 - l) * l * cap) / l;
                fmove *= k;
                smove *= k;
            }
        } else {
            let k = cap / l2.sqrt();
            fmove *= k;
            smove *= k;
        }
        smove *= s.speed_scale_side;
        if fmove < 0.0 {
            fmove *= s.speed_scale_back;
        }
    } else {
        // Along the wall: forward input also pushes up the wall (wallrun_upwardAutoPush), and
        // the side axis never points down it.
        let h = (n.x * n.x + n.y * n.y).sqrt().max(NORM_EPS);
        let fm = fmove.max(0.0);
        let d = (f.y * -n.x + f.x * n.y) / h;
        let a = (d * smove).max(0.0);
        smove += fm * d * cv.wallrun_upward_auto_push;
        fmove += a * fmove;
        if (r.z < -0.01 && smove > 0.0) || (r.z > 0.01 && smove < 0.0) {
            smove = 0.0;
        }
    }
    let mut wish = f * fmove + r * smove;
    let wn = wish.dot(n);
    wish -= n * wn;
    if !p.is_wallrunning() {
        // 0x1802066c0 repel from other players, pushaway: nothing here.
        if p.sliding {
            let wv = wish.dot(p.vel);
            if wv > 0.0 || (wv < 0.0 && p.forced_slide == 3) {
                wish -= p.vel * (wv / p.vel.length_squared());
            }
        }
        let l = wish.length();
        let wishdir = wish * (1.0 / l.max(NORM_EPS));
        let wishspeed = l.min(1.0) * p.maxspeed;
        let accel = get_acceleration(p, s);
        friction(p, wishdir, wishspeed, s, dt);
        accelerate(p, wishdir, wishspeed, accel, dt);
        p.summed_wish += wishdir * wishspeed;
    } else {
        let strength = wallrun_strength(p, cv);
        let slip;
        if !p.hanging {
            let mut sl = ((p.now - p.wallrun_start_time - cv.wallrun_slip_start_time) / cv.wallrun_slip_duration).clamp(0.0, 1.0);
            if fmove.abs() < 0.1 && smove.abs() < 0.1 && sl <= cv.wallrun_no_input_slip_frac {
                sl = cv.wallrun_no_input_slip_frac;
            }
            slip = sl;
        } else {
            slip = ((p.hang_time - cv.wallrun_hang_slip_start_time) / cv.wallrun_hang_slip_duration).clamp(0.0, 1.0);
        }
        let sp = p.vel.length();
        if sp > 0.0 {
            let mut fr = p.wallrun_friction_scale * s.wallrun_friction * p.friction_factor * p.impulse_ramp();
            if s.wallstick {
                fr = 10.0;
            }
            let nsp = ((sp - sp * dt * fr) * strength).max(0.0);
            let vz = p.vel.z;
            p.vel *= nsp / sp;
            if slip != 0.0 && vz < 0.0 {
                let cur = p.vel.z;
                p.vel.z = (vz - cur) * slip + cur;
            }
        }
        if wish.z.abs() > 0.0001 {
            let m = wish.z.abs().min(1.0);
            let wd = Vec3::new(0.0, 0.0, wish.z.signum());
            let ws = if !s.wallstick { m * s.wallrun_max_speed_vertical * strength } else { 0.0 };
            accelerate(p, wd, ws, (1.0 - slip) * s.wallrun_accel_vertical, dt);
            p.summed_wish.z += wd.z * ws;
        }
        let h2 = wish.x * wish.x + wish.y * wish.y;
        if h2 > 0.0001 {
            let hl = h2.sqrt();
            let hd = Vec3::new(wish.x, wish.y, 0.0) * (1.0 / hl.max(NORM_EPS));
            let m = hl.min(1.0);
            let ws = if !s.wallstick {
                if hd.x * p.fwd.x + hd.y * p.fwd.y <= -0.6 {
                    m * s.wallrun_max_speed_horizontal_backward
                } else {
                    m * s.wallrun_max_speed_horizontal
                }
            } else {
                0.0
            } * strength;
            accelerate(p, hd, ws, s.wallrun_accel_horizontal * p.move_scale, dt);
            p.summed_wish.x += hd.x * ws;
            p.summed_wish.y += hd.y * ws;
        }
    }
    let sp = p.vel.length();
    let _ = old_vel;
    if p.sliding && sp < s.slide_stop_speed && p.forced_slide != 3 {
        end_slide(p, cv);
    }
    if sp < 1.0 {
        p.vel = Vec3::ZERO;
        return;
    }
    // The move: along the ground plane (0x3b7c) at the same speed, not below it.
    let mut v = p.vel;
    let gn = p.ground_normal;
    if !p.is_wallrunning() && gn.length_squared() != 0.0 {
        let vg = gn.dot(v);
        let ng = n.dot(gn);
        let vp = v * ng - n * vg;
        let k = (v.length_squared() / vp.length_squared()).min(f32::MAX);
        v = vp * k.sqrt();
    }
    let mut end = p.origin + v * dt;
    {
        let d = gn.dot(end - p.origin);
        if d < 0.0 {
            end -= gn * (d - 0.001);
        }
    }
    p.block_normal = Vec3::ZERO;
    let tr = p.trace(w, s, p.origin, end);
    if tr.start_solid && tr.normal.abs().max_element() != 0.0 {
        let d = tr.end - p.origin;
        let k = d.dot(tr.normal).clamp(-1.0, 0.0);
        p.origin = d + d * k + p.origin;
        stay_on_ground(p, s, w);
        return;
    }
    if tr.fraction == 1.0 {
        p.origin = tr.end;
    } else {
        let v0 = p.vel;
        if !blocked_handler(p, &tr, true, None, false, s, cv, w, dt) {
            p.block_normal = tr.normal;
            try_start_wallrun(p, tr.end, tr.normal, Some(&tr), v0, s, cv, w, dt);
        }
    }
    stay_on_ground(p, s, w);
}

/// AirMove (`0x1801f1bd0`).
fn air_move(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let wd = p.wishdir;
    let l = wd.length();
    let wishdir = wd * (1.0 / l.max(NORM_EPS));
    let m = l.min(1.0);
    let air_speed = if s.air_strafe { 0.0 } else { s.air_speed };
    air_accelerate(p, wishdir, m * air_speed, s.air_accel * p.move_scale, cv, dt);
    // The keyboard grace (0x1801f1d62): a movement key pressed within
    // jump_keyboardgraceperiodmax of the jump turns the horizontal velocity toward the keys.
    if p.pressed & (IN_FORWARD | IN_BACK | IN_MOVELEFT | IN_MOVERIGHT) != 0 {
        let t = p.now - p.jump_time;
        if t < cv.jump_keyboardgraceperiodmax {
            let f = (p.buttons & IN_FORWARD != 0) as i32 as f32 - (p.buttons & IN_BACK != 0) as i32 as f32;
            let sd = (p.buttons & IN_MOVERIGHT != 0) as i32 as f32 - (p.buttons & IN_MOVELEFT != 0) as i32 as f32;
            let dir = p.fwd_flat * f + p.right_flat * sd;
            let dl = dir.length().max(NORM_EPS);
            let hs = (p.vel.x * p.vel.x + p.vel.y * p.vel.y).sqrt();
            let hl = hs.max(NORM_EPS);
            let mut hx = p.vel.x / hl;
            let mut hy = p.vel.y / hl;
            // Full strength until jump_keyboardgraceperiodmin, fading out to the max.
            let k = ((t - cv.jump_keyboardgraceperiodmax) / (cv.jump_keyboardgraceperiodmin - cv.jump_keyboardgraceperiodmax)).clamp(0.0, 1.0)
                * cv.jump_keyboardgrace_strength;
            hx += (dir.x / dl - hx) * k;
            hy += (dir.y / dl - hy) * k;
            let nl = (hx * hx + hy * hy).sqrt();
            if nl > 0.01 {
                hx *= hs / nl;
                hy *= hs / nl;
            }
            let mut dx = hx - p.vel.x;
            let mut dy = hy - p.vel.y;
            let cap = p.pose_sprint_speed(s, 0) * cv.jump_keyboardgrace_max;
            let d2 = dx * dx + dy * dy;
            if cap * cap < d2 {
                let dl = d2.sqrt();
                dx *= cap / dl;
                dy *= cap / dl;
            }
            p.vel.x += dx;
            p.vel.y += dy;
        }
    }
    p.vel += p.push_off * dt;
    let end = p.origin + p.vel * dt;
    p.block_normal = Vec3::ZERO;
    let tr = p.trace(w, s, p.origin, end);
    if tr.fraction == 1.0 {
        p.origin = tr.end;
        // A wall-run start that failed only on the probes is retried (wallrun_retry_interval).
        let retry = p.retry_time;
        if retry != 0.0 && p.now >= retry {
            p.retry_time = 0.0;
            if (p.origin - p.retry_pos).dot(p.retry_normal).abs() < 1.0 {
                let rn = p.retry_normal;
                let o = p.origin;
                try_start_wallrun(p, o, rn, None, -rn, s, cv, w, dt);
            }
        }
        return;
    }
    let v = p.vel;
    let slow_fall = !(v.z >= 0.0 || v.x * v.x + v.y * v.y >= 90000.0 || tr.normal.z >= 0.7 || p.now - p.time_left_ground <= 0.1 || p.dodge_flag);
    let mut handled = false;
    if !slow_fall {
        handled = blocked_handler(p, &tr, false, None, false, s, cv, w, dt);
    } else {
        let mut use_handler = false;
        if tr.normal.z > 0.0 {
            let f = (0.0078125 - tr.normal.dot(v)) * tr.normal.z + v.z;
            if f >= 0.0 {
                use_handler = true;
            }
        } else {
            let down = tr.end - p.normal * p.step_height;
            let t2 = p.trace(w, s, tr.end, down);
            if t2.fraction < 1.0 && t2.normal.z >= 0.7 {
                use_handler = true;
            }
        }
        if use_handler {
            handled = blocked_handler(p, &tr, false, None, false, s, cv, w, dt);
        } else {
            let mut o = p.origin;
            let mut vv = p.vel;
            try_player_move(p, &mut o, &mut vv, dt, Some(&tr), s, w);
            p.origin = o;
            p.vel = vv;
        }
    }
    if handled {
        return;
    }
    p.block_normal = tr.normal;
    try_start_wallrun(p, tr.end, tr.normal, Some(&tr), v, s, cv, w, dt);
}

/// CategorizePosition (`0x1801f3160`), `force` is its second argument.
fn categorize_position(p: &mut Pm, force: bool, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    p.friction_factor = 1.0;
    p.push_off = Vec3::ZERO;
    let on_ground_or_wall = p.on_ground || p.is_wallrunning();
    let dist = if on_ground_or_wall { p.step_height } else { 2.0 };
    let mut on_ground_or_wall = on_ground_or_wall;
    let start = p.origin;
    let end = p.origin - p.normal * dist;
    let vn = p.vel.dot(p.normal);
    let moving_up = (140.0 < vn && !p.is_wallrunning()) || (force && (!p.is_wallrunning() || 1.0 < vn));
    if moving_up || p.dodging {
        on_land(p, None, s, cv);
        finish_categorize(p, None);
        return;
    }
    let mut tr = match p.cached {
        Some((t, e)) if (end - e).length_squared() < 4.0 => t,
        _ => p.trace(w, s, start, end),
    };
    let mut tn = tr.normal.dot(p.normal);
    let mut got_ground = true;
    let hit = tr_ent(&tr);
    let mut lose_ground = false;
    if !hit {
        lose_ground = ground_recheck(p, &mut tr, &mut tn, &mut got_ground, s, w);
    } else if tn < 0.7 && !p.is_wallrunning() {
        got_ground = false;
        lose_ground = ground_recheck(p, &mut tr, &mut tn, &mut got_ground, s, w);
    } else {
        // A standable surface (the world): its velocity is 0, so the 140 u/s relative check
        // (0x1801f3bce) never skips the landing.
    }
    if lose_ground {
        if (!got_ground || tr_ent(&tr)) && (p.on_ground || p.now - p.time_left_ground < 0.1) {
            p.time_left_ground = p.now;
        }
        on_land(p, None, s, cv);
        if 0.0 < vn {
            p.friction_factor = 0.25;
        }
        on_ground_or_wall = false;
    } else {
        on_land(p, Some(&tr), s, cv);
        if p.is_wallrunning() && tn < 0.999 {
            on_ground_or_wall = cv.wallrun_angle_change_min_cos <= tn && on_ground_or_wall;
            if tr.normal.z < 0.7 && -0.35 < tr.normal.z {
                turn_normal(p, tr.normal, cv, dt);
            }
        }
        // 0x1801f2fd0: the surface's friction (default surfaces give 1).
        p.friction_factor = (p.friction_factor * 1.25).min(1.0);
    }
    if on_ground_or_wall && !tr_allsolid(&tr) && 0.0 < tr.fraction && tr.fraction < 1.0 && dt != 0.0 {
        p.origin = tr.end;
    }
    finish_categorize(p, Some(&tr));
}

/// The ground re-check of CategorizePosition (`0x1801f391d`): on the ground with the trace
/// short, the quarter-hull ground trace (`0x1802081b0`) looks for a standable plane. Returns
/// true when the Pilot is not on something standable.
fn ground_recheck(p: &mut Pm, tr: &mut Trace, tn: &mut f32, got_ground: &mut bool, s: &Settings, w: &CollisionWorld) -> bool {
    let _ = got_ground;
    if p.on_ground && tr.fraction < 1.0 {
        let end2 = tr.end - p.normal * 2.0;
        let (mins, maxs) = p.hull(s);
        *tr = ground_trace(w, p.origin, end2, mins, maxs, *tr, p.clip);
        *tn = tr.normal.dot(p.normal);
    }
    !tr_ent(tr) || (*tn < 0.7 && !p.is_wallrunning())
}

/// `0x1802081b0`: the trace with the hull's -x-y, +x+y, -x+y and +x-y quarters in turn until
/// one hits a plane within 0.7 of up; the result keeps the original end point and fraction.
fn ground_trace(w: &CollisionWorld, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, orig: Trace, clip: usize) -> Trace {
    let quarters = [
        (Vec3::new(mins.x, mins.y, mins.z), Vec3::new(maxs.x.min(0.0), maxs.y.min(0.0), maxs.z)),
        (Vec3::new(mins.x.max(0.0), mins.y.max(0.0), mins.z), Vec3::new(maxs.x, maxs.y, maxs.z)),
        (Vec3::new(mins.x, mins.y.max(0.0), mins.z), Vec3::new(maxs.x.min(0.0), maxs.y, maxs.z)),
        (Vec3::new(mins.x.max(0.0), mins.y, mins.z), Vec3::new(maxs.x, maxs.y.min(0.0), maxs.z)),
    ];
    let mut tr = orig;
    for (qmins, qmaxs) in quarters {
        tr = w.trace_box(start, end, qmins, qmaxs, clip);
        if tr_ent(&tr) && !tr.start_solid && tr.normal.z >= 0.7 {
            break;
        }
    }
    tr.end = orig.end;
    tr.fraction = orig.fraction;
    tr
}

/// The end of CategorizePosition (`0x1801f3f23`): the ground plane copy (`player+0x3b7c`).
fn finish_categorize(p: &mut Pm, tr: Option<&Trace>) {
    p.ground_normal = match tr {
        Some(t) if p.on_ground && !(t.fraction >= 1.0 && !tr_allsolid(t) && !t.start_solid) => t.normal,
        _ => Vec3::ZERO,
    };
}

/// StayOnGround (`0x180207860` / `0x1801faed0`): a trace a step down; when it finds a floor
/// within 0.7 of the normal (and not within 1/64 of where the Pilot is), the Pilot is put on
/// it. The trace is kept for CategorizePosition. Returns whether a floor was found.
fn stay_on_ground(p: &mut Pm, s: &Settings, w: &CollisionWorld) -> bool {
    let start = p.origin;
    let end = p.origin - p.normal * p.step_height;
    let mut tr = p.trace(w, s, start, end);
    if tr.start_solid {
        let start2 = start + p.normal;
        tr = p.trace(w, s, start2, end);
        if tr.start_solid {
            p.cached = Some((tr, end));
            return false;
        }
    }
    let found = if tr.fraction <= 0.0 {
        true
    } else if tr.fraction < 1.0 {
        if 0.7 <= p.normal.dot(tr.normal) {
            if 0.015625 < p.normal.dot(tr.end - p.origin).abs() {
                true
            } else {
                tr.end = p.origin;
                true
            }
        } else {
            false
        }
    } else {
        false
    };
    p.cached = Some((tr, end));
    if !found {
        return false;
    }
    if !p.is_wallrunning() {
        p.origin = tr.end;
    }
    true
}

/// TryPlayerMove (`0x1801fa310`): slide `vel` for `time` along up to 5 planes, 8 bumps.
/// `first` is a trace already made for the first bump. Returns the blocked flags.
fn try_player_move(p: &Pm, origin: &mut Vec3, vel: &mut Vec3, time: f32, first: Option<&Trace>, s: &Settings, w: &CollisionWorld) -> u32 {
    let mut blocked = 0u32;
    let original = *vel;
    let mut hit = false;
    let mut time_left = time;
    let mut planes: [Vec3; 5] = [Vec3::ZERO; 5];
    let mut numplanes = 0usize;
    let mut last_origin = *origin;
    let mut first = first.copied();
    let mut bump = 0;
    while bump < 8 {
        let tr = match first.take() {
            Some(t) => t,
            None => {
                let mut end = *origin + *vel * time_left;
                let d = end - *origin;
                if d.length_squared() < 0.0001 {
                    break;
                }
                for pl in planes.iter().take(numplanes) {
                    let dd = pl.dot(d);
                    if dd < 0.0 {
                        end -= *pl * (dd - 0.001);
                    }
                }
                p.trace(w, s, *origin, end)
            }
        };
        if 0.0 < tr.fraction {
            hit = true;
            *origin = tr.end;
            if tr.fraction == 1.0 {
                return blocked;
            }
            if 1.0 < (last_origin - tr.end).length_squared() {
                numplanes = 0;
                last_origin = tr.end;
            }
        }
        let fn_ = tr.normal.dot(DOWN);
        if 0.7 < fn_ {
            blocked |= 1;
        }
        if fn_ == 0.0 {
            blocked |= 2;
        }
        time_left -= time_left * tr.fraction;
        // Clip the velocity to the plane (with a 1/128 push off it).
        let n = tr.normal;
        *vel += n * (0.0078125 - n.dot(*vel));
        if numplanes == 0 {
            planes[0] = n;
            numplanes = 1;
        } else if numplanes == 1 {
            if 0.0 <= planes[0].dot(*vel) {
                planes[0] = n;
            } else {
                planes[1] = n;
                numplanes = 2;
                crease(&planes, vel);
            }
        } else {
            let d0 = planes[0].dot(n);
            let d1 = planes[1].dot(n);
            if d0.max(d1) < 0.866 {
                *vel = Vec3::ZERO;
                return blocked;
            }
            let idx = if d0 < d1 { 1 } else { 0 };
            planes[idx] = n;
            crease(&planes, vel);
        }
        if vel.dot(original) <= 0.0 {
            *vel = Vec3::ZERO;
            return blocked;
        }
        bump += 1;
    }
    if !hit {
        *vel = Vec3::ZERO;
    }
    blocked
}

/// Two planes: the velocity goes along their crease (`0x1801fac63`).
fn crease(planes: &[Vec3; 5], vel: &mut Vec3) {
    let p0 = planes[0];
    let p1 = planes[1];
    let mut cr = p1.cross(p0);
    let sum = p0 + p1;
    let k = (cr.length_squared() / (sum.length_squared() + 1.1754944e-38)).sqrt();
    let sign = if cr.dot(*vel) < 0.0 { -1.0 } else { 1.0 };
    cr += sum * (sign * k * 0.0078125);
    let d = cr.dot(*vel) / (cr.length_squared() + 1.1754944e-38);
    *vel = cr * d;
}

/// The blocked handler (`0x1802055b0`): a walkable plane (within 0.7 of the normal) is taken
/// by moving the rest of the frame along it; anything else goes to the step handler.
#[allow(clippy::too_many_arguments)]
fn blocked_handler(p: &mut Pm, tr: &Trace, walk: bool, target: Option<Vec3>, no_slope: bool, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) -> bool {
    let n = p.normal;
    let tn = tr.normal;
    if tn.dot(n) <= 0.7 || no_slope {
        step_handler(p, tr, s, cv, w, dt);
        return false;
    }
    let start = tr.end;
    let rem = (1.0 - tr.fraction) * dt;
    let dest;
    if !walk {
        let (v, scale) = match target {
            None => (p.vel, rem),
            Some(t) => (t - start, 1.0),
        };
        let vn = v.dot(tn);
        let nn = n.dot(tn);
        let perp = v * nn - n * vn;
        let v2 = v.length_squared();
        let p2 = perp.length_squared();
        if nn * nn * v2 <= p2 {
            let k = (v2 / p2).min(f32::MAX).sqrt() * scale;
            dest = start + perp * k + Vec3::new(0.0, 0.0, 0.01);
        } else {
            let k = scale / nn;
            dest = start + perp * k + Vec3::new(0.0, 0.0, 0.01);
        }
    } else {
        let (v, scale) = match target {
            None => (p.vel, rem),
            Some(t) => (t - start, 1.0),
        };
        let vn = v.dot(tn);
        let nn = n.dot(tn);
        let perp = v * nn - n * vn;
        let k = (v.length_squared() / perp.length_squared()).min(f32::MAX).sqrt() * scale;
        dest = start + perp * k + Vec3::new(0.0, 0.0, 0.01);
    }
    let f0 = tr.fraction;
    let mut t2 = p.trace(w, s, start, dest);
    if t2.fraction == 1.0 {
        p.origin = dest;
        return true;
    }
    t2.fraction = (1.0 - f0) * t2.fraction + f0;
    step_handler(p, &t2, s, cv, w, dt);
    false
}

/// The step handler (`0x180207a10`): StepMove, then a slide loses speed for the step climbed.
fn step_handler(p: &mut Pm, tr: &Trace, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    let mut origin = p.origin;
    let mut vel = p.vel;
    let disp = step_move(p, &mut origin, &mut vel, dt, p.step_height, s.step_test_dist, tr, s, w);
    p.origin = origin;
    p.vel = vel;
    if p.sliding && 0.0 < disp.z {
        let h = (p.vel.x * p.vel.x + p.vel.y * p.vel.y).sqrt();
        if 0.0 < h {
            let nh = (h - disp.z * cv.slide_step_velocity_reduction).max(0.0);
            p.vel.x *= nh / h;
            p.vel.y *= nh / h;
        }
    }
}

/// StepMove (`0x1801fb0e0`): the ground move against the move after stepping up, the better
/// one wins; the stepped move is taken only when the floor under it is walkable (or, close to
/// a steep edge, a probe stepTestDist further says so). Returns how far the step moved.
#[allow(clippy::too_many_arguments)]
fn step_move(p: &Pm, origin: &mut Vec3, vel: &mut Vec3, dt: f32, step: f32, step_test_dist: f32, tr: &Trace, s: &Settings, w: &CollisionWorld) -> Vec3 {
    let n = p.normal;
    if vel.length_squared() == 0.0 {
        return Vec3::ZERO;
    }
    let start = *origin;
    let v0 = *vel;
    // 1. Along the ground.
    let mut pos1 = start;
    let mut v1 = v0;
    try_player_move(p, &mut pos1, &mut v1, dt, Some(tr), s, w);
    *origin = pos1;
    *vel = v1;
    // 2. Up a step, then the move again.
    let up_dist0 = step + 0.03125;
    let t2 = p.trace(w, s, start, start + n * up_dist0);
    let (up_dist, pos2_start) = if !t2.start_solid && !tr_allsolid(&t2) { (t2.fraction * up_dist0, t2.end) } else { (0.0, start) };
    let mut pos2 = pos2_start;
    let mut v2 = v0;
    try_player_move(p, &mut pos2, &mut v2, dt, None, s, w);
    let d1 = pos1 - start;
    let d1 = d1 - n * d1.dot(n);
    let d2 = pos2 - start;
    let d2 = d2 - n * d2.dot(n);
    if d2.length_squared() * 0.999 <= d1.length_squared() {
        return Vec3::ZERO;
    }
    // 3. Back down.
    let down_dist = up_dist + step + 0.03125;
    let t3 = p.trace(w, s, pos2, pos2 - n * down_dist);
    let v3 = v2;
    let mut pos3 = pos2;
    if !t3.start_solid && !tr_allsolid(&t3) {
        pos3 = t3.end;
    }
    let mut t_floor = t3;
    let mut accept = t3.normal.dot(n) >= 0.7;
    if !accept {
        let dd = pos3 - pos1;
        let perp = dd - n * dd.dot(n);
        let p2 = perp.length_squared();
        if p2 <= 0.0 {
            return Vec3::ZERO;
        }
        let check_plane = |t: &Trace, dd: Vec3| -> bool {
            let f13 = t.normal.dot(n);
            if 0.0 < f13 {
                let vd = dd.dot(n);
                if vd <= 0.0 {
                    return true;
                }
                let pp = dd - n * vd;
                let r = pp.length_squared();
                return 0.48999998 < r / (vd * vd + r);
            }
            false
        };
        if step_test_dist * step_test_dist <= p2 {
            accept = check_plane(&t3, dd);
        } else {
            // Too close to the edge: probe stepTestDist further along the move.
            let mut pp = pos2_start;
            let mut pv = Vec3::new(v0.x, v0.y, v0.z.max(0.0));
            let l = (v0.x * v0.x + v0.y * v0.y + v0.z.max(0.0).powi(2)).sqrt();
            try_player_move(p, &mut pp, &mut pv, step_test_dist / l, None, s, w);
            let t4 = p.trace(w, s, pp, pp - n * down_dist);
            let dd4 = t4.end - pos1;
            pos3 = t4.end;
            t_floor = t4;
            if t4.normal.dot(n) < 0.7 {
                accept = check_plane(&t4, dd4);
            } else {
                accept = true;
            }
        }
    }
    if !accept {
        return Vec3::ZERO;
    }
    let mut out = pos3 - start;
    if n.z != 1.0 {
        let f = t_floor.normal.dot(pos3 - start);
        out = if f <= 0.0 { Vec3::ZERO } else { t_floor.normal * f };
    }
    *origin = pos3;
    *vel = v3;
    out
}

/// `0x1802c2e80`: the wall-run ends; the move's up vector is straight up again.
fn end_wallrun(p: &mut Pm) {
    if p.normal.z < 1.0 {
        p.wallrun_end_time = p.now;
    }
    p.normal = Vec3::Z;
    p.prev_normal.y = p.normal.y;
    p.prev_normal.z = p.normal.z;
    p.hanging = false;
    p.push_away_timer = 0.0;
}

/// `0x18020e640`: how much of the wall-run is left while hanging (1 while running).
fn wallrun_strength(p: &Pm, cv: &Cvars) -> f32 {
    if !p.is_wallrunning() || !p.hanging {
        return 1.0;
    }
    let t = p.hang_time;
    let slip = ((t - cv.wallrun_hang_slip_start_time) / cv.wallrun_hang_slip_duration).clamp(0.0, 1.0);
    if cv.wallrun_hang_stop_time <= t {
        return slip;
    }
    let f = 1.0 - t / cv.wallrun_hang_stop_time;
    (1.0 - f) * slip + f
}

/// `0x18020e6f0`: gravity ramps in over wallrun_gravityRampUpTime from the wall-run's start,
/// scaled by the wall-run strength (by count).
fn gravity_ramp(p: &Pm, s: &Settings, cv: &Cvars, t: f32) -> f32 {
    let ramp = if s.wallstick { 100.0 } else { cv.wallrun_gravity_ramp_up_time };
    let t = t.max(0.0);
    if ramp <= t {
        return wallrun_strength(p, cv);
    }
    let st = p.wallrun_strength_by_count(cv);
    wallrun_strength(p, cv) * (1.0 - (1.0 - t / ramp) * st)
}

/// `0x1802068c0`: the wall normal turns toward `n`, by at most wallrun_rotateMaxRate x dt.
fn turn_normal(p: &mut Pm, n: Vec3, cv: &Cvars, dt: f32) {
    let max = cv.wallrun_rotate_max_rate * dt;
    let c = max.cos();
    let cur = p.normal;
    if c < n.dot(cur) {
        p.normal = n;
        return;
    }
    let sn = max.sin();
    let cr = cur.cross(n);
    let mut perp = cr.cross(cur);
    if perp.length_squared() == 0.0 {
        if n.length_squared() == 0.0 {
            return;
        }
        perp = Vec3::new(cur.y, -cur.x, 0.0);
    }
    let v = cur * c + perp * (sn / perp.length());
    p.normal = norm(v);
}

/// The wall probe (`0x18020e2b0`): a cube of the hull's half width swept from `pos` into the
/// wall (`-n`) by `extra` + the allowed distance + a corner margin. 0: blocked by something
/// that is not a wall, 1: no wall within `allowed`, 2: the wall is there (its normal in `out`).
fn wall_probe(p: &Pm, pos: Vec3, extra: f32, n: Vec3, hw: f32, allowed: f32, out: &mut Vec3, w: &CollisionWorld) -> i32 {
    let len = extra + allowed + hw * 0.2929;
    let end = Vec3::new(pos.x - len * n.x, pos.y - len * n.y, pos.z);
    let tr = w.trace_box(pos, end, Vec3::splat(-hw), Vec3::splat(hw), p.clip);
    if tr.fraction == 1.0 {
        return 1;
    }
    let mut d = tr.fraction * len;
    if allowed < d && 0.0 < tr.normal.z && !tr.start_solid {
        let f = (tr.normal.x * n.x + tr.normal.y * n.y).clamp(0.0, 1.0);
        d = ((1.0 / f - 1.0) * hw + d) - tr.normal.z * (1.0 / f) * extra;
    }
    *out = tr.normal;
    if d <= allowed {
        2
    } else {
        1
    }
}

/// `0x180201e50`: pushing and looking into the wall squarely enough to mantle instead.
fn mantle_check(p: &Pm, yaw_fwd: Vec3, n: Vec3, s: &Settings, cv: &Cvars) -> bool {
    let wd = p.wishdir;
    let push = -(wd.dot(n));
    if push < 0.0 {
        return false;
    }
    let c = cv.automantle_maxangle_push.to_radians().cos();
    if !(wd.length_squared() * c * c <= push * push) {
        return false;
    }
    let f = -(yaw_fwd.dot(n)) * s.mantle_angle_scale;
    if !p.is_wallrunning() {
        cv.automantle_maxangle_view.to_radians().cos() <= f
    } else {
        cv.automantle_wallrun_maxangle_view.to_radians().cos() <= f && -100.0 <= p.vel.dot(n)
    }
}

/// CanWallrun (`0x180202090`) for the wall with normal `n` hit at `pos` (by trace `tr`, if
/// any) with velocity `vel`. 0: no, 1: not this frame (the probes failed), 2: yes.
fn can_wallrun(p: &mut Pm, pos: Vec3, n: Vec3, tr: Option<&Trace>, vel: Vec3, s: &Settings, cv: &Cvars, w: &CollisionWorld) -> i32 {
    if !cv.wallrun_enable {
        return 0;
    }
    if ((p.on_ground || p.now - p.time_left_ground < 0.5) && !p.is_wallrunning()) || !s.wallrun_enabled {
        return 0;
    }
    if 0.7 < n.z || n.z < -0.35 {
        return 0;
    }
    if let Some(t) = tr {
        if t.start_solid {
            return 0;
        }
    }
    if p.duck_state != 0 && !can_unduck(p, s, w) && p.forced_slide != 3 {
        return 0;
    }
    if !s.wallstick && !p.last_walls.is_empty() {
        let nf = norm(Vec3::new(n.x, n.y, 0.0));
        let top = p.wallrun_start_pos.z + cv.wallrun_same_wall_height;
        for lw in &p.last_walls {
            if cv.wallrun_same_wall_dot < nf.dot(*lw) {
                if !s.wallrun_same_wall_allowed {
                    return 0;
                }
                let z = p.origin.z;
                if top <= z {
                    let dx = p.origin.x - p.wallrun_start_pos.x;
                    let dy = p.origin.y - p.wallrun_start_pos.y;
                    let d2 = dx * dx + dy * dy;
                    let dist = cv.wallrun_same_wall_dist;
                    if d2 <= dist * dist {
                        return 0;
                    }
                    if (d2.sqrt() - dist) * cv.wallrun_same_wall_slope <= z - top {
                        return 0;
                    }
                }
            }
        }
    }
    if !p.is_wallrunning() {
        let v = norm(vel);
        let c = (cv.wallrun_min_angle_air * 0.5).to_radians().cos();
        if c * -1.0 <= v.dot(n) {
            return 0;
        }
    }
    let behind = (p.origin - pos).dot(n) < -0.01;
    let base = if behind { p.origin } else { pos };
    let hw = p.hull_half_width(s);
    let hh = p.hull_height(s);
    let probes = [(base + Vec3::new(0.0, 0.0, 16.0), 16.0), (base + Vec3::new(0.0, 0.0, hh * 0.5 + hw), hh * 0.5 + hw)];
    let mut out = Vec3::ZERO;
    for (pt, extra) in probes {
        let r = wall_probe(p, pt, extra, n, hw, cv.wallrun_allowed_wall_dist, &mut out, w);
        if r != 2 {
            return r;
        }
        if behind && 0.7 < out.z {
            return 0;
        }
    }
    if !p.is_wallrunning() {
        let (sy, cy) = p.yaw.to_radians().sin_cos();
        let yaw_fwd = Vec3::new(cy, sy, 0.0);
        if mantle_check(p, yaw_fwd, n, s, cv) {
            let pt = Vec3::new(base.x, base.y, (hh - hw) + base.z);
            let r = wall_probe(p, pt, hh, n, hw, cv.wallrun_allowed_wall_dist, &mut out, w);
            if r != 2 {
                return r;
            }
        }
    }
    2
}

/// TryStartWallrun (`0x18020b390`): after a move ran into `n` at `pos` with velocity `vel`.
#[allow(clippy::too_many_arguments)]
fn try_start_wallrun(p: &mut Pm, pos: Vec3, n: Vec3, tr: Option<&Trace>, vel: Vec3, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    p.retry_time = 0.0;
    let r = can_wallrun(p, pos, n, tr, vel, s, cv, w);
    if r == 2 {
        if !p.is_wallrunning() {
            p.normal = n;
            p.wallrun_count += 1;
            let boost = p.wallrun_strength_by_count(cv) * cv.wallrun_up_wall_boost;
            let h = (n.x * n.x + n.y * n.y).sqrt();
            let wallup = Vec3::new(-(n.z * n.x * (1.0 / h)), -(n.z * n.y * (1.0 / h)), h);
            let cur = p.vel.dot(wallup);
            if cur < boost {
                let cur = cur.max(0.0);
                p.vel += wallup * (boost - cur);
            }
            for lw in &p.last_walls {
                if cv.wallrun_same_wall_dot < n.dot(*lw) {
                    p.no_boost = true;
                    break;
                }
            }
            p.wallrun_start_pos = p.origin;
            p.wallrun_start_time = p.now;
            p.push_away_timer = 0.0;
            p.hanging = false;
            p.dodging = false;
            p.dodge_flag = false;
            p.duck_toggle = false;
            if p.sliding {
                p.sliding = false;
                if cv.slide_auto_stand {
                    p.duck_toggle = false;
                }
            }
        } else {
            turn_normal(p, n, cv, dt);
        }
    } else {
        if r == 1 && !p.on_ground && n.z <= 0.7 {
            p.retry_time = cv.wallrun_retry_interval + p.now;
            p.retry_pos = p.origin;
            p.retry_normal = n;
        }
        if n.z <= 0.7 {
            if p.is_wallrunning() && p.vel.z <= 140.0 {
                let end = p.origin - Vec3::new(0.0, 0.0, p.step_height);
                let t = p.trace(w, s, p.origin, end);
                if t.fraction < 1.0 && 0.7 < t.normal.z {
                    end_wallrun(p);
                }
            }
        } else if 0.0 <= (p.origin - pos).dot(n) {
            end_wallrun(p);
        }
    }
}

/// The wall-run update (`0x18020e7e0`): hang start / end, pushing away, the time limit, the
/// feet probe; a wall-run that ends here pushes the Pilot off the wall.
fn wallrun_update(p: &mut Pm, s: &Settings, cv: &Cvars, w: &CollisionWorld, dt: f32) {
    p.prev_normal = p.normal;
    if !p.is_wallrunning() {
        // In the air the engine looks ahead for the view tilt only.
        return;
    }
    let prev_hang_time = p.hang_time;
    let mut fall = false;
    let mut timed_out = false;
    if p.buttons & (IN_DUCK | IN_DUCKTOGGLE) != 0 {
        p.jumped_since_leaving = true;
        fall = true;
    }
    if p.forced_slide == 2 || p.forced_slide == 3 {
        fall = true;
    }
    let was_hanging = p.hanging;
    if !p.hanging && (p.zoom_toggle || p.buttons & IN_ZOOM != 0) {
        if s.wallrun_ads_type == 1 {
            p.hanging = true;
            p.hang_start = p.now;
            p.hang_time = 0.0;
        } else if s.wallrun_ads_type == 0 {
            p.zoom_toggle = false;
        }
    }
    if p.hanging {
        let hh = p.hull_height(s);
        let hw = p.hull_half_width(s);
        let pt = Vec3::new(p.origin.x, p.origin.y, p.origin.z + hh);
        let mut out = Vec3::ZERO;
        let n = p.normal;
        if wall_probe(p, pt, 0.0, n, hw, cv.wallrun_allowed_wall_dist_wallhang, &mut out, w) != 2 {
            p.hanging = false;
        }
    }
    if !p.hanging {
        if p.wishdir.dot(p.normal) <= 0.7 {
            p.push_away_timer = 0.0;
        } else {
            if p.push_away_timer == 0.0 {
                p.push_away_timer = p.now;
            }
            if cv.wallrun_push_away_fall_off_time < p.now - p.push_away_timer {
                fall = true;
            }
        }
        if s.wallrun_time_limit < p.now - p.wallrun_start_time {
            fall = true;
            timed_out = true;
        }
    } else {
        p.hang_time += dt;
        if s.wallrun_hang_time_limit < p.hang_time {
            p.saved_vel = Vec3::ZERO;
            fall = true;
        }
        if !p.zoom_toggle && p.buttons & IN_ZOOM == 0 {
            fall = true;
            p.saved_vel = Vec3::ZERO;
        }
        let _ = (was_hanging, prev_hang_time);
    }
    // The feet probe (gravity orientation 0): the wall must still be there 16 units up.
    let hw = p.hull_half_width(s);
    let pt = Vec3::new(p.origin.x, p.origin.y, p.origin.z + 16.0);
    let mut out = Vec3::ZERO;
    let n = p.normal;
    let there = wall_probe(p, pt, 16.0, n, hw, cv.wallrun_allowed_wall_dist, &mut out, w) == 2;
    if there && !fall {
        p.prev_normal = p.normal;
        return;
    }
    p.zoom_toggle = false;
    p.vel += p.normal * cv.wallrun_fall_away_speed;
    remember_wall(p, n, cv);
    end_wallrun(p);
    p.no_boost = true;
    on_land(p, None, s, cv);
    let _ = timed_out;
    p.prev_normal = p.normal;
}
