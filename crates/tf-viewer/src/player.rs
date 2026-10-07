//! Playing as BT: input (live or scripted), Titan movement, locomotion animation and cameras.

use crate::actor::{Actor, Aim, Layer};
use bevy::camera_controller::free_camera::FreeCameraState;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3 as SVec3;
use tf_sim::titan::{self, TitanInput, TitanParams, TitanState};

/// Game units (inches) to metres.
pub const UNIT: f32 = 0.0254;

/// Game space (Z up, inches) to Bevy space (Y up, metres).
pub fn to_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y) * UNIT
}

/// Bevy space back to game space (inverse of `to_bevy`).
pub fn from_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y) / UNIT
}

fn sv(v: Vec3) -> SVec3 {
    SVec3::new(v.x, v.y, v.z)
}
fn bv(v: SVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// The level's collision, and the game's Titan (`_large`) and infantry (`_small`) navmeshes
/// when the map ships them.
#[derive(Resource)]
pub struct Collision(pub CollisionWorld, pub Option<crate::nav::Nav>, pub Option<crate::nav::Nav>);

#[derive(Resource, Clone)]
pub struct TitanSettings(pub TitanParams);

/// The free camera's `--cam` / `--look` start (Bevy space), re-applied by the script's `free`.
#[derive(Resource, Clone, Copy)]
pub struct FreeCamStart(pub Vec3, pub Vec3);

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    Third,
    Cockpit,
    Free,
}

/// The player's Titan. Lives on the entity that places BT in the world (game space).
#[derive(Component)]
pub struct PlayerTitan {
    pub state: TitanState,
    pub actor: Entity,
    /// Cockpit placement entity and its actor.
    pub cockpit: Option<(Entity, Entity)>,
    /// A sequence (and time into it) that replaces locomotion, e.g. embark or Titanfall.
    pub override_anim: Option<(String, f32)>,
    pub move_cycle: f32,
    idle_cycle: f32,
    /// Smoothed animation blend inputs.
    pub blend_speed: f32,
    blend_dir: Vec2,
    sprint_blend: f32,
}

impl PlayerTitan {
    pub fn new(state: TitanState, actor: Entity, cockpit: Option<(Entity, Entity)>) -> Self {
        Self {
            state,
            actor,
            cockpit,
            override_anim: None,
            move_cycle: 0.0,
            idle_cycle: 0.0,
            blend_speed: 0.0,
            blend_dir: Vec2::X,
            sprint_blend: 0.0,
        }
    }
}

/// Input for the current frame, from the keyboard/mouse or from a test script.
#[derive(Resource, Default, Debug)]
pub struct PlayerInput {
    pub yaw: f32,
    pub pitch: f32,
    pub forward: f32,
    pub right: f32,
    pub sprint: bool,
    /// Latched until the simulation consumes it.
    pub dash: bool,
    pub fire: bool,
    pub ads: bool,
    /// Latched until the weapon consumes it.
    pub reload: bool,
    /// Ordnance button held (lock on while held, fire on release).
    pub ordnance: bool,
    /// Core requested (latched).
    pub core: bool,
    /// Vortex Shield held.
    pub vortex: bool,
    /// Titan utility slot (+offhand2: Electric Smoke, Sonar Pulse, ...).
    pub utility: bool,
    /// Punch requested (latched).
    pub melee: bool,
    /// Pilot jump (latched; Space also requests a Titan dash).
    pub jump: bool,
    pub crouch: bool,
    /// Embark/disembark (latched).
    pub interact: bool,
    /// Script `pfxtest`: play TF_PFX_TEST now (pfx::pfx_test).
    pub pfx_test: bool,
    /// Pilot tactical ability (Q on foot) and frag throw (G), one-shot.
    pub tactical: bool,
    pub throw: bool,
    /// Call in a Titanfall (latched).
    pub titanfall: bool,
    /// Start / restart the game (latched).
    pub start: bool,
    /// Toggle the pause menu (scripted tests; Esc for players).
    pub pause: bool,
    /// Scripted menu navigation: 0 up, 1 down, 2 left, 3 right, 4 confirm, 5 back.
    pub menu_nav: Option<u8>,
    /// Pilot weapon slot to switch to (0 primary, 1 sidearm, 2 anti-Titan).
    pub pilot_slot: Option<u8>,
    /// BT loadout to switch to (index into weapons::TITAN_ARSENAL).
    pub titan_kit: Option<u8>,
    /// The pilot's own weapon inputs (the Titan's are cleared outside the Titan and these
    /// inside it).
    pub pilot_fire: bool,
    pub pilot_ads: bool,
    pub pilot_reload: bool,
}

/// Scripted input for automated testing: `start-end:action` entries separated by commas.
/// Actions: fwd back left right sprint dash fire ads reload aim ordnance core vortex melee
/// jump crouch interact tactical throw titanfall turn=DEG/S pitch=DEG cockpit third
/// nav=N (menu: 0 up, 1 down, 2 left, 3 right, 4 confirm, 5 back).
#[derive(Resource, Default)]
pub struct Script {
    pub entries: Vec<(f32, f32, String)>,
    pub fired: Vec<bool>,
    pub enabled: bool,
    /// Set while an `aim` entry is active: point the view at the nearest living dummy.
    pub aim_dummy: bool,
}

impl Script {
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Some((range, action)) = item.split_once(':') else { continue };
            let (a, b) = match range.split_once('-') {
                Some((a, b)) => (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)),
                None => {
                    let t = range.parse().unwrap_or(0.0);
                    (t, t)
                }
            };
            entries.push((a, b, action.to_string()));
        }
        Self { fired: vec![false; entries.len()], entries, enabled: true, aim_dummy: false }
    }
}

