//! Weapon feel from the weapon scripts: recoil (`viewkick_*` with the `scripts/weapons/
//! springs.txt` spring named by `viewkick_spring`), viewmodel sway (`sway_*`) and bob
//! (`bob_*`). The engine code behind these keys is compiled C++, so how the numbers combine
//! is reconstructed (see README, Weapon feel); the numbers themselves are the scripts'.
//!
//! Angles are degrees in Source's convention (pitch down, yaw left, roll right positive);
//! viewmodel translations are in the view's frame (x forward, y left, z up), in inches.

use bevy::prelude::*;
use std::collections::HashMap;
use tf_assets::settings::PlayerSettings;

/// A weapon script value: the `sp_base` block's copy if there is one, else the top level.
fn val(s: &PlayerSettings, k: &str) -> Option<f32> {
    s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).and_then(|v| v.trim().parse().ok())
}

/// One damped spring per axis (pitch, yaw, roll): `springConstant` and `damping`.
#[derive(Clone, Copy, Debug)]
pub struct SpringAxes {
    pub k: Vec3,
    pub c: Vec3,
}

impl Default for SpringAxes {
    /// Source's view punch spring (PUNCH_SPRING_CONSTANT 65, PUNCH_DAMPING 9).
    fn default() -> Self {
        Self { k: Vec3::splat(65.0), c: Vec3::splat(9.0) }
    }
}

/// `scripts/weapons/springs.txt`: hip-fire and ADS springs per name.
#[derive(Resource, Default, Clone)]
pub struct WeaponSprings(pub HashMap<String, (SpringAxes, SpringAxes)>);

impl WeaponSprings {
    pub fn load(read: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let Some(s) = PlayerSettings::load("scripts/weapons/springs.txt", true, read) else {
            log::warn!("scripts/weapons/springs.txt not found; recoil uses Source's punch spring");
            return Self::default();
        };
        let mut names: Vec<String> = s.values.keys().filter_map(|k| k.split_once('.').map(|(n, _)| n.to_string())).filter(|n| !n.is_empty()).collect();
        names.sort();
        names.dedup();
        let mut out = HashMap::new();
        for n in names {
            let axes = |mode: &str| {
                let g = |axis: &str, what: &str, d: f32| s.f32(&format!("{n}.{mode}_{axis}_{what}"), d);
                SpringAxes {
                    k: Vec3::new(g("pitch", "springconstant", 65.0), g("yaw", "springconstant", 65.0), g("roll", "springconstant", 65.0)),
                    c: Vec3::new(g("pitch", "damping", 9.0), g("yaw", "damping", 9.0), g("roll", "damping", 9.0)),
                }
            };
            out.insert(n.clone(), (axes("hipfire"), axes("ads")));
        }
        log::info!("weapon springs: {}", out.len());
        Self(out)
    }
}

/// The `viewkick_*` keys.
#[derive(Clone, Debug, Default)]
pub struct KickDef {
    pub spring: String,
    /// (base, random, softScale, hardScale) for pitch and yaw.
    pub pitch: [f32; 4],
    pub yaw: [f32; 4],
    pub yaw_inner_exclude: f32,
    /// (base, randomMin, randomMax, softScale, hardScale).
    pub roll: [f32; 5],
    /// (weaponFraction, weaponFraction_vmScale) for hip fire and ADS.
    pub hip_fraction: (f32, f32),
    pub ads_fraction: (f32, f32),
    pub first_hip: Option<f32>,
    pub first_ads: Option<f32>,
    pub scale_hip: (f32, f32),
    pub scale_ads: (f32, f32),
    pub per_shot: f32,
    pub lerp: (f32, f32),
    pub decay_delay: f32,
    pub decay_rate: f32,
    /// Permanent aim change per shot (`viewkick_perm_*`): (base, random) pitch and yaw.
    pub perm_pitch: (f32, f32),
    pub perm_yaw: (f32, f32),
    pub duck_scale: f32,
    /// `viewmodel_shake_forward`, `_up`, `_right`: how far a shot jolts the gun (units).
    pub vm_shake: Vec3,
}

