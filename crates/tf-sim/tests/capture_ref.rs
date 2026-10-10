//! Numbers from a capture of the real Titanfall 2 (Northstar, pilot_grapple_male on
//! mp_forwardbase_kodai, 2026-10-10): the TD.MoveRef mod logged the predicted local player every
//! client frame while scripted inputs ran 31 movement sequences. The real game runs one movement
//! step per client frame (the server's states are the client's frames, bit for bit), which was
//! about 141 fps there, so these tests step the Pilot at 1/141 s on a flat floor. Values are
//! the capture's; tolerances are about a frame. `cargo test -p tf-sim --test capture_ref --
//! --nocapture` prints ours.

use glam::{Vec2, Vec3};
use tf_sim::collision::CollisionWorld;
use tf_sim::pilot::*;

const DT: f32 = 1.0 / 141.0;

fn floor() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    let s = 20000.0;
    let (a, b, c, d) = (Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    w.add(a, b, c);
    w.add(a, c, d);
    w
}

struct Sim {
    s: PilotState,
    p: PilotParams,
    w: CollisionWorld,
    t: f32,
}

impl Sim {
    fn new() -> Self {
        let (p, w) = (PilotParams::default(), floor());
        let mut s = PilotState::new(Vec3::ZERO, 0.0);
        for _ in 0..30 {
            step(&mut s, &PilotInput::default(), &p, &w, DT);
        }
        assert!(s.on_ground());
        Sim { s, p, w, t: 0.0 }
    }
    fn tick(&mut self, i: PilotInput) {
        step(&mut self.s, &i, &self.p, &self.w, DT);
        self.t += DT;
    }
    fn run(&mut self, i: PilotInput, secs: f32) {
        for _ in 0..(secs / DT).round() as usize {
            self.tick(i);
        }
    }
    fn h(&self) -> f32 {
        self.s.horizontal_speed()
    }
}

fn sprint() -> PilotInput {
    PilotInput { forward: 1.0, sprint: true, ..Default::default() }
}

fn jump(i: PilotInput) -> PilotInput {
    PilotInput { jump: true, ..i }
}

/// Capture `sprint`: 260 u/s reached 0.105 s after the key, a stop over 0.173 s and 21.5 units.
#[test]
fn sprint_start_and_stop() {
    let mut m = Sim::new();
    let t0 = m.t;
    while m.h() < 259.0 {
        m.tick(sprint());
    }
    let ramp = m.t - t0;
    m.run(sprint(), 1.0);
    let (x0, t1) = (m.s.pos.x, m.t);
    while m.h() > 0.5 {
        m.tick(PilotInput::default());
    }
    let (stop_t, stop_d) = (m.t - t1, m.s.pos.x - x0);
    println!("sprint: 99% in {ramp:.3} s, stop {stop_t:.3} s over {stop_d:.2}");
    assert!((ramp - 0.105).abs() < 0.008, "{ramp}");
    assert!((stop_t - 0.173).abs() < 0.008 && (stop_d - 21.5).abs() < 0.5, "{stop_t} {stop_d}");
}

/// Capture `jumpsprint` / `jumpstand`: 293.7 u/s on the jump frame, 73.85 units up, 0.99 s in
/// the air; `dblstand`: the double jump 0.39 s in leaves at 295.8 and peaks 145.9 up after
/// 1.59 s in the air.
#[test]
fn jump_and_double_jump() {
    let mut m = Sim::new();
    m.tick(jump(PilotInput::default()));
    let v0 = m.s.vel.z;
    let t0 = m.t - DT;
    let mut top = 0.0f32;
    while !m.s.on_ground() {
        m.tick(PilotInput::default());
        top = top.max(m.s.pos.z);
    }
    let air = m.t - t0;
    println!("jump: {v0:.1} u/s, apex {top:.2}, {air:.3} s");
    assert!((v0 - 293.7).abs() < 0.3, "{v0}");
    assert!((top - 73.85).abs() < 0.3, "{top}");
    assert!((air - 0.99).abs() < 0.015, "{air}");

    let mut m = Sim::new();
    m.tick(jump(PilotInput::default()));
    let t0 = m.t - DT;
    m.run(PilotInput::default(), 0.39);
    m.tick(jump(PilotInput::default()));
    let v2 = m.s.vel.z;
    let mut top = 0.0f32;
    while !m.s.on_ground() {
        m.tick(PilotInput::default());
        top = top.max(m.s.pos.z);
    }
    let air = m.t - t0;
    println!("double jump: {v2:.1} u/s, apex {top:.2}, {air:.3} s");
    assert!((v2 - 295.8).abs() < 0.3, "{v2}");
    assert!((top - 145.9).abs() < 1.0, "{top}");
    assert!((air - 1.59).abs() < 0.03, "{air}");
}

/// Capture `bhop`: hopping again just after landing (a skip) leaves at 253.5 u/s (0.75 of the
/// height) and keeps the sprint speed.
#[test]
fn bunny_hop_skip() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    m.tick(jump(sprint()));
    while !m.s.on_ground() {
        m.tick(sprint());
    }
    m.tick(sprint());
    m.tick(jump(sprint()));
    let (vz, h) = (m.s.vel.z, m.h());
    println!("skip hop: {vz:.1} u/s up, {h:.1} across");
    assert!((vz - 253.5).abs() < 0.3, "{vz}");
    assert!((h - 260.0).abs() < 0.5, "{h}");
}