pub fn gather_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut input: ResMut<PlayerInput>,
    mut script: ResMut<Script>,
    mut mode: ResMut<CameraMode>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut free: Query<&mut FreeCameraState>,
    game: Res<crate::game::Game>,
    menu: Res<crate::ui::Menu>,
    loaded_at: Option<Res<crate::ui::LoadedAt>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut latches: Local<(u8, bool)>,
    (settings, gamepads, pad_cfg, mut pad, mut assist, cams, control): (
        Res<crate::settings::Settings>,
        Query<&Gamepad>,
        Res<crate::gamepad::PadConfig>,
        ResMut<crate::gamepad::PadState>,
        ResMut<crate::aimassist::AimAssist>,
        Query<&Projection, With<MainCamera>>,
        Res<crate::pilotctl::Control>,
    ),
    (free_start, mut cam_tf, pilot_settings): (Option<Res<FreeCamStart>>, Query<&mut Transform, With<MainCamera>>, Res<crate::pilotctl::PilotSettings>),
) {
    // Look limits (positive pitch is down): the Pilot's pitchMaxUp/Down (pilot_solo.set 85/89),
    // BT keeps the old +-1.4 rad.
    let (pitch_lo, pitch_hi) = if *control == crate::pilotctl::Control::Pilot {
        (-pilot_settings.0.pitch_max_up.to_radians(), pilot_settings.0.pitch_max_down.to_radians())
    } else {
        (-1.4, 1.4)
    };
    for mut f in &mut free {
        f.enabled = *mode == CameraMode::Free;
    }
    if script.enabled {
        let t = loaded_at.as_ref().map(|l| l.secs()).unwrap_or(0.0);
        let dt = time.delta_secs();
        input.forward = 0.0;
        input.right = 0.0;
        input.sprint = false;
        input.fire = false;
        input.ads = false;
        input.pilot_fire = false;
        input.pilot_ads = false;
        input.ordnance = false;
        input.vortex = false;
        input.utility = false;
        input.crouch = false;
        script.aim_dummy = false;
        let mut new_mode = None;
        for (i, (a, b, action)) in script.entries.clone().into_iter().enumerate() {
            let active = t >= a && t <= b.max(a);
            let (name, arg) = action.split_once('=').unwrap_or((action.as_str(), ""));
            let arg: f32 = arg.parse().unwrap_or(0.0);
            // One-shot actions fire once when their start time passes.
            let once = t >= a && !script.fired[i];
            match name {
                "fwd" if active => input.forward += 1.0,
                "back" if active => input.forward -= 1.0,
                "left" if active => input.right -= 1.0,
                "right" if active => input.right += 1.0,
                "sprint" if active => input.sprint = true,
                "fire" if active => {
                    input.fire = true;
                    input.pilot_fire = true;
                }
                "start" if once => input.start = true,
                "pause" if once => input.pause = true,
                "nav" if once => input.menu_nav = Some(arg as u8),
                "slot" if once => input.pilot_slot = Some(arg as u8),
                "kit" if once => input.titan_kit = Some(arg as u8),
                "aim" if active => script.aim_dummy = true,
                "ordnance" if active => input.ordnance = true,
                "core" if once => input.core = true,
                "vortex" if active => input.vortex = true,
                "utility" if active => input.utility = true,
                "melee" if once => input.melee = true,
                "jump" if once => input.jump = true,
                "crouch" if active => input.crouch = true,
                "interact" if once => input.interact = true,
                "pfxtest" if once => input.pfx_test = true,
                "tactical" if once => input.tactical = true,
                "throw" if once => input.throw = true,
                "titanfall" if once => input.titanfall = true,
                "ads" if active => {
                    input.ads = true;
                    input.pilot_ads = true;
                }
                "reload" if once => {
                    input.reload = true;
                    input.pilot_reload = true;
                }
                "turn" if active => input.yaw += arg.to_radians() * dt,
                "pitch" if once => input.pitch = arg.to_radians(),
                "dash" if once => input.dash = true,
                "cockpit" if once => new_mode = Some(CameraMode::Cockpit),
                "third" if once => new_mode = Some(CameraMode::Third),
                "free" if once => {
                    new_mode = Some(CameraMode::Free);
                    // Back to the --cam/--look start, so a script can look at a spot.
                    if let Some(start) = free_start.as_deref() {
                        for mut tf in &mut cam_tf {
                            *tf = Transform::from_translation(start.0).looking_at(start.1, Vec3::Y);
                            let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
                            for mut st in &mut free {
                                st.yaw = yaw;
                                st.pitch = pitch;
                                st.velocity = Vec3::ZERO;
                            }
                        }
                    }
                }
                _ => {}
            }
            if once {
                script.fired[i] = true;
            }
        }
        if let Some(m) = new_mode {
            *mode = m;
        }
        return;
    }

    use crate::settings::Command as C;
    if keys.just_pressed(KeyCode::F1) {
        *mode = if *mode == CameraMode::Free { CameraMode::Cockpit } else { CameraMode::Free };
    }
    let bind = |c: C| settings.bind(c);
    let down = |c: C| bind(c).pressed(&keys, &buttons);
    let hit = |c: C| bind(c).just_pressed(&keys, &buttons);
    if hit(C::View) && *mode != CameraMode::Free {
        *mode = if *mode == CameraMode::Third { CameraMode::Cockpit } else { CameraMode::Third };
    }
    if *mode == CameraMode::Free {
        return;
    }
    // How zoomed the view is now (ADS), for sensitivity: tan(fov/2) against the hip FOV's.
    let zoom = cams
        .single()
        .ok()
        .and_then(|p| if let Projection::Perspective(pp) = p { Some((pp.fov * 0.5).tan() / (settings.vertical_fov() * 0.5).tan()) } else { None })
        .unwrap_or(1.0)
        .clamp(0.05, 1.0);
    let zoomed = zoom < 0.98;

    // Grab the mouse on click, release with Escape.
    let mut grabbed = false;
    if let Ok(mut c) = cursor.single_mut() {
        grabbed = c.grab_mode != CursorGrabMode::None;
        // Only grab the mouse for play, never over a menu.
        let playing = game.in_play() && !menu.paused;
        if buttons.just_pressed(MouseButton::Left) && !grabbed && playing {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
        }
        if keys.just_pressed(KeyCode::Escape) {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
        }
        if c.grab_mode != CursorGrabMode::None {
            // Source: m_yaw 0.022 degrees per count times mouse_sensitivity; zoomed, the
            // zoomed sensitivity scaled by how much the view is magnified.
            let sens = if zoomed { settings.mouse_sensitivity_zoomed * zoom } else { settings.mouse_sensitivity };
            let k = (crate::settings::M_YAW * sens).to_radians();
            let invert = if settings.invert_y { -1.0 } else { 1.0 };
            input.yaw -= motion.delta.x * k;
            input.pitch = (input.pitch + motion.delta.y * k * invert).clamp(pitch_lo, pitch_hi);
            if motion.delta != Vec2::ZERO {
                pad.active = false;
            }
        }
    }
    let axis = |pos: C, neg: C| down(pos) as i32 as f32 - down(neg) as i32 as f32;
    input.forward = axis(C::Forward, C::Back);
    input.right = axis(C::Right, C::Left);
    input.sprint = down(C::Sprint);
    if hit(C::Jump) {
        input.dash = true;
        input.jump = true;
    }
    input.crouch = down(C::Crouch);
    if hit(C::Use) {
        input.interact = true;
    }
    for (c, slot) in [(C::Slot1, 0u8), (C::Slot2, 1), (C::Slot3, 2)] {
        if hit(c) {
            input.pilot_slot = Some(slot);
        }
    }
    // BT's loadouts stay on the number row (1-8).
    let kit_keys = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8];
    for (i, k) in kit_keys.into_iter().enumerate() {
        if keys.just_pressed(k) {
            input.titan_kit = Some(i as u8);
        }
    }
    if scroll.delta.y.abs() > 0.0 {
        let cur = latches.0;
        let next = if scroll.delta.y < 0.0 { (cur + 1) % 3 } else { (cur + 2) % 3 };
        input.pilot_slot = Some(next);
    }
    if hit(C::Titanfall) {
        input.titanfall = true;
    }
    // The first click only grabs the mouse; after that the buttons fire and aim.
    let mouse_ok = |c: C| !matches!(bind(c), crate::settings::Bind::Mouse(_)) || grabbed;
    input.fire = mouse_ok(C::Fire) && down(C::Fire);
    input.ads = mouse_ok(C::Ads) && down(C::Ads);
    // Sprint latches like the game's: a tap starts it and it keeps going while you move
    // forward, until you stop, crouch (a slide), aim or fire.
    if hit(C::Sprint) || (input.sprint && input.forward > 0.5) {
        latches.1 = true;
    }
    if input.forward < 0.5 || input.crouch || input.ads || input.fire {
        latches.1 = false;
    }
    input.sprint = latches.1;
    if hit(C::Reload) {
        input.reload = true;
        input.pilot_reload = true;
    }
    // The game's offhand slots: +offhand1 (Q) is the Pilot's tactical and the Titan's
    // defensive, +offhand0 (G) the ordnance of both, +offhand2 (E) the Titan's utility.
    input.vortex = down(C::Ability);
    if hit(C::Ability) {
        input.tactical = true;
    }
    input.ordnance = down(C::Throw);
    if hit(C::Throw) {
        input.throw = true;
    }
    if hit(C::Core) {
        input.core = true;
    }
    input.utility = down(C::Utility);
    if hit(C::Melee) {
        input.melee = true;
    }

    // --- Controller ---
    let dt = time.delta_secs();
    let playing = game.in_play() && !menu.paused;
    if let Some(gp) = gamepads.iter().next().filter(|_| playing) {
        let any_button = gp.get_pressed().next().is_some();
        let left = crate::gamepad::deadzone(gp.left_stick(), 0.15);
        let right = crate::gamepad::deadzone(gp.right_stick(), 0.15);
        if any_button || left != Vec2::ZERO || right != Vec2::ZERO {
            pad.active = true;
        }
        if left != Vec2::ZERO {
            input.forward = left.y;
            input.right = left.x;
        }
        // Look: the game's sensitivity table row (Titan or Pilot, hip or ADS) and curve.
        let titan = *control != crate::pilotctl::Control::Pilot;
        let table = match (titan, zoomed) {
            (true, true) if !pad_cfg.look_titan_zoomed.is_empty() => &pad_cfg.look_titan_zoomed,
            (true, false) if !pad_cfg.look_titan.is_empty() => &pad_cfg.look_titan,
            (false, true) if !pad_cfg.look_zoomed.is_empty() => &pad_cfg.look_zoomed,
            _ => &pad_cfg.look,
        };
        let idx = if zoomed { settings.gamepad_look_ads } else { settings.gamepad_look };
        if let Some(row) = table.get(idx.min(table.len().saturating_sub(1))) {
            pad.edge_time = if right.length() >= 0.99 { pad.edge_time + dt } else { 0.0 };
            let lin = crate::gamepad::Curve::linear();
            let curve = pad_cfg.curves.get(settings.gamepad_curve).unwrap_or(&lin);
            let rate = crate::gamepad::look_rate(right, row, curve, pad_cfg.accel.as_ref(), pad.edge_time) * assist.slowdown;
            let invert = if settings.gamepad_invert_y { -1.0 } else { 1.0 };
            input.yaw -= rate.x.to_radians() * dt;
            input.pitch = (input.pitch - rate.y.to_radians() * dt * invert).clamp(pitch_lo, pitch_hi);
        }
        let (dy, dp) = assist.take_pull(dt);
        input.yaw += dy;
        input.pitch = (input.pitch + dp).clamp(pitch_lo, pitch_hi);
        use crate::gamepad::PadCommand as P;
        let mut core_pair = 0;
        for &(b, cmd) in &pad_cfg.buttons {
            let held = gp.pressed(b);
            let tap = gp.just_pressed(b);
            match cmd {
                P::Jump if tap => {
                    input.jump = true;
                    input.dash = true;
                }
                P::ToggleDuck if tap => pad.crouch_toggled = !pad.crouch_toggled,
                P::UseAndReload => {
                    // Tap reloads, holding uses (embark/disembark), as `+useandreload` does.
                    if held {
                        pad.use_held += dt;
                        if pad.use_held > 0.3 && !pad.use_fired {
                            pad.use_fired = true;
                            input.interact = true;
                        }
                    } else if gp.just_released(b) {
                        if !pad.use_fired {
                            input.reload = true;
                            input.pilot_reload = true;
                        }
                        pad.use_held = 0.0;
                        pad.use_fired = false;
                    }
                }
                P::SwitchWeapon if tap => {
                    // Between the primary and the sidearm (from the anti-Titan weapon: primary).
                    let next = if latches.0 == 0 { 1 } else { 0 };
                    input.pilot_slot = Some(next);
                }
                P::AntiTitan if tap => input.pilot_slot = Some(2),
                P::Zoom if held => input.ads = true,
                P::Attack if held => input.fire = true,
                P::Offhand1 => {
                    if held {
                        input.vortex = true;
                        core_pair += 1;
                    }
                    if tap {
                        input.tactical = true;
                    }
                }
                P::Offhand0 => {
                    if held {
                        input.ordnance = true;
                        core_pair += 1;
                    }
                    if tap {
                        input.throw = true;
                    }
                }
                P::Titanfall if tap => input.titanfall = true,
                P::Sprint if held => input.sprint = true,
                P::Melee if tap => input.melee = true,
                P::Pause if tap => input.pause = true,
                _ => {}
            }
        }
        // Titan core: both shoulder buttons together.
        if core_pair == 2 && gamepads.iter().next().is_some_and(|g| g.just_pressed(GamepadButton::LeftTrigger) || g.just_pressed(GamepadButton::RightTrigger)) {
            input.core = true;
        }
        if pad.crouch_toggled {
            input.crouch = true;
        }
    }
    if let Some(s) = input.pilot_slot {
        latches.0 = s;
    }
    input.pilot_fire = input.fire;
    input.pilot_ads = input.ads;
}