impl KickDef {
    pub fn from_settings(s: &PlayerSettings) -> Self {
        let f = |k: &str, d: f32| val(s, k).unwrap_or(d);
        Self {
            spring: s.get("sp_base.viewkick_spring").or_else(|| s.get(".viewkick_spring")).unwrap_or("").trim().to_ascii_lowercase(),
            pitch: [f("viewkick_pitch_base", 0.0), f("viewkick_pitch_random", 0.0), f("viewkick_pitch_softscale", 1.0), f("viewkick_pitch_hardscale", 0.0)],
            yaw: [f("viewkick_yaw_base", 0.0), f("viewkick_yaw_random", 0.0), f("viewkick_yaw_softscale", 1.0), f("viewkick_yaw_hardscale", 0.0)],
            yaw_inner_exclude: f("viewkick_yaw_random_innerexclude", 0.0),
            roll: [f("viewkick_roll_base", 0.0), f("viewkick_roll_randommin", 0.0), f("viewkick_roll_randommax", 0.0), f("viewkick_roll_softscale", 1.0), f("viewkick_roll_hardscale", 0.0)],
            hip_fraction: (f("viewkick_hipfire_weaponfraction", 0.0), f("viewkick_hipfire_weaponfraction_vmscale", 1.0)),
            ads_fraction: (f("viewkick_ads_weaponfraction", 0.0), f("viewkick_ads_weaponfraction_vmscale", 1.0)),
            first_hip: val(s, "viewkick_scale_firstshot_hipfire"),
            first_ads: val(s, "viewkick_scale_firstshot_ads"),
            scale_hip: (f("viewkick_scale_min_hipfire", 1.0), f("viewkick_scale_max_hipfire", 1.0)),
            scale_ads: (f("viewkick_scale_min_ads", 1.0), f("viewkick_scale_max_ads", 1.0)),
            per_shot: f("viewkick_scale_valuepershot", 1.0),
            lerp: (f("viewkick_scale_valuelerpstart", 0.0), f("viewkick_scale_valuelerpend", 1.0)),
            decay_delay: f("viewkick_scale_valuedecaydelay", 0.2),
            decay_rate: f("viewkick_scale_valuedecayrate", 10.0),
            perm_pitch: (f("viewkick_perm_pitch_base", 0.0), f("viewkick_perm_pitch_random", 0.0)),
            perm_yaw: (f("viewkick_perm_yaw_base", 0.0), f("viewkick_perm_yaw_random", 0.0)),
            duck_scale: f("viewkick_duck_scale", 1.0),
            vm_shake: Vec3::new(f("viewmodel_shake_forward", 0.0), f("viewmodel_shake_up", 0.0), f("viewmodel_shake_right", 0.0)),
        }
    }
}

/// How fast a shot's viewmodel jolt dies away (per second).
const VM_SHAKE_DECAY: f32 = 25.0;

/// Recoil state: the view's punch (added to the aim) and the viewmodel's, each a damped
/// spring per axis, and the per-shot kick scale.
#[derive(Resource, Default, Clone)]
pub struct ViewPunch {
    /// View punch angles (degrees): added to the player's aim.
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    vel: Vec3,
    /// Viewmodel kick angles (pitch, yaw, roll) and their spring velocity.
    pub vm: Vec3,
    vm_vel: Vec3,
    springs: (SpringAxes, SpringAxes),
    /// How zoomed the gun was at the last shot (blends the hip and ADS springs).
    ads: f32,
    /// `viewkick_scale_value` and seconds since the last shot.
    value: f32,
    since_shot: f32,
    /// Permanent aim change not yet taken by the aim (pitch, yaw), degrees.
    pub perm: Vec2,
    /// The gun's shot jolt (view frame: forward, left, up), decaying.
    pub vm_shake: Vec3,
}