/// Capture `airback`: forward let go and back pressed 0.2 s into a sprint jump: the keyboard
/// grace takes 182 (0.7 x 260) off on that frame only (74.7 left), air acceleration then brakes
/// it through zero 0.15 s later, and it lands going backward at 60.7.
#[test]
fn air_back_grace_once() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    m.tick(jump(sprint()));
    m.run(sprint(), 0.2);
    let back = PilotInput { forward: -1.0, ..Default::default() };
    m.tick(back);
    let after = m.s.vel.x;
    let t0 = m.t;
    while m.s.vel.x > 0.0 {
        m.tick(back);
    }
    let zero = m.t - t0;
    while !m.s.on_ground() {
        m.tick(back);
    }
    let land = m.s.vel.x;
    println!("air back: {after:.1} after the press, zero {zero:.3} s later, lands at {land:.1}");
    assert!((after - 74.7).abs() < 1.0, "{after}");
    assert!((zero - 0.149).abs() < 0.01, "{zero}");
    assert!((land + 60.7).abs() < 1.5, "{land}");
}

/// Capture `airstrafe`: forward let go and right pressed 0.206 s into a sprint jump: it lands
/// 44.4 degrees off the takeoff heading at 224 u/s. Capture `airstand`: forward pressed 0.1 s
/// into a standing jump lands at 61.0, over airSpeed 60 (the extra air acceleration's cap is on
/// the 3D speed, so it takes a little of the vertical speed).
#[test]
fn air_strafe_and_air_speed() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    m.tick(jump(sprint()));
    m.run(sprint(), 0.2);
    let right = PilotInput { right: 1.0, ..Default::default() };
    while !m.s.on_ground() {
        m.tick(right);
    }
    let v = Vec2::new(m.s.vel.x, m.s.vel.y);
    let heading = v.y.atan2(v.x).to_degrees();
    println!("air strafe: lands {heading:.1} deg off at {:.1}", v.length());
    assert!((heading + 44.4).abs() < 1.0 && (v.length() - 224.0).abs() < 1.5, "{heading} {}", v.length());

    let mut m = Sim::new();
    m.tick(jump(PilotInput::default()));
    m.run(PilotInput::default(), 0.1);
    let fwd = PilotInput { forward: 1.0, ..Default::default() };
    let mut last = 0.0;
    while !m.s.on_ground() {
        last = m.h();
        m.tick(fwd);
    }
    println!("air stand: lands at {last:.2}");
    assert!((last - 61.0).abs() < 0.2, "{last}");
}

/// Capture `slide`: crouch at 260 slides at 398.7 after the first frame (+150 to the 400 cap,
/// then a frame of slide friction), 310.6 after 0.5 s, 236.1 after 1 s, and drops to the crouch
/// speed 80 at 2.03 s.
#[test]
fn slide_curve() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    let crouch = PilotInput { crouch: true, ..sprint() };
    m.tick(crouch);
    let first = m.h();
    let t0 = m.t - DT;
    let mut at = [0.0f32; 2];
    while m.h() > 80.5 {
        m.tick(crouch);
        for (k, s) in [0.5f32, 1.0].iter().enumerate() {
            if at[k] == 0.0 && m.t - t0 >= *s {
                at[k] = m.h();
            }
        }
    }
    let end = m.t - t0;
    println!("slide: {first:.1}, {:.1} at 0.5 s, {:.1} at 1 s, crouch speed at {end:.3} s", at[0], at[1]);
    assert!((first - 398.7).abs() < 0.3, "{first}");
    assert!((at[0] - 310.6).abs() < 1.5 && (at[1] - 236.1).abs() < 1.5, "{at:?}");
    assert!((end - 2.03).abs() < 0.03, "{end}");
}

/// Capture `slidejump`: letting go of crouch and pressing jump on the same frame 0.4 s into a
/// slide doesn't jump (the unduck blocks jumps); the slide ends and the speed falls back to the
/// sprint speed at 1500 u/s^2.
#[test]
fn jump_while_unducking_is_lost() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    m.run(PilotInput { crouch: true, ..sprint() }, 0.4);
    let h0 = m.h();
    m.tick(jump(sprint()));
    let mut t = DT;
    while m.h() > 260.5 {
        assert!(m.s.on_ground() && m.s.vel.z <= 0.0, "jumped");
        m.tick(sprint());
        t += DT;
    }
    m.run(sprint(), 0.2);
    println!("slide at {h0:.1}, crouch let go with jump: no jump, sprint speed after {t:.3} s");
    assert!(m.s.on_ground() && m.s.jumps == 0);
    assert!((t - (h0 - 260.0) / 1500.0).abs() < 2.0 * DT, "{t}");
}

/// Capture `slidehop`: crouch pressed 0.1 s into a sprint jump: once ducked (0.4 s later) the
/// feet lift 12.5 units, and the landing frame is already at 400 (the slide starts on landing).
#[test]
fn crouched_landing_slides_at_once() {
    let mut m = Sim::new();
    m.run(sprint(), 1.0);
    m.tick(jump(sprint()));
    m.run(sprint(), 0.1);
    let crouch = PilotInput { crouch: true, ..sprint() };
    let mut lift = (0.0f32, 0.0f32);
    let t0 = m.t;
    while !m.s.on_ground() {
        let (z, vz) = (m.s.pos.z, m.s.vel.z);
        m.tick(crouch);
        let jump = m.s.pos.z - z - vz * DT;
        if jump > lift.0 {
            lift = (jump, m.t - t0);
        }
    }
    println!("crouched jump: feet lift {:.1} at {:.3} s, landing frame {:.1}", lift.0, lift.1, m.h());
    assert!((lift.0 - 12.5).abs() < 0.5 && (lift.1 - 0.4).abs() < 0.02, "{lift:?}");
    assert!((m.h() - 400.0).abs() < 0.1 && m.s.mode == PilotMove::Slide, "{}", m.h());
}