pub fn simulate(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    mut input: ResMut<PlayerInput>,
    mode: Res<CameraMode>,
    control: Res<crate::pilotctl::Control>,
    mut titans: Query<(
        &mut PlayerTitan,
        &mut Transform,
        Option<&crate::abilities::TitanCore>,
        Option<&crate::combat::TitanHealth>,
        Option<&crate::pilotctl::Titanfall>,
        Option<&crate::weapons::Weapon>,
    )>,
    mut mods: ResMut<crate::titankit::TitanMoveMods>,
    auto: Res<crate::autotitan::AutoTitan>,
) {
    let dt = time.delta_secs().min(0.1);
    for (mut titan, mut tf, core, health, titanfall, weapon) in &mut titans {
        // While dropping in from the sky the Titanfall controls BT's position.
        if let Some(o) = titanfall.and_then(|t| t.offset) {
            tf.translation = bv(titan.state.pos) + o;
            tf.rotation = Quat::from_rotation_z(titan.state.yaw);
            continue;
        }
        let piloted = *control == crate::pilotctl::Control::Titan;
        let busy = titanfall.is_some_and(|t| t.busy()) || titan.override_anim.is_some();
        // On foot, BT moves himself (autotitan.rs) when he has something to do.
        let auto_in = if piloted { None } else { auto.input };
        let dead = health.is_some_and(|h| h.dead_for.is_some()) || (!piloted && auto_in.is_none()) || busy;
        // Burst Core slows BT while it spins up and fires.
        let mut params = settings.0.clone();
        if let Some(c) = core {
            let k = c.move_scale();
            params.speed *= k;
            params.sprint_speed *= k;
        }
        // Aiming down sights slows BT (the weapon's ads_move_speed_scale).
        if let Some(w) = weapon {
            params.speed *= 1.0 + (w.def.ads_move_scale - 1.0) * w.ads;
        }
        let frozen = *mode == CameraMode::Free || dead;
        let ti = match auto_in {
            Some(a) if !frozen => TitanInput { speed_scale: mods.speed_scale, ..a },
            _ => TitanInput {
                forward: if frozen || !piloted { 0.0 } else { input.forward },
                right: if frozen || !piloted { 0.0 } else { input.right },
                yaw: if piloted { input.yaw } else { titan.state.yaw },
                pitch: if piloted { input.pitch } else { 0.0 },
                sprint: input.sprint && !frozen && piloted,
                dash: input.dash && !frozen && piloted && mods.fly.is_none(),
                speed_scale: mods.speed_scale,
                fly: if frozen { None } else { mods.fly },
            },
        };
        // Fixed 120 Hz substeps keep movement independent of frame rate.
        let n = (dt * 120.0).ceil().max(1.0) as usize;
        if let Some(v) = mods.launch.take() {
            titan.state.vel = SVec3::new(v.x, v.y, v.z);
            titan.state.on_ground = false;
        }
        let dashes = titan.state.dashes;
        for i in 0..n {
            let mut step_input = ti;
            step_input.dash = ti.dash && i == 0;
            titan::step(&mut titan.state, &step_input, &params, &world.0, dt / n as f32);
        }
        if titan.state.dashes != dashes {
            crate::audio::cue(&mut commands, crate::audio::Cue::BtDash, None);
        }
        if piloted {
            input.dash = false;
        }
        tf.translation = bv(titan.state.pos);
        tf.rotation = Quat::from_rotation_z(titan.state.yaw);
    }
}