impl ViewPunch {
    /// One shot's kick, times `extra` (a charge level's scale). `r` are four uniform random
    /// numbers in 0..1.
    pub fn kick(&mut self, def: &KickDef, springs: &WeaponSprings, ads: f32, crouched: bool, extra: f32, r: [f32; 4]) {
        self.springs = springs.0.get(&def.spring).copied().unwrap_or_default();
        self.ads = ads;
        // Kick scale ramps from min to max as shots pile up (value per shot, lerp window).
        let t = ((self.value - def.lerp.0) / (def.lerp.1 - def.lerp.0).max(1e-3)).clamp(0.0, 1.0);
        let first = self.value <= 0.0;
        let hip = if first { def.first_hip.unwrap_or(def.scale_hip.0) } else { def.scale_hip.0 + (def.scale_hip.1 - def.scale_hip.0) * t };
        let ads_s = if first { def.first_ads.unwrap_or(def.scale_ads.0) } else { def.scale_ads.0 + (def.scale_ads.1 - def.scale_ads.0) * t };
        let mut scale = hip + (ads_s - hip) * ads;
        if crouched {
            scale *= def.duck_scale;
        }
        scale *= extra;
        self.value += def.per_shot;
        self.since_shot = 0.0;
        let signed = |u: f32| u * 2.0 - 1.0;
        let pitch = def.pitch[0] + def.pitch[1] * signed(r[0]);
        // Yaw: random magnitude outside the inner exclusion, random side.
        let mag = def.yaw_inner_exclude + (1.0 - def.yaw_inner_exclude) * r[1];
        let yaw = def.yaw[0] + def.yaw[1] * mag * if r[2] < 0.5 { -1.0 } else { 1.0 };
        let roll = def.roll[0] + (def.roll[1] + (def.roll[2] - def.roll[1]) * r[3]) * if r[1] < 0.5 { -1.0 } else { 1.0 };
        let kick = Vec3::new(pitch, yaw, roll) * scale;
        let soft = kick * Vec3::new(def.pitch[2], def.yaw[2], def.roll[3]);
        let hard = kick * Vec3::new(def.pitch[3], def.yaw[3], def.roll[4]);
        // Part of the kick moves the gun instead of the view (weaponFraction), scaled for the
        // viewmodel by its vmScale.
        let frac = def.hip_fraction.0 + (def.ads_fraction.0 - def.hip_fraction.0) * ads;
        let vm_scale = def.hip_fraction.1 + (def.ads_fraction.1 - def.hip_fraction.1) * ads;
        let view = 1.0 - frac;
        // Hard kick moves the angle at once; soft kick pushes the spring (Source's ViewPunch:
        // velocity += angle * 20).
        self.pitch += hard.x * view;
        self.yaw += hard.y * view;
        self.roll += hard.z * view;
        self.vel += soft * view * 20.0;
        self.vm += hard * frac * vm_scale;
        self.vm_vel += soft * frac * vm_scale * 20.0;
        log::debug!("kick x{scale:.2} ({:.2} {:.2} {:.2}) view now ({:.2} {:.2} {:.2}) vel {:.1?} vm {:.2?}", kick.x, kick.y, kick.z, self.pitch, self.yaw, self.roll, self.vel, self.vm);
        // Viewmodel shake: back along the barrel, and a random nudge up and sideways.
        self.vm_shake = Vec3::new(-def.vm_shake.x * (0.5 + 0.5 * r[3]), def.vm_shake.z * signed(r[1]), def.vm_shake.y * signed(r[2]));
        self.perm += Vec2::new(def.perm_pitch.0 + def.perm_pitch.1 * signed(r[0]), def.perm_yaw.0 + def.perm_yaw.1 * signed(r[2]));
    }

    /// Spring the punch back and decay the kick scale.
    pub fn step(&mut self, dt: f32, def: Option<&KickDef>) {
        self.since_shot += dt;
        let (delay, rate) = def.map(|d| (d.decay_delay, d.decay_rate)).unwrap_or((0.2, 10.0));
        if self.since_shot > delay {
            self.value = (self.value - rate * dt).max(0.0);
        }
        let (h, a) = self.springs;
        let k = h.k.lerp(a.k, self.ads);
        let c = h.c.lerp(a.c, self.ads);
        // Stiff roll springs (k up to 28000) need small steps.
        let n = ((dt * 1000.0).ceil() as usize).clamp(1, 200);
        let h = dt / n as f32;
        let mut ang = Vec3::new(self.pitch, self.yaw, self.roll);
        for _ in 0..n {
            self.vel += (-k * ang - c * self.vel) * h;
            ang += self.vel * h;
            self.vm_vel += (-k * self.vm - c * self.vm_vel) * h;
            self.vm += self.vm_vel * h;
        }
        (self.pitch, self.yaw, self.roll) = (ang.x, ang.y, ang.z);
        self.vm_shake *= (-VM_SHAKE_DECAY * dt).exp();
    }
}

