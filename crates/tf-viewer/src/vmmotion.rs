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
    s.get(&format!("sp_base.{k}"))
        .or_else(|| s.get(&format!(".{k}")))
        .and_then(|v| v.trim().parse().ok())
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
        Self {
            k: Vec3::splat(65.0),
            c: Vec3::splat(9.0),
        }
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
        let mut names: Vec<String> = s
            .values
            .keys()
            .filter_map(|k| k.split_once('.').map(|(n, _)| n.to_string()))
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        names.dedup();
        let mut out = HashMap::new();
        for n in names {
            let axes = |mode: &str| {
                let g =
                    |axis: &str, what: &str, d: f32| s.f32(&format!("{n}.{mode}_{axis}_{what}"), d);
                SpringAxes {
                    k: Vec3::new(
                        g("pitch", "springconstant", 65.0),
                        g("yaw", "springconstant", 65.0),
                        g("roll", "springconstant", 65.0),
                    ),
                    c: Vec3::new(
                        g("pitch", "damping", 9.0),
                        g("yaw", "damping", 9.0),
                        g("roll", "damping", 9.0),
                    ),
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
    /// `viewkick_*_random_innerexclude`: the smallest random kick (pitch, yaw).
    pub pitch_inner_exclude: f32,
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
    /// `viewmodel_shake_forward`, `_up`, `_right`: how far the gun moves per unit of punch
    /// roll (see `ViewPunch::vm_shake`).
    pub vm_shake: Vec3,
}

impl KickDef {
    pub fn from_settings(s: &PlayerSettings) -> Self {
        let f = |k: &str, d: f32| val(s, k).unwrap_or(d);
        Self {
            spring: s
                .get("sp_base.viewkick_spring")
                .or_else(|| s.get(".viewkick_spring"))
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase(),
            pitch: [
                f("viewkick_pitch_base", 0.0),
                f("viewkick_pitch_random", 0.0),
                f("viewkick_pitch_softscale", 0.0),
                f("viewkick_pitch_hardscale", 0.0),
            ],
            yaw: [
                f("viewkick_yaw_base", 0.0),
                f("viewkick_yaw_random", 0.0),
                f("viewkick_yaw_softscale", 0.0),
                f("viewkick_yaw_hardscale", 0.0),
            ],
            pitch_inner_exclude: f("viewkick_pitch_random_innerexclude", 0.0),
            yaw_inner_exclude: f("viewkick_yaw_random_innerexclude", 0.0),
            roll: [
                f("viewkick_roll_base", 0.0),
                f("viewkick_roll_randommin", 0.0),
                f("viewkick_roll_randommax", 0.0),
                f("viewkick_roll_softscale", 0.0),
                f("viewkick_roll_hardscale", 0.0),
            ],
            hip_fraction: (
                f("viewkick_hipfire_weaponfraction", 0.0),
                f("viewkick_hipfire_weaponfraction_vmscale", 0.0),
            ),
            ads_fraction: (
                f("viewkick_ads_weaponfraction", 0.0),
                f("viewkick_ads_weaponfraction_vmscale", 0.0),
            ),
            // A first-shot scale of 0 (the default) isn't used (client.dll 0x1805a7990).
            first_hip: val(s, "viewkick_scale_firstshot_hipfire").filter(|&v| v != 0.0),
            first_ads: val(s, "viewkick_scale_firstshot_ads").filter(|&v| v != 0.0),
            scale_hip: (
                f("viewkick_scale_min_hipfire", 1.0),
                f("viewkick_scale_max_hipfire", 1.0),
            ),
            scale_ads: (
                f("viewkick_scale_min_ads", 1.0),
                f("viewkick_scale_max_ads", 1.0),
            ),
            per_shot: f("viewkick_scale_valuepershot", 0.0),
            lerp: (
                f("viewkick_scale_valuelerpstart", 0.0),
                f("viewkick_scale_valuelerpend", 0.0),
            ),
            decay_delay: f("viewkick_scale_valuedecaydelay", 0.0),
            decay_rate: f("viewkick_scale_valuedecayrate", 0.0),
            perm_pitch: (
                f("viewkick_perm_pitch_base", 0.0),
                f("viewkick_perm_pitch_random", 0.0),
            ),
            perm_yaw: (
                f("viewkick_perm_yaw_base", 0.0),
                f("viewkick_perm_yaw_random", 0.0),
            ),
            duck_scale: f("viewkick_duck_scale", 1.0),
            vm_shake: Vec3::new(
                f("viewmodel_shake_forward", 0.0),
                f("viewmodel_shake_up", 0.0),
                f("viewmodel_shake_right", 0.0),
            ),
        }
    }
}

/// ConVar `viewmodelShake_sourceRollRange` (3): the punch roll (degrees) that gives one unit of
/// `viewmodel_shake_*`.
const VM_SHAKE_ROLL_RANGE: f32 = 3.0;

/// Recoil state, as client.dll keeps it: the shot kicks one "weapon punch" spring (player
/// m_vecPunchWeapon_Angle / _AngleVel), stepped with the weapon's `viewkick_spring`, its hip
/// and ADS settings blended by the current zoom (0x1805a9380 and the punch update at
/// 0x1801f5210). What each part sees is split by the weapon fraction, blended hip to ADS by
/// the current zoom (weapon +0x1b40..0x1b4c):
/// - the camera, and with it the shots: pitch and yaw x (1 - fraction), the whole roll
///   (0x1802c7260, the view's punch);
/// - the viewmodel: pitch and yaw x fraction x vmScale, the roll undone (0x1802c7360).
/// (A function giving pitch and yaw x fraction alone, 0x1802c71c0, has no callers in either
/// dll. Source's base view punch, its own 65/9 spring, isn't kicked by shots.)
#[derive(Resource, Default, Clone)]
pub struct ViewPunch {
    /// Camera punch angles (degrees): added to the view.
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    /// Viewmodel kick angles (pitch, yaw, roll).
    pub vm: Vec3,
    /// The weapon punch spring: angles (pitch, yaw, roll) and velocity.
    ang: Vec3,
    vel: Vec3,
    springs: (SpringAxes, SpringAxes),
    /// Weapon fraction and its vmScale, hip and ADS, of the gun that last fired.
    fractions: ((f32, f32), (f32, f32)),
    /// `viewkick_scale_value` and seconds since the last shot.
    value: f32,
    since_shot: f32,
    /// Permanent aim change not yet taken by the aim (pitch, yaw), degrees.
    pub perm: Vec2,
    /// The gun's shot shake (view frame: forward, left, up): the weapon's `viewmodel_shake_*`
    /// times the punch roll over `viewmodelShake_sourceRollRange`, clamped to +-3, so it
    /// buzzes with the roll spring (client.dll 0x1800d23a3, viewmodel placement).
    pub vm_shake: Vec3,
    /// `viewmodel_shake_*` of the gun that last fired (forward, up, right).
    shake: Vec3,
}

impl ViewPunch {
    /// One shot's kick, times `extra` (a charge level's scale). `r` are four uniform random
    /// numbers in 0..1.
    pub fn kick(
        &mut self,
        def: &KickDef,
        springs: &WeaponSprings,
        ads: f32,
        crouched: bool,
        extra: f32,
        r: [f32; 4],
    ) {
        self.springs = springs.0.get(&def.spring).copied().unwrap_or_default();
        self.fractions = (def.hip_fraction, def.ads_fraction);
        // Kick scale ramps from min to max as shots pile up (value per shot, lerp window); with
        // no window (start = end, the default) it is the max (client.dll 0x1805a7990).
        let w = def.lerp.1 - def.lerp.0;
        let t = if w == 0.0 {
            1.0
        } else {
            ((self.value - def.lerp.0) / w).clamp(0.0, 1.0)
        };
        let first = self.value <= 0.0;
        let hip = if first {
            def.first_hip.unwrap_or(def.scale_hip.0)
        } else {
            def.scale_hip.0 + (def.scale_hip.1 - def.scale_hip.0) * t
        };
        let ads_s = if first {
            def.first_ads.unwrap_or(def.scale_ads.0)
        } else {
            def.scale_ads.0 + (def.scale_ads.1 - def.scale_ads.0) * t
        };
        let mut scale = hip + (ads_s - hip) * ads;
        if crouched {
            scale *= def.duck_scale;
        }
        scale *= extra;
        self.value += def.per_shot;
        self.since_shot = 0.0;
        // client.dll 0x18059c350: pitch and yaw are base +- a random amount between half the
        // inner exclusion and half the random range, on a random side; roll is +- randomMin..Max
        // and isn't scaled by the kick scale (only by the weapon's own roll scale, here `extra`).
        let side = |u: f32| if u < 0.5 { -1.0 } else { 1.0 };
        let rand = |random: f32, inner: f32, u: f32| inner * 0.5 + (random * 0.5 - inner * 0.5) * u;
        let pitch = def.pitch[0] + rand(def.pitch[1], def.pitch_inner_exclude, r[0]) * side(r[2]);
        let yaw = def.yaw[0] + rand(def.yaw[1], def.yaw_inner_exclude, r[1]) * side(r[3]);
        // (Sums of two of the four draws, wrapped, are new uniform draws for the roll.)
        let (roll_u, roll_side) = ((r[0] + r[3]).fract(), (r[1] + r[2]).fract());
        let roll = (def.roll[0]
            + (def.roll[1] + (def.roll[2] - def.roll[1]) * roll_u) * side(roll_side))
            * extra;
        let kick = Vec3::new(pitch * scale, yaw * scale, roll);
        let soft = kick * Vec3::new(def.pitch[2], def.yaw[2], def.roll[3]);
        let hard = kick * Vec3::new(def.pitch[3], def.yaw[3], def.roll[4]);
        // Hard kick moves the angle at once; soft kick pushes the spring (velocity += angle * 20,
        // client.dll 0x1802cfc50).
        self.ang += hard;
        self.vel += soft * 20.0;
        self.split(ads);
        log::debug!(
            "kick x{scale:.2} ({:.2} {:.2} {:.2}) punch now {:.2?} vel {:.1?}",
            kick.x,
            kick.y,
            kick.z,
            self.ang,
            self.vel
        );
        self.shake = def.vm_shake;
        // The permanent kick works the same way, times the kick scale.
        self.perm += Vec2::new(
            def.perm_pitch.0 + rand(def.perm_pitch.1, 0.0, r[0]) * side(r[2]),
            def.perm_yaw.0 + rand(def.perm_yaw.1, 0.0, r[1]) * side(r[3]),
        ) * scale;
    }

    /// The camera, shot and viewmodel shares of the weapon punch at zoom `ads`.
    fn split(&mut self, ads: f32) {
        let ((hf, hv), (af, av)) = self.fractions;
        let frac = hf + (af - hf) * ads;
        let vm_scale = hv + (av - hv) * ads;
        self.pitch = self.ang.x * (1.0 - frac);
        self.yaw = self.ang.y * (1.0 - frac);
        self.roll = self.ang.z;
        self.vm = Vec3::new(
            self.ang.x * frac * vm_scale,
            self.ang.y * frac * vm_scale,
            -self.ang.z,
        );
        let s = (self.ang.z / VM_SHAKE_ROLL_RANGE).clamp(-3.0, 3.0);
        self.vm_shake = Vec3::new(self.shake.x, -self.shake.z, self.shake.y) * s;
    }

    /// Spring the punch back and decay the kick scale; `ads` is the gun's zoom now.
    pub fn step(&mut self, dt: f32, def: Option<&KickDef>, ads: f32) {
        self.since_shot += dt;
        let (delay, rate) = def
            .map(|d| (d.decay_delay, d.decay_rate))
            .unwrap_or((0.0, 0.0));
        if self.since_shot > delay {
            self.value = (self.value - rate * dt).max(0.0);
        }
        let (h, a) = self.springs;
        let k = h.k.lerp(a.k, ads);
        let c = h.c.lerp(a.c, ads);
        // Stiff roll springs (k up to 28000) need small steps.
        let n = ((dt * 1000.0).ceil() as usize).clamp(1, 200);
        let h = dt / n as f32;
        for _ in 0..n {
            self.vel += (-k * self.ang - c * self.vel) * h;
            self.ang += self.vel * h;
        }
        self.split(ads);
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
            let dir_t = |d: &str| {
                Vec3::new(
                    g(&format!("sway_{d}_translate_x")),
                    g(&format!("sway_{d}_translate_y")),
                    g(&format!("sway_{d}_translate_z")),
                )
            };
            let dir_r = |d: &str| {
                Vec3::new(
                    g(&format!("sway_{d}_rotate_pitch")),
                    g(&format!("sway_{d}_rotate_yaw")),
                    g(&format!("sway_{d}_rotate_roll")),
                )
            };
            let gain = |k: &str, d: f32| {
                val(s, &format!("{k}{suffix}")).unwrap_or(
                    fallback
                        .map(|f| {
                            if k.contains("translate") {
                                f.t_gain
                            } else {
                                f.r_gain
                            }
                        })
                        .unwrap_or(d),
                )
            };
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
                move_t: [
                    "move_forward",
                    "move_back",
                    "move_left",
                    "move_right",
                    "move_up",
                    "move_down",
                ]
                .map(dir_t),
                move_r: [
                    "move_forward",
                    "move_back",
                    "move_left",
                    "move_right",
                    "move_up",
                    "move_down",
                ]
                .map(dir_r),
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
        Self {
            hip,
            zoomed,
            bob: b,
            bob_zoomed: bz,
        }
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

/// Sway and bob state of one viewmodel. The hip and zoomed sway springs run side by side
/// (each toward its own targets, within its own limits) and the result is blended by the zoom
/// fraction: blending the pivots instead swung the gun by units when aiming in while turning
/// (a 2.5 degree hip yaw sway about a pivot sliding out to the zoomed one 230 units ahead).
#[derive(Clone, Default)]
pub struct VmMotion {
    last: Option<(f32, f32)>,
    t: Vec3,
    r: Vec3,
    tz: Vec3,
    rz: Vec3,
    bob_phase: f32,
    bob_amount: f32,
}

/// The viewmodel offset to apply about its sway pivots: rotation (pitch, yaw, roll degrees)
/// and translation (view frame), blended by the zoom, plus each spring's own rotation for
/// its pivot.
#[derive(Clone, Copy, Default, Debug)]
pub struct VmOffset {
    pub rot: Vec3,
    pub trans: Vec3,
    pub rot_hip: Vec3,
    pub rot_zoomed: Vec3,
}

impl VmOffset {
    /// The translation that turning the viewmodel (whose camera bone sits at `offset` in model
    /// space) about the hip pivot `hp` by the hip sway and about the zoomed pivot `zp` by the
    /// zoomed sway comes to, blended by `ads`.
    pub fn pivot_term(&self, hp: Vec3, zp: Vec3, offset: Vec3, ads: f32) -> Vec3 {
        let about = |p: Vec3, r: Vec3| p - offset - angles_quat(r) * p;
        about(hp, self.rot_hip).lerp(about(zp, self.rot_zoomed), ads)
    }
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
                dy = (dy + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                (dy.to_degrees() / dt, (inp.pitch - p).to_degrees() / dt)
            }
            None => (0.0, 0.0),
        };
        self.last = Some((inp.yaw, inp.pitch));
        let (h, z) = (&def.hip, &def.zoomed);
        // Pushes: positive yaw rate turns left; positive pitch rate looks down.
        let turn = [
            (dyaw / TURN_FULL).clamp(0.0, 1.0),
            (-dyaw / TURN_FULL).clamp(0.0, 1.0),
            (-dpitch / TURN_FULL).clamp(0.0, 1.0),
            (dpitch / TURN_FULL).clamp(0.0, 1.0),
        ];
        let m = inp.vel / MOVE_FULL;
        let mv = [
            m.x.clamp(0.0, 1.0),
            (-m.x).clamp(0.0, 1.0),
            m.y.clamp(0.0, 1.0),
            (-m.y).clamp(0.0, 1.0),
            m.z.clamp(0.0, 1.0),
            (-m.z).clamp(0.0, 1.0),
        ];
        let spring = |set: &SwaySet, t: &mut Vec3, r: &mut Vec3| {
            let mut target_t = Vec3::ZERO;
            let mut target_r = Vec3::ZERO;
            for i in 0..6 {
                target_t += set.move_t[i] * mv[i];
                target_r += set.move_r[i] * mv[i];
            }
            for i in 0..4 {
                target_t += set.turn_t[i] * turn[i];
                target_r += set.turn_r[i] * turn[i];
            }
            target_t = target_t.clamp(set.min_t, set.max_t);
            target_r = target_r.clamp(set.min_r, set.max_r);
            *t += (target_t - *t) * (1.0 - (-set.t_gain * dt).exp());
            *r += (target_r - *r) * (1.0 - (-set.r_gain * dt).exp());
        };
        spring(h, &mut self.t, &mut self.r);
        spring(z, &mut self.tz, &mut self.rz);
        let sway_t = self.t.lerp(self.tz, inp.ads);
        let sway_r = self.r.lerp(self.rz, inp.ads);

        // Bob: one vertical cycle per bob_cycle_time, the sideways sway and the angles at
        // half that rate, scaled by speed up to bob_max_speed.
        let b = &def.bob;
        let bz = &def.bob_zoomed;
        let cycle = (b.cycle + (bz.cycle - b.cycle) * inp.ads).max(0.05);
        let max_speed = (b.max_speed + (bz.max_speed - b.max_speed) * inp.ads).max(1.0);
        let speed = Vec2::new(inp.vel.x, inp.vel.y).length();
        let want = if inp.on_ground && inp.bob {
            (speed / max_speed).min(1.0)
        } else {
            0.0
        };
        self.bob_amount += (want - self.bob_amount) * (1.0 - (-10.0 * dt).exp());
        self.bob_phase = (self.bob_phase + dt / cycle * std::f32::consts::TAU)
            .rem_euclid(std::f32::consts::TAU * 2.0);
        let vert = b.vert + (bz.vert - b.vert) * inp.ads;
        let horz = b.horz + (bz.horz - b.horz) * inp.ads;
        let (v, s) = (self.bob_phase.sin(), (self.bob_phase * 0.5).sin());
        let a = self.bob_amount;
        let bob_t = Vec3::new(0.0, horz * s, vert * v) * a;
        // The bob's angles too are each set's own about its own pivot (the hip bob's 1.7
        // degree yaw about the zoomed pivot would walk the sight by units mid-zoom).
        let bob_r = |ang: Vec3| Vec3::new(ang.x * v, ang.y * s, ang.z * s) * a;
        let (bh, bzr) = (bob_r(b.angles), bob_r(bz.angles));
        VmOffset {
            rot: sway_r + bh.lerp(bzr, inp.ads),
            trans: sway_t + bob_t,
            rot_hip: self.r + bh,
            rot_zoomed: self.rz + bzr,
        }
    }
}

/// How far the FOV has narrowed at zoom fraction `ads`: the zoom happens over the script's
/// `ads_fov_zoomfrac_start..end` part of the transition (the Kraber's 0.5..0.9: the scope comes up
/// first, then the view zooms), eased with a smoothstep (the easing is a guess).
pub fn ads_fov_blend(ads: f32, (start, end): (f32, f32)) -> f32 {
    let t = if end > start {
        ((ads - start) / (end - start)).clamp(0.0, 1.0)
    } else if ads >= end {
        1.0
    } else {
        0.0
    };
    t * t * (3.0 - 2.0 * t)
}

/// Seconds a change of zoom direction takes to cross-fade between `ads_in_seq` and
/// `ads_out_seq` (a guess; the engine's sequence blend time isn't in the data).
pub const ADS_SEQ_CROSSFADE: f32 = 0.1;

/// The ADS transition layers: `ads_in_seq` runs from the hip pose to the ADS pose and
/// `ads_out_seq` back, each driven by the zoom fraction (so `zoom_time_in/out` and fast-ADS mods
/// set their pace). `out` is how much the zoom-out sequence is shown (1 while zooming out,
/// cross-faded on a change of direction). Returns (cycle, weight) for the in and out sequences.
pub fn ads_layers(ads: f32, out: f32) -> [(f32, f32); 2] {
    let ads = ads.clamp(0.0, 1.0);
    [(ads.min(0.999), 1.0 - out), ((1.0 - ads).min(0.999), out)]
}

/// Moves `out` (see `ads_layers`) toward the direction the zoom is moving this frame.
pub fn ads_out_step(out: f32, ads: f32, last_ads: f32, dt: f32) -> f32 {
    let target = if ads < last_ads {
        1.0
    } else if ads > last_ads {
        0.0
    } else {
        return out;
    };
    let step = dt / ADS_SEQ_CROSSFADE;
    if target > out {
        (out + step).min(1.0)
    } else {
        (out - step).max(0.0)
    }
}

/// Rotation for (pitch, yaw, roll) degrees in the view frame (x forward, y left, z up).
pub fn angles_quat(a: Vec3) -> Quat {
    Quat::from_rotation_z(a.y.to_radians())
        * Quat::from_rotation_y(a.x.to_radians())
        * Quat::from_rotation_x(a.z.to_radians())
}

/// Cockpit sway (`cockpitSway*`): the cockpit model (BT's frame; the Pilot's HUD rides the
/// helmet's `human_pov_cockpit`) lags behind turns and movement. Factors are pilot_base.set's for
/// the Pilot; titan_buddy.set sets none, so BT uses the engine defaults (client.dll's player
/// settings table: gain 3, turn -4 / roll 0 / origin -1, move -0.3 / roll -0.2 / origin -0.15,
/// limits +-1 unit and +-0.5 degrees).
///
/// The update follows client.dll (the routine at 0x1805939c0, fed by 0x1804e7e70):
/// - the view-space velocity times the frame time, and the view's angular velocity averaged over
///   the last 5 frames times the tick interval, each push a target through the factors (forward
///   motion: origin x; sideways: origin y, yaw and roll; vertical: origin z and pitch; pitching:
///   origin z and pitch; turning: origin y, yaw and roll);
/// - the target is clamped to cockpitSwayMin/Max;
/// - the sway eases toward it at `cockpitSwayGain` per second (state = lerp(target, state,
///   exp(-gain dt))): the gain is a rate, not a scale.
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
    pub const PILOT: Self = Self {
        turn_angle: -0.6,
        turn_roll: 0.2,
        turn_origin: -0.15,
        move_angle: -0.4,
        move_roll: -0.2,
        move_origin: -0.15,
        gain: 1.0,
    };
    /// Engine defaults (BT's cockpit: titan_buddy.set sets none).
    pub const TITAN: Self = Self {
        turn_angle: -4.0,
        turn_roll: 0.0,
        turn_origin: -1.0,
        move_angle: -0.3,
        move_roll: -0.2,
        move_origin: -0.15,
        gain: 3.0,
    };
}

/// cockpitSwayMin/MaxAngle* and Min/MaxOrigin* engine defaults.
const SWAY_MAX_ANGLE: f32 = 0.5;
const SWAY_MAX_ORIGIN: f32 = 1.0;
/// Frames the angular velocity is averaged over (a ring of 5, times 0.2).
const SWAY_RATE_FRAMES: usize = 5;
/// gpGlobals->interval_per_tick: base_tickinterval_sp (server.dll, 0.05; MP is 1/60).
const TICK_INTERVAL: f32 = 0.05;
/// cockpit_spring_* engine defaults, for the footstep and damage jolts.
const SPRING_K: f32 = 65.0;
const SPRING_D: f32 = 9.0;

#[derive(Clone, Copy, Default)]
pub struct CockpitSway {
    last: Option<(f32, f32)>,
    /// The last frames' (pitch, yaw) rates in degrees per second, and the next slot.
    rates: [(f32, f32); SWAY_RATE_FRAMES],
    slot: usize,
    /// The eased sway: (pitch, yaw, roll) degrees and (forward, left, up) units.
    sway_angles: Vec3,
    sway_origin: Vec3,
    /// Jolts (footsteps, damage) on the cockpit spring, added on top.
    jolt_angles: Vec3,
    jolt_origin: Vec3,
    angles_vel: Vec3,
    origin_vel: Vec3,
    /// (pitch, yaw, roll) degrees and origin offset (forward, left, up) in units: sway plus jolts.
    pub angles: Vec3,
    pub origin: Vec3,
}

impl CockpitSway {
    /// `yaw`/`pitch` in radians (pitch positive looking down); `vel` in the view's yaw frame
    /// (forward, left, up).
    pub fn step(&mut self, def: &CockpitSwayDef, yaw: f32, pitch: f32, vel: Vec3, dt: f32) {
        let dt = dt.max(1e-4);
        let (dyaw, dpitch) = match self.last {
            Some((y, p)) => {
                let dy = (yaw - y + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                (dy.to_degrees() / dt, (pitch - p).to_degrees() / dt)
            }
            None => (0.0, 0.0),
        };
        self.last = Some((yaw, pitch));
        self.rates[self.slot] = (dpitch, dyaw);
        self.slot = (self.slot + 1) % SWAY_RATE_FRAMES;
        let k = 1.0 / SWAY_RATE_FRAMES as f32;
        let (pitch_in, yaw_in) = self
            .rates
            .iter()
            .fold((0.0, 0.0), |(p, y), r| (p + r.0 * k, y + r.1 * k));
        let (pitch_in, yaw_in) = (pitch_in * TICK_INTERVAL, yaw_in * TICK_INTERVAL);
        // The velocity in the view frame (pitch included), times the frame time.
        let (sp, cp) = pitch.sin_cos();
        let v = Vec3::new(vel.x * cp - vel.z * sp, vel.y, vel.x * sp + vel.z * cp) * dt;
        let target_o = Vec3::new(
            def.move_origin * v.x,
            def.move_origin * v.y + def.turn_origin * yaw_in,
            def.move_origin * v.z - def.turn_origin * pitch_in,
        )
        .clamp(Vec3::splat(-SWAY_MAX_ORIGIN), Vec3::splat(SWAY_MAX_ORIGIN));
        let target_a = Vec3::new(
            -def.move_angle * v.z - def.turn_angle * pitch_in,
            def.move_angle * v.y + def.turn_angle * yaw_in,
            -def.move_roll * v.y - def.turn_roll * yaw_in,
        )
        .clamp(Vec3::splat(-SWAY_MAX_ANGLE), Vec3::splat(SWAY_MAX_ANGLE));
        let f = (-def.gain * dt).exp();
        self.sway_origin = target_o.lerp(self.sway_origin, f);
        self.sway_angles = target_a.lerp(self.sway_angles, f);
        // The jolts spring back to rest. Substep so a long frame can't blow it up.
        let n = (dt / 0.005).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as usize {
            self.angles_vel += (-SPRING_K * self.jolt_angles - SPRING_D * self.angles_vel) * h;
            self.jolt_angles += self.angles_vel * h;
            self.origin_vel += (-SPRING_K * self.jolt_origin - SPRING_D * self.origin_vel) * h;
            self.jolt_origin += self.origin_vel * h;
        }
        self.angles = self.sway_angles + self.jolt_angles;
        self.origin = self.sway_origin + self.jolt_origin;
    }

    /// A footstep's cockpit jolt (cockpit_stepJolt_origin / _angles, "Cockpit jolt per
    /// footstep"). client.dll adds them to the cockpit spring's velocities (units/s and
    /// degrees/s), times 2 while sprinting (1 walking; 0.5 in one other movement state).
    pub fn step_jolt(&mut self, origin: Vec3, angles: Vec3, scale: f32) {
        self.origin_vel += origin * scale;
        self.angles_vel += angles * scale;
    }

    /// A damage jolt (cl_titan_cockpit.nut JoltCockpit -> CockpitJolt in client.dll): severity
    /// is the damage over 2000, clamped to 0..1 (CalcJoltMagnitude); `dir` points from the
    /// damage toward the eye (camera - damageOrigin), in the view's yaw frame (forward, left,
    /// up); `pitch` in radians. The engine flattens the direction and, per unit of severity,
    /// moves the cockpit 1 unit against it (toward the hit) and gives it 100 units/s that way,
    /// and turns it by its forward part in pitch and its right part in yaw and against it in
    /// roll (1 degree, 100 degrees/s).
    pub fn jolt(&mut self, dir: Vec3, severity: f32, pitch: f32) {
        const VEL: f32 = 100.0;
        let s = severity.clamp(0.0, 1.0);
        let d = Vec3::new(dir.x, dir.y, 0.0).normalize_or_zero();
        let (f, r, u) = (d.x, -d.y, pitch.sin() * d.x);
        let o = Vec3::new(-f, r, -u) * s;
        let a = Vec3::new(f, r, -r) * s;
        self.jolt_origin += o;
        self.origin_vel += o * VEL;
        self.jolt_angles += a;
        self.angles_vel += a * VEL;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cockpit_sway_eases_to_its_clamped_target() {
        // BT turning left at 90 degrees/s: the target (-4 x 90 x 0.05 = -18 degrees of yaw,
        // -4.5 units left) clamps to -0.5 and -1, and the sway eases there at 3/s.
        let mut s = CockpitSway::default();
        let dt = 1.0 / 60.0;
        let mut yaw = 0.0f32;
        for i in 0..120 {
            yaw += 90f32.to_radians() * dt;
            s.step(&CockpitSwayDef::TITAN, yaw, 0.0, Vec3::ZERO, dt);
            if i == 19 {
                // A third of a second in: 1 - exp(-1) of the way, less the 5-frame average's lag.
                assert!(s.angles.y < -0.25 && s.angles.y > -0.33, "{}", s.angles.y);
            }
        }
        assert!(
            (s.angles.y + 0.5).abs() < 0.01 && (s.origin.y + 1.0).abs() < 0.02,
            "{:?} {:?}",
            s.angles,
            s.origin
        );
        // Stopped: it eases back to rest.
        for _ in 0..120 {
            s.step(&CockpitSwayDef::TITAN, yaw, 0.0, Vec3::ZERO, dt);
        }
        assert!(s.angles.y.abs() < 0.01, "{}", s.angles.y);
    }

    #[test]
    fn weapon_punch_splits_by_fraction() {
        // The Mozambique's kick: hip fraction 0.1 (vmScale 0), ADS fraction 1.0 (vmScale 0.15).
        let def = KickDef {
            pitch: [-2.25, 0.0, 1.4, 0.5],
            yaw: [-0.95, 0.0, 1.5, 0.5],
            roll: [0.0, 0.6, 0.6, 0.2, 2.75],
            hip_fraction: (0.1, 0.0),
            ads_fraction: (1.0, 0.15),
            scale_hip: (1.0, 1.0),
            scale_ads: (1.0, 1.0),
            per_shot: 1.0,
            lerp: (0.0, 1.0),
            duck_scale: 1.0,
            ..Default::default()
        };
        let springs = WeaponSprings::default();
        // Aimed down the sights: the camera keeps still in pitch and yaw but takes the roll,
        // the gun takes 15% of the kick and undoes the roll.
        let mut p = ViewPunch::default();
        p.kick(&def, &springs, 1.0, false, 1.0, [0.5; 4]);
        p.step(0.01, Some(&def), 1.0);
        assert!(
            p.pitch.abs() < 1e-6 && p.yaw.abs() < 1e-6,
            "{} {}",
            p.pitch,
            p.yaw
        );
        assert!(
            p.vm.x < -0.05 && p.roll.abs() > 0.5 && (p.vm.z + p.roll).abs() < 1e-6,
            "{} {:?}",
            p.roll,
            p.vm
        );
        // From the hip: the camera takes 90% of the kick, the gun no pitch or yaw.
        let mut h = ViewPunch::default();
        h.kick(&def, &springs, 0.0, false, 1.0, [0.5; 4]);
        h.step(0.01, Some(&def), 0.0);
        assert!(h.pitch < -0.5 && h.vm.x == 0.0, "{} {:?}", h.pitch, h.vm);
    }

    #[test]
    fn fov_zooms_only_inside_its_window() {
        // Kraber: ads_fov_zoomfrac_start 0.5, end 0.9.
        assert_eq!(ads_fov_blend(0.3, (0.5, 0.9)), 0.0);
        assert_eq!(ads_fov_blend(0.5, (0.5, 0.9)), 0.0);
        assert!((ads_fov_blend(0.7, (0.5, 0.9)) - 0.5).abs() < 1e-6);
        assert_eq!(ads_fov_blend(0.95, (0.5, 0.9)), 1.0);
        // No window in the script: the whole transition.
        assert!((ads_fov_blend(0.5, (0.0, 1.0)) - 0.5).abs() < 1e-6);
        assert_eq!(ads_fov_blend(1.0, (0.0, 1.0)), 1.0);
    }

    #[test]
    fn ads_layers_meet_at_the_ends() {
        // Steady hip: the in sequence's start and the out sequence's end are both the hip pose.
        for out in [0.0, 0.4, 1.0] {
            let [(ci, wi), (co, wo)] = ads_layers(0.0, out);
            assert_eq!(ci, 0.0);
            assert_eq!(co, 0.999);
            assert!((wi + wo - 1.0).abs() < 1e-6);
        }
        // Full ADS: the in sequence's end and the out sequence's start.
        let [(ci, _), (co, _)] = ads_layers(1.0, 0.0);
        assert_eq!((ci, co), (0.999, 0.0));
    }

    #[test]
    fn zoom_direction_cross_fades() {
        // Zooming out: the out sequence fades in over the cross-fade time.
        let mut out = 0.0;
        for _ in 0..5 {
            out = ads_out_step(out, 0.5, 0.6, 0.01);
        }
        assert!((out - 0.5).abs() < 1e-5);
        // Holding still keeps whatever blend there is.
        assert_eq!(ads_out_step(out, 0.5, 0.5, 0.01), out);
        // Zooming in again fades back.
        for _ in 0..20 {
            out = ads_out_step(out, 0.6, 0.5, 0.01);
        }
        assert_eq!(out, 0.0);
    }
}