/// Choose and weight BT's locomotion clips from his velocity, and aim his gun arm.
pub fn drive_animation(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<TitanSettings>,
    mut titans: Query<(&mut PlayerTitan, Option<&crate::combat::Melee>)>,
    mut actors: Query<&mut Actor>,
) {
    let dt = time.delta_secs();
    for (mut t, melee) in &mut titans {
        let Ok(mut actor) = actors.get_mut(t.actor) else { continue };
        actor.autoplay = false;
        let s = t.state.clone();
        // Scripted sequences (embark, disembark, Titanfall landing) replace locomotion.
        if let Some((name, time_in)) = t.override_anim.clone() {
            if let Some(c) = actor.clip(&name) {
                let cycle = (time_in / actor.clips[c].duration.max(1e-3)).min(0.999);
                actor.layers = vec![Layer { clip: c, cycle, weight: 1.0 }];
                actor.aim = None;
                continue;
            }
        }
        let fwd = Vec2::new(s.yaw.cos(), s.yaw.sin());
        let left = Vec2::new(-s.yaw.sin(), s.yaw.cos());
        let v = Vec2::new(s.vel.x, s.vel.y);
        let speed = v.length();
        let local = Vec2::new(v.dot(fwd), v.dot(left));
        let k = 1.0 - (-dt * 10.0).exp();
        let sprint_target = if s.sprinting { ((speed - settings.0.speed) / (settings.0.sprint_speed - settings.0.speed)).clamp(0.0, 1.0) } else { 0.0 };
        let (bs, bd) = (t.blend_speed, t.blend_dir);
        t.blend_speed = bs + (speed - bs) * k;
        if speed > 10.0 {
            t.blend_dir = bd.lerp(local / speed, k).normalize_or(Vec2::X);
        }
        t.sprint_blend += (sprint_target - t.sprint_blend) * k;

        let names = ["bt_combat_walk_forward", "bt_combat_walk_backward", "bt_combat_walk_left", "bt_combat_walk_right"];
        let ids: Vec<Option<usize>> = names.iter().map(|n| actor.clip(n)).collect();
        let idle = actor.clip("bt_combat_idle").or_else(|| actor.clip("bt_casual_idle"));
        let sprint = actor.clip("bt_combat_sprint_forward_noaim");

        // Directional weights from the movement direction in BT's frame.
        let d = t.blend_dir;
        let mut w = [d.x.max(0.0), (-d.x).max(0.0), d.y.max(0.0), (-d.y).max(0.0)];
        let sum: f32 = w.iter().sum::<f32>().max(1e-3);
        w.iter_mut().for_each(|x| *x /= sum);
        let moving = (t.blend_speed / 60.0).clamp(0.0, 1.0);
        let sprint_w = t.sprint_blend * w[0];

        // Advance a shared, normalized cycle so the blended walk cycles stay in step.
        let mut stride = 0.0;
        for (i, id) in ids.iter().enumerate() {
            if let Some(c) = id {
                let clip = &actor.clips[*c];
                stride += w[i] * clip.ground_speed * clip.duration;
            }
        }
        if let Some(c) = sprint {
            let clip = &actor.clips[c];
            stride = stride * (1.0 - sprint_w) + sprint_w * clip.ground_speed * clip.duration;
        }
        if stride > 1.0 {
            let before = t.move_cycle;
            t.move_cycle = (t.move_cycle + t.blend_speed * dt / stride).rem_euclid(1.0);
            // Two footfalls per walk cycle.
            let crossed = (before < 0.5 && t.move_cycle >= 0.5) || t.move_cycle < before;
            if crossed && t.blend_speed > 40.0 {
                crate::audio::cue(&mut commands, crate::audio::Cue::BtStep, None);
            }
        }
        if let Some(c) = idle {
            t.idle_cycle = (t.idle_cycle + dt / actor.clips[c].duration).rem_euclid(1.0);
        }

        let mut layers = Vec::new();
        if let Some(c) = idle {
            layers.push(Layer { clip: c, cycle: t.idle_cycle, weight: 1.0 - moving });
        }
        for (i, id) in ids.iter().enumerate() {
            if let Some(c) = id {
                layers.push(Layer { clip: *c, cycle: t.move_cycle, weight: moving * w[i] * (1.0 - if i == 0 { sprint_w } else { 0.0 }) });
            }
        }
        if let Some(c) = sprint {
            layers.push(Layer { clip: c, cycle: t.move_cycle, weight: moving * sprint_w });
        }
        // A punch plays over everything else.
        if let (Some(mt), Some(c)) = (melee.and_then(|m| m.t), actor.clip("at_player_melee_punch_01")) {
            let k = (mt / 0.1).min(1.0) * ((crate::combat::MELEE_ANIM - mt) / 0.15).clamp(0.0, 1.0);
            for l in &mut layers {
                l.weight *= 1.0 - k;
            }
            layers.push(Layer { clip: c, cycle: (mt / actor.clips[c].duration.max(1e-3)).min(0.999), weight: k.max(1e-3) });
        }
        actor.layers = layers;

        // The aim matrix folds the shoulder pods and points the gun arm at the view pitch.
        // Source pitch is positive looking down; aim_pitch is positive aiming up.
        let grid = if moving > 0.5 { "combat_aim_run" } else { "combat_aim_stand" };
        let no_aim = std::env::var_os("TF_NO_AIM").is_some();
        actor.aim = (!no_aim).then(|| Aim { grid: grid.to_string(), yaw: 0.0, pitch: -s.pitch.to_degrees(), weight: 1.0 - t.sprint_blend });
    }
}