/// One set of `sway_*` values (hip or `_zoomed`).
#[derive(Clone, Debug, Default)]
pub struct SwaySet {
    pub attach: String,
    pub min_t: Vec3,
    pub max_t: Vec3,
    /// (pitch, yaw, roll)
    pub min_r: Vec3,
    pub max_r: Vec3,
    pub t_gain: f32,
    pub r_gain: f32,
    /// Per direction: forward, back, left, right, up, down.
    pub move_t: [Vec3; 6],
    pub move_r: [Vec3; 6],
    /// Per direction: left, right, up, down.
    pub turn_t: [Vec3; 4],
    pub turn_r: [Vec3; 4],
}

/// One set of `bob_*` values.
#[derive(Clone, Debug, Default)]
pub struct BobSet {
    pub cycle: f32,
    pub vert: f32,
    pub horz: f32,
    pub max_speed: f32,
    /// (pitch, yaw, roll)
    pub angles: Vec3,
}

#[derive(Clone, Debug, Default)]
pub struct MotionDef {
    pub hip: SwaySet,
    pub zoomed: SwaySet,
    pub bob: BobSet,
    pub bob_zoomed: BobSet,
}

impl MotionDef {
    pub fn from_settings(s: &PlayerSettings) -> Self {
        let sway = |suffix: &str, fallback: Option<&SwaySet>| -> SwaySet {
            // Zoomed keys a script leaves out are 0, except gains and the pivot, which keep
            // the hip values (guess: ADS stays steady unless a script asks for sway).
            let g = |k: &str| val(s, &format!("{k}{suffix}")).unwrap_or(0.0);
            let v3 = |a: &str, b: &str, c: &str| Vec3::new(g(a), g(b), g(c));
            let dir_t = |d: &str| Vec3::new(g(&format!("sway_{d}_translate_x")), g(&format!("sway_{d}_translate_y")), g(&format!("sway_{d}_translate_z")));
            let dir_r = |d: &str| Vec3::new(g(&format!("sway_{d}_rotate_pitch")), g(&format!("sway_{d}_rotate_yaw")), g(&format!("sway_{d}_rotate_roll")));
            let gain = |k: &str, d: f32| val(s, &format!("{k}{suffix}")).unwrap_or(fallback.map(|f| if k.contains("translate") { f.t_gain } else { f.r_gain }).unwrap_or(d));
            let attach = s
                .get(&format!("sp_base.sway_rotate_attach{suffix}"))
                .or_else(|| s.get(&format!(".sway_rotate_attach{suffix}")))
                .map(|a| a.trim().to_string())
                .or_else(|| fallback.map(|f| f.attach.clone()))
                .unwrap_or_default();
            SwaySet {
                attach,
                min_t: v3("sway_min_x", "sway_min_y", "sway_min_z"),
                max_t: v3("sway_max_x", "sway_max_y", "sway_max_z"),
                min_r: v3("sway_min_pitch", "sway_min_yaw", "sway_min_roll"),
                max_r: v3("sway_max_pitch", "sway_max_yaw", "sway_max_roll"),
                t_gain: gain("sway_translate_gain", 5.0),
                r_gain: gain("sway_rotate_gain", 5.0),
                move_t: ["move_forward", "move_back", "move_left", "move_right", "move_up", "move_down"].map(dir_t),
                move_r: ["move_forward", "move_back", "move_left", "move_right", "move_up", "move_down"].map(dir_r),
                turn_t: ["turn_left", "turn_right", "turn_up", "turn_down"].map(dir_t),
                turn_r: ["turn_left", "turn_right", "turn_up", "turn_down"].map(dir_r),
            }
        };
        let hip = sway("", None);
        let zoomed = sway("_zoomed", Some(&hip));
        let bob = |suffix: &str, cycle_fallback: f32| BobSet {
            cycle: val(s, &format!("bob_cycle_time{suffix}")).unwrap_or(cycle_fallback),
            vert: val(s, &format!("bob_vert_dist{suffix}")).unwrap_or(0.0),
            horz: val(s, &format!("bob_horz_dist{suffix}")).unwrap_or(0.0),
            max_speed: val(s, &format!("bob_max_speed{suffix}")).unwrap_or(150.0),
            angles: Vec3::new(
                val(s, &format!("bob_pitch{suffix}")).unwrap_or(0.0),
                val(s, &format!("bob_yaw{suffix}")).unwrap_or(0.0),
                val(s, &format!("bob_roll{suffix}")).unwrap_or(0.0),
            ),
        };
        let b = bob("", 0.4);
        let bz = bob("_zoomed", b.cycle);
        Self { hip, zoomed, bob: b, bob_zoomed: bz }
    }
}

/// What the player's body is doing, for sway and bob.
pub struct MotionInput {
    /// View yaw and pitch (radians) this frame.
    pub yaw: f32,
    pub pitch: f32,
    /// Velocity in the view's yaw frame: forward, left, up (units/s).
    pub vel: Vec3,
    pub on_ground: bool,
    /// Bob is off while the sprint animation plays.
    pub bob: bool,
    pub ads: f32,
}

/// Sway and bob state of one viewmodel.
#[derive(Clone, Default)]
pub struct VmMotion {
    last: Option<(f32, f32)>,
    t: Vec3,
    r: Vec3,
    bob_phase: f32,
    bob_amount: f32,
}

/// The viewmodel offset to apply about its sway pivot: rotation (pitch, yaw, roll degrees)
/// and translation (view frame).
pub struct VmOffset {
    pub rot: Vec3,
    pub trans: Vec3,
}

/// Turn rate (degrees/s) that counts as a full `sway_turn_*` push. The engine's scale isn't
/// in the scripts; chosen so ordinary mouse turns reach the clamps.
const TURN_FULL: f32 = 150.0;
/// Speeds that count as a full `sway_move_*` push: the viewmodels' velocity pose parameter
/// range (0..173) horizontally, a jump's launch speed vertically.
const MOVE_FULL: Vec3 = Vec3::new(173.0, 173.0, 300.0);