#[derive(Component)]
pub struct MainCamera;

pub fn update_camera(
    mode: Res<CameraMode>,
    control: Res<crate::pilotctl::Control>,
    world: Res<Collision>,
    settings: Res<TitanSettings>,
    punch: Res<crate::weapons::ViewPunch>,
    mods: Res<crate::titankit::TitanMoveMods>,
    titans: Query<(&PlayerTitan, Option<&crate::weapons::Weapon>, Option<&crate::combat::TitanHealth>)>,
    actors: Query<&Actor>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<MainCamera>, Without<PlayerTitan>)>,
    mut vis: Query<&mut Visibility>,
    mut cockpit_tf: Query<&mut Transform, (Without<MainCamera>, Without<PlayerTitan>, Without<Camera>)>,
    (time, mut chase, mut sway, mut tilt): (Res<Time>, Local<f32>, Local<crate::vmmotion::CockpitSway>, Local<DashTilt>),
    (damage_from, mut last_hp): (Res<crate::hud::DamageFrom>, Local<Option<f32>>),
) {
    if *mode == CameraMode::Free || *control != crate::pilotctl::Control::Titan {
        *last_hp = None;
        return;
    }
    let Ok((t, weapon, health)) = titans.single() else { return };
    let Ok((mut cam, mut projection)) = cameras.single_mut() else { return };
    let s = &t.state;
    let p = &settings.0;
    let dash_roll = tilt.step(s, time.delta_secs());
    let (yaw, pitch) = (s.yaw + punch.yaw.to_radians(), s.pitch + punch.pitch.to_radians());
    let dir = Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin());
    // ADS zoom: the game's FOVs are horizontal at 4:3 (75 hip); convert the ratio to our
    // vertical 60 degree base.
    if let (Some(w), Projection::Perspective(pp)) = (weapon, projection.as_mut()) {
        let base = 60f32.to_radians();
        let ratio = (w.def.zoom_fov.to_radians() * 0.5).tan() / (75f32.to_radians() * 0.5).tan();
        let zoomed = 2.0 * ((base * 0.5).tan() * ratio).atan();
        let ads = w.ads * w.ads * (3.0 - 2.0 * w.ads);
        pp.fov = base + (zoomed - base) * ads;
    }
    let cockpit = *mode == CameraMode::Cockpit;
    if let Ok(mut v) = vis.get_mut(t.actor) {
        // Phased (Phase Dash) BT isn't drawn either.
        *v = if cockpit || mods.phased { Visibility::Hidden } else { Visibility::Inherited };
    }
    // Cockpit step jolt (titan_buddy.set: cockpit_stepJolt_origin "0 0 -5"): dip twice per
    // walk cycle, once per footstep, scaled by how much BT is moving.
    let moving = (t.blend_speed / p.speed).clamp(0.0, 1.5);
    let jolt = Vec3::Z * (-5.0 * moving * (t.move_cycle * std::f32::consts::TAU * 2.0).sin().abs());
    let eye = bv(s.eye(p)) + jolt;
    if let Some((c, c_actor)) = t.cockpit {
        if let Ok(mut v) = vis.get_mut(c) {
            *v = if cockpit { Visibility::Inherited } else { Visibility::Hidden };
        }
        // Put the cockpit's camera bone on the eye and turn the cockpit around it.
        let cam_bone = actors.get(c_actor).ok().and_then(|a| a.bone_model_transform("jx_c_camera"));
        let offset = cam_bone.map(|b| b.translation).unwrap_or(Vec3::ZERO);
        // The cockpit lags behind turns and movement (cockpitSway*, vmmotion.rs), turning
        // about the eye.
        let (cy, sy) = (s.yaw.cos(), s.yaw.sin());
        let vel = Vec3::new(s.vel.x * cy + s.vel.y * sy, -s.vel.x * sy + s.vel.y * cy, s.vel.z);
        sway.step(&crate::vmmotion::CockpitSwayDef::TITAN, s.yaw, s.pitch, vel, time.delta_secs());
        // Damage (hull or shield) jolts the cockpit away from where it came from
        // (TitanCockpit_DamageFeedback: joltDir = eye - damageOrigin, damage / 2000).
        let hp = health.map(|h| h.v.health + h.v.shield);
        if let (Some(prev), Some(now)) = (*last_hp, hp) {
            if now < prev - 0.5 {
                let eye_w = bv(s.eye(p));
                // The newest damage direction (most time left); straight ahead if none.
                let from = damage_from.0.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|d| d.0);
                let dir_w = from.map(|f| (eye_w - f).normalize_or_zero()).unwrap_or(Vec3::new(-cy, -sy, 0.0));
                let dir = Vec3::new(dir_w.x * cy + dir_w.y * sy, -dir_w.x * sy + dir_w.y * cy, dir_w.z);
                sway.jolt(dir, (prev - now) / 2000.0);
            }
        }
        *last_hp = hp;
        if let Ok(mut ctf) = cockpit_tf.get_mut(c) {
            let rot = Quat::from_rotation_z(s.yaw) * Quat::from_rotation_y(s.pitch);
            let rs = crate::vmmotion::angles_quat(sway.angles) * Quat::from_rotation_x(dash_roll * DODGE_COCKPIT_TILT_MAX / DODGE_VIEW_TILT_MAX);
            ctf.rotation = rot * rs;
            ctf.translation = bv(s.eye(p)) + jolt - rot * rs * offset + rot * sway.origin;
        }
    }
    let pos = if cockpit {
        eye
    } else {
        // Over-the-shoulder third person, pulled in if geometry is in the way.
        let right = Vec3::new(s.yaw.sin(), -s.yaw.cos(), 0.0);
        let pivot = bv(s.pos) + Vec3::Z * (p.height * 0.95) + right * 90.0;
        let back = -dir * 520.0;
        let dist = world
            .0
            .raycast(sv(pivot), sv(back.normalize()), back.length())
            .map(|h| (h.t - 20.0).max(40.0))
            .unwrap_or(back.length());
        // Snap in when geometry gets in the way, ease back out: a ray grazing an edge
        // otherwise flips the camera between two distances every frame.
        let cur = if *chase <= 0.0 { dist } else { *chase };
        let dt = time.delta_secs().min(0.1);
        *chase = if dist < cur { dist } else { (cur + 900.0 * dt).min(dist) };
        pivot + back.normalize() * *chase
    };
    *cam = Transform::from_translation(to_bevy(pos)).looking_to(to_bevy(dir).normalize(), Vec3::Y);
    // Recoil roll (positive rolls the view right): about the view axis.
    cam.rotate_local_z(-punch.roll.to_radians() - if cockpit { dash_roll } else { 0.0 });
    log::trace!("titan pos {:?} yaw {:.1} cam {:?} dir {:?}", s.pos, s.yaw.to_degrees(), pos, dir);
}