impl VmMotion {
    pub fn step(&mut self, def: &MotionDef, inp: &MotionInput, dt: f32) -> VmOffset {
        let dt = dt.max(1e-4);
        let (dyaw, dpitch) = match self.last {
            Some((y, p)) => {
                let mut dy = inp.yaw - y;
                dy = (dy + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                (dy.to_degrees() / dt, (inp.pitch - p).to_degrees() / dt)
            }
            None => (0.0, 0.0),
        };
        self.last = Some((inp.yaw, inp.pitch));
        let lerp = |a: Vec3, b: Vec3| a.lerp(b, inp.ads);
        let (h, z) = (&def.hip, &def.zoomed);
        // Pushes: positive yaw rate turns left; positive pitch rate looks down.
        let turn = [(dyaw / TURN_FULL).clamp(0.0, 1.0), (-dyaw / TURN_FULL).clamp(0.0, 1.0), (-dpitch / TURN_FULL).clamp(0.0, 1.0), (dpitch / TURN_FULL).clamp(0.0, 1.0)];
        let m = inp.vel / MOVE_FULL;
        let mv = [m.x.clamp(0.0, 1.0), (-m.x).clamp(0.0, 1.0), m.y.clamp(0.0, 1.0), (-m.y).clamp(0.0, 1.0), m.z.clamp(0.0, 1.0), (-m.z).clamp(0.0, 1.0)];
        let mut target_t = Vec3::ZERO;
        let mut target_r = Vec3::ZERO;
        for i in 0..6 {
            target_t += lerp(h.move_t[i], z.move_t[i]) * mv[i];
            target_r += lerp(h.move_r[i], z.move_r[i]) * mv[i];
        }
        for i in 0..4 {
            target_t += lerp(h.turn_t[i], z.turn_t[i]) * turn[i];
            target_r += lerp(h.turn_r[i], z.turn_r[i]) * turn[i];
        }
        target_t = target_t.clamp(lerp(h.min_t, z.min_t), lerp(h.max_t, z.max_t));
        target_r = target_r.clamp(lerp(h.min_r, z.min_r), lerp(h.max_r, z.max_r));
        let tg = h.t_gain + (z.t_gain - h.t_gain) * inp.ads;
        let rg = h.r_gain + (z.r_gain - h.r_gain) * inp.ads;
        self.t += (target_t - self.t) * (1.0 - (-tg * dt).exp());
        self.r += (target_r - self.r) * (1.0 - (-rg * dt).exp());

        // Bob: one vertical cycle per bob_cycle_time, the sideways sway and the angles at
        // half that rate, scaled by speed up to bob_max_speed.
        let b = &def.bob;
        let bz = &def.bob_zoomed;
        let cycle = (b.cycle + (bz.cycle - b.cycle) * inp.ads).max(0.05);
        let max_speed = (b.max_speed + (bz.max_speed - b.max_speed) * inp.ads).max(1.0);
        let speed = Vec2::new(inp.vel.x, inp.vel.y).length();
        let want = if inp.on_ground && inp.bob { (speed / max_speed).min(1.0) } else { 0.0 };
        self.bob_amount += (want - self.bob_amount) * (1.0 - (-10.0 * dt).exp());
        self.bob_phase = (self.bob_phase + dt / cycle * std::f32::consts::TAU).rem_euclid(std::f32::consts::TAU * 2.0);
        let vert = b.vert + (bz.vert - b.vert) * inp.ads;
        let horz = b.horz + (bz.horz - b.horz) * inp.ads;
        let ang = b.angles.lerp(bz.angles, inp.ads);
        let (v, s) = (self.bob_phase.sin(), (self.bob_phase * 0.5).sin());
        let a = self.bob_amount;
        let bob_t = Vec3::new(0.0, horz * s, vert * v) * a;
        let bob_r = Vec3::new(ang.x * v, ang.y * s, ang.z * s) * a;
        VmOffset { rot: self.r + bob_r, trans: self.t + bob_t }
    }
}

/// Rotation for (pitch, yaw, roll) degrees in the view frame (x forward, y left, z up).
pub fn angles_quat(a: Vec3) -> Quat {
    Quat::from_rotation_z(a.y.to_radians()) * Quat::from_rotation_y(a.x.to_radians()) * Quat::from_rotation_x(a.z.to_radians())
}

/// Cockpit sway (`cockpitSway*`): the cockpit model (BT's frame; the Pilot's HUD rides the
/// helmet's `human_pov_cockpit`) lags behind turns and movement. Factors are pilot_base.set's for
/// the Pilot; titan_buddy.set sets none, so BT uses the engine defaults (client.dll settings
/// table: gain 3, turn -4 / roll 0 / origin -1, move -0.3 / roll -0.2 / origin -0.15). The
/// target is clamped to cockpitSwayMin/Max (angles +-0.5, origin +-1) and scaled by the gain,
/// and the sway follows it on the cockpit_spring (constant 65, damping 9) - all engine
/// defaults. Reconstructed: a turn of 1 degree/s pushes `factor * TURN_K`, a speed of 1 unit/s
/// `factor * MOVE_K`, and the clamp comes before the gain.
#[derive(Clone, Copy, Debug)]
pub struct CockpitSwayDef {
    pub turn_angle: f32,
    pub turn_roll: f32,
    pub turn_origin: f32,
    pub move_angle: f32,
    pub move_roll: f32,
    pub move_origin: f32,
    pub gain: f32,
}

impl CockpitSwayDef {
    /// pilot_base.set (the Pilot's helmet HUD).
    pub const PILOT: Self = Self { turn_angle: -0.6, turn_roll: 0.2, turn_origin: -0.15, move_angle: -0.4, move_roll: -0.2, move_origin: -0.15, gain: 1.0 };
    /// Engine defaults (BT's cockpit: titan_buddy.set sets none).
    pub const TITAN: Self = Self { turn_angle: -4.0, turn_roll: 0.0, turn_origin: -1.0, move_angle: -0.3, move_roll: -0.2, move_origin: -0.15, gain: 3.0 };
}

const TURN_K: f32 = 0.01;
const MOVE_K: f32 = 0.003;
const SWAY_MAX_ANGLE: f32 = 0.5;
const SWAY_MAX_ORIGIN: f32 = 1.0;
const SPRING_K: f32 = 65.0;
const SPRING_D: f32 = 9.0;

#[derive(Clone, Copy, Default)]
pub struct CockpitSway {
    last: Option<(f32, f32)>,
    /// (pitch, yaw, roll) degrees and origin offset (forward, left, up) in units.
    pub angles: Vec3,
    pub origin: Vec3,
    angles_vel: Vec3,
    origin_vel: Vec3,
}

impl CockpitSway {
    /// `vel` in the view's yaw frame (forward, left, up).
    pub fn step(&mut self, def: &CockpitSwayDef, yaw: f32, pitch: f32, vel: Vec3, dt: f32) {
        let dt = dt.max(1e-4);
        let (dyaw, dpitch) = match self.last {
            Some((y, p)) => {
                let dy = (yaw - y + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                (dy.to_degrees() / dt, (pitch - p).to_degrees() / dt)
            }
            None => (0.0, 0.0),
        };
        self.last = Some((yaw, pitch));
        let (tk, mk) = (TURN_K, MOVE_K);
        // Yaw rate is positive turning left; pitch rate positive looking down.
        let target_a = Vec3::new(
            def.turn_angle * dpitch * tk + def.move_angle * vel.x * mk,
            def.turn_angle * dyaw * tk,
            def.turn_roll * dyaw * tk + def.move_roll * -vel.y * mk,
        )
        .clamp(Vec3::splat(-SWAY_MAX_ANGLE), Vec3::splat(SWAY_MAX_ANGLE))
            * def.gain;
        let target_o = (Vec3::new(0.0, def.turn_origin * dyaw * tk, 0.0) + vel * def.move_origin * mk)
            .clamp(Vec3::splat(-SWAY_MAX_ORIGIN), Vec3::splat(SWAY_MAX_ORIGIN))
            * def.gain;
        // Substep the spring so a long frame can't blow it up.
        let n = (dt / 0.005).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as usize {
            self.angles_vel += (SPRING_K * (target_a - self.angles) - SPRING_D * self.angles_vel) * h;
            self.angles += self.angles_vel * h;
            self.origin_vel += (SPRING_K * (target_o - self.origin) - SPRING_D * self.origin_vel) * h;
            self.origin += self.origin_vel * h;
        }
    }

    /// A damage jolt (cl_titan_cockpit.nut JoltCockpit -> engine CockpitJolt): severity is the
    /// damage over 2000, clamped to 0..1 (CalcJoltMagnitude). `dir` is from the damage toward
    /// the eye, in the view's yaw frame (forward, left, up). The engine side is compiled; this
    /// kicks the sway spring so the cockpit is shoved away from the hit and rolls by up to
    /// cockpitShake_sourceRollRange (3 degrees) away from the side it came from. The push
    /// distance (about 4 units at full severity) is by eye.
    pub fn jolt(&mut self, dir: Vec3, severity: f32) {
        const ROLL_RANGE: f32 = 3.0;
        const PUSH: f32 = 4.0;
        let s = severity.clamp(0.0, 1.0);
        // An initial velocity of v peaks near 0.6 v / w for this spring (w = sqrt(65)).
        let w = SPRING_K.sqrt() / 0.6;
        self.origin_vel += dir.normalize_or_zero() * PUSH * s * w;
        // Hit from the left (dir pointing right, -y) rolls the cockpit right (positive roll).
        self.angles_vel.z += -dir.y.clamp(-1.0, 1.0) * ROLL_RANGE * s * w;
    }
}