/// Dash view tilt (client.dll convars): the view rolls toward the dash's sideways direction, up
/// to dodge_viewTiltMax (10 degrees) at dodge_viewTiltIncreaseSpeed 5, held for
/// dodge_viewTiltFalloffTime 0.7 s, then back at dodge_viewTiltDecreaseSpeed 2.5; the cockpit
/// model tilts by dodge_cockpitTiltMax (4). Speeds are treated as exponential rates and the
/// direction (into the dash) is a guess.
const DODGE_VIEW_TILT_MAX: f32 = 10.0;
const DODGE_COCKPIT_TILT_MAX: f32 = 4.0;

#[derive(Default)]
pub struct DashTilt {
    dashes: u32,
    side: f32,
    since: f32,
    frac: f32,
}

impl DashTilt {
    /// Roll in radians, positive to the right.
    fn step(&mut self, s: &tf_sim::titan::TitanState, dt: f32) -> f32 {
        if s.dashes != self.dashes {
            self.dashes = s.dashes;
            let right = tf_sim::glam::Vec2::new(s.yaw.sin(), -s.yaw.cos());
            let h = tf_sim::glam::Vec2::new(s.vel.x, s.vel.y).normalize_or_zero();
            self.side = h.dot(right);
            self.since = 0.0;
        }
        self.since += dt;
        let (want, rate) = if self.since < 0.7 { (self.side, 5.0) } else { (0.0, 2.5) };
        self.frac += (want - self.frac) * (1.0 - (-dt * rate).exp());
        self.frac * DODGE_VIEW_TILT_MAX.to_radians()
    }
}

/// Outside the Titan, the Titan's own inputs (guns, abilities) are ignored.
pub fn route_input(control: Res<crate::pilotctl::Control>, mut input: ResMut<PlayerInput>) {
    if *control != crate::pilotctl::Control::Pilot {
        input.pilot_fire = false;
        input.pilot_ads = false;
        input.pilot_reload = false;
    }
    if *control != crate::pilotctl::Control::Titan {
        input.fire = false;
        input.ads = false;
        input.reload = false;
        input.ordnance = false;
        input.core = false;
        input.vortex = false;
        input.utility = false;
        // melee stays: on foot it is the Pilot's punch / execution (executions.rs).
    }
}

/// BT's cockpit damage light (cl_titan_cockpit.nut FlashCockpitLight at `SCR_CL_BL`): losing a
/// health segment pulses a red light (1, 0.06, 0), radius 70, for 3 s (LostChunk_DamageFeedback);
/// doomed, a dimmer red (0.6, 0.06, 0) pulses until BT isn't doomed (TitanCockpitDoomedThink).
/// The pulse is 0.5..1.5 at rate 3 (GetPulseFrac + 0.5). The light reaches only the
/// first-person layer, as SetCockpitLight does; its brightness is by eye.
#[derive(Component)]
pub struct CockpitDamageLight;

pub fn cockpit_damage_light(
    mut commands: Commands,
    time: Res<Time>,
    control: Res<crate::pilotctl::Control>,
    titans: Query<(&PlayerTitan, &crate::combat::TitanHealth)>,
    actors: Query<&Actor>,
    globals: Query<&GlobalTransform>,
    mut lights: Query<(Entity, &mut PointLight, &mut Transform), With<CockpitDamageLight>>,
    mut st: Local<(Option<u32>, f32)>,
    mut sparks: Local<(Option<(f32, bool)>, u32)>,
) {
    let dt = time.delta_secs();
    let Ok((t, health)) = titans.single() else { return };
    cockpit_sparks(&mut commands, t, health, &actors, &globals, *control == crate::pilotctl::Control::Titan, &mut sparks);
    let segment = (health.v.health / crate::combat::SEGMENT).ceil().max(0.0) as u32;
    if st.0.is_some_and(|s| segment < s) && health.dead_for.is_none() {
        st.1 = 3.0;
    }
    st.0 = Some(segment);
    st.1 = (st.1 - dt).max(0.0);
    let doomed = health.v.doomed.is_some() && health.dead_for.is_none();
    let color = if st.1 > 0.0 {
        Some(Vec3::new(1.0, 0.06, 0.0))
    } else if doomed {
        Some(Vec3::new(0.6, 0.06, 0.0))
    } else {
        None
    };
    // BT's cockpit model has no SCR_CL_BL. The model's origin (what a failed lookup gives) is
    // at BT's feet, far out of the light's reach, so use the bottom-left screen (SCR_BL_BL).
    // SCR_BL_BL is on def_c_intBase at (15.6, -1.3, 17.3) (attachdump).
    let at = t.cockpit.and_then(|(_, a)| {
        let a = actors.get(a).ok()?;
        a.joint("def_c_intBase").and_then(|j| globals.get(j).ok()).map(|g| g.transform_point(Vec3::new(15.6, -1.3, 17.3)))
    });
    let (Some(c), Some(at), true) = (color, at, *control == crate::pilotctl::Control::Titan) else {
        for (e, ..) in &lights {
            commands.entity(e).despawn();
        }
        return;
    };
    let pulse = 0.5 + 0.5 * (1.0 - (time.elapsed_secs() * 3.0 * std::f32::consts::TAU).cos());
    let rgb = c * pulse;
    let peak = rgb.max_element().max(1e-3);
    let light = PointLight { color: Color::linear_rgb(rgb.x / peak, rgb.y / peak, rgb.z / peak), intensity: 150_000.0 * peak, range: 70.0 * UNIT, shadows_enabled: false, ..default() };
    if let Ok((_, mut l, mut tf)) = lights.single_mut() {
        *l = light;
        tf.translation = at;
    } else {
        commands.spawn((light, Transform::from_translation(at), bevy::camera::visibility::RenderLayers::layer(crate::vmcam::VM_LAYER), CockpitDamageLight));
    }
}

/// BT's cockpit FX panels (attachdump of pov_titan_medium_cockpit.mdl): all on def_c_intBase,
/// position and x/y/z axes in the bone's frame.
const COCKPIT_FX_PANELS: [([f32; 3], [[f32; 3]; 3]); 6] = [
    ([14.4, 19.6, 13.8], [[-0.63, -0.71, -0.31], [-0.77, 0.62, 0.15], [0.08, 0.33, -0.94]]), // FX_TL_PANEL
    ([-10.3, 19.6, 19.6], [[0.25, -0.86, -0.44], [-0.97, -0.21, -0.13], [0.02, 0.46, -0.89]]), // FX_TR_PANEL
    ([-3.5, 19.5, 16.6], [[-0.18, -0.87, -0.45], [-0.98, 0.17, 0.07], [0.02, 0.46, -0.89]]), // FX_TC_PANELA
    ([5.1, 20.4, 16.6], [[0.19, -0.72, -0.66], [-0.98, -0.17, -0.11], [-0.03, 0.67, -0.74]]), // FX_TC_PANELB
    ([17.3, 5.5, 11.0], [[-0.97, 0.24, -0.00], [0.23, 0.91, 0.34], [0.08, 0.33, -0.94]]), // FX_BL_PANEL
    ([-14.7, 7.6, 15.7], [[0.85, 0.40, -0.33], [0.47, -0.87, 0.15], [-0.23, -0.29, -0.93]]), // FX_BR_PANEL
];

/// Cockpit sparks on hull damage (cl_titan_cockpit.nut CalSparkCountForHit/PlayCockpitSparkFX):
/// three `xo_cockpit_spark_01` per 1000 health lost across a thousand mark (none for the first
/// hit from full), 20 on becoming doomed, spread over the six FX panels in a shuffled order.
fn cockpit_sparks(
    commands: &mut Commands,
    t: &PlayerTitan,
    health: &crate::combat::TitanHealth,
    actors: &Query<&Actor>,
    globals: &Query<&GlobalTransform>,
    in_cockpit: bool,
    st: &mut (Option<(f32, bool)>, u32),
) {
    let (now, doomed) = (health.v.health, health.v.doomed.is_some());
    let Some((prev, was_doomed)) = st.0.replace((now, doomed)) else { return };
    let count = if doomed && !was_doomed {
        20
    } else if now < prev && !doomed {
        let full = prev >= health.v.max_health - 0.5;
        let (a, b) = ((now / 1000.0).floor() as i32, (prev / 1000.0).floor() as i32 - if full { 1 } else { 0 });
        ((b - a).max(0) * 3) as u32
    } else {
        0
    };
    if count == 0 || !in_cockpit || health.dead_for.is_some() {
        return;
    }
    let Some(base) = t.cockpit.and_then(|(_, a)| actors.get(a).ok()).and_then(|a| a.joint("def_c_intBase")).and_then(|j| globals.get(j).ok()) else { return };
    // A shuffled panel order per hit.
    st.1 = st.1.wrapping_mul(1664525).wrapping_add(1013904223);
    let mut order = [0usize, 1, 2, 3, 4, 5];
    for i in (1..6).rev() {
        let j = ((st.1 >> (i * 3)) as usize) % (i + 1);
        order.swap(i, j);
    }
    for i in 0..count as usize {
        let (p, [x, y, z]) = COCKPIT_FX_PANELS[order[i % 6]];
        let local = Quat::from_mat3(&Mat3::from_cols(Vec3::from(x), Vec3::from(y), Vec3::from(z))).normalize();
        crate::pfx::emit_named_cockpit(commands, "xo_cockpit_spark_01", base.transform_point(Vec3::from(p)), base.rotation() * local);
    }
}
