//! Pilot ground and air movement against client.dll's movement code (the numbers the real game
//! gives with pilot_mp.set): top speeds, how fast they're reached and lost, jump takeoff and
//! height, and the keyboard grace after a jump. `cargo test -p tf-sim --test pilot_feel --
//! --nocapture` prints the measured curve values.

use glam::Vec3;
use tf_sim::collision::CollisionWorld;
use tf_sim::pilot::*;

const DT: f32 = 1.0 / 120.0;

fn floor() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    let s = 20000.0;
    let (a, b, c, d) = (Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    w.add(a, b, c);
    w.add(a, c, d);
    w
}

fn settle(s: &mut PilotState, p: &PilotParams, w: &CollisionWorld) {
    for _ in 0..30 {
        step(s, &PilotInput::default(), p, w, DT);
    }
    assert!(s.on_ground());
}

/// Seconds from a standstill until the horizontal speed reaches `frac` of `target`.
fn time_to(p: &PilotParams, w: &CollisionWorld, i: PilotInput, target: f32, frac: f32) -> (f32, f32) {
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, p, w);
    let mut t = 0.0;
    let mut reached = None;
    while t < 3.0 {
        step(&mut s, &i, p, w, DT);
        t += DT;
        if reached.is_none() && s.horizontal_speed() >= target * frac {
            reached = Some(t);
        }
    }
    (reached.unwrap_or(f32::INFINITY), s.horizontal_speed())
}

/// Seconds and units to stop after letting go of the keys at full speed.
fn stop(p: &PilotParams, w: &CollisionWorld, i: PilotInput) -> (f32, f32) {
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, p, w);
    for _ in 0..240 {
        step(&mut s, &i, p, w, DT);
    }
    let x0 = s.pos.x;
    let mut t = 0.0;
    while s.horizontal_speed() > 0.5 && t < 3.0 {
        step(&mut s, &PilotInput::default(), p, w, DT);
        t += DT;
    }
    (t, s.pos.x - x0)
}

/// Takeoff speed, apex above the floor and air time of a standing jump.
fn jump(p: &PilotParams, w: &CollisionWorld) -> (f32, f32, f32) {
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, p, w);
    step(&mut s, &PilotInput { jump: true, ..Default::default() }, p, w, DT);
    let v0 = s.vel.z;
    let (mut top, mut t) = (0.0f32, DT);
    while !s.on_ground() && t < 3.0 {
        step(&mut s, &PilotInput::default(), p, w, DT);
        top = top.max(s.pos.z);
        t += DT;
    }
    (v0, top, t)
}

fn fwd() -> PilotInput {
    PilotInput { forward: 1.0, ..Default::default() }
}

fn sprint() -> PilotInput {
    PilotInput { forward: 1.0, sprint: true, ..Default::default() }
}

#[test]
fn ground_speeds_and_ramps() {
    let (p, w) = (PilotParams::default(), floor());
    let (t_walk, walk) = time_to(&p, &w, fwd(), p.walk_speed, 0.99);
    let (t_sprint, run) = time_to(&p, &w, sprint(), p.sprint_speed, 0.99);
    let (t_stop_walk, d_stop_walk) = stop(&p, &w, fwd());
    let (t_stop_sprint, d_stop_sprint) = stop(&p, &w, sprint());
    println!("walk {walk:.1} in {t_walk:.3}s, sprint {run:.1} in {t_sprint:.3}s");
    println!("stop from walk {t_stop_walk:.3}s {d_stop_walk:.1}u, from sprint {t_stop_sprint:.3}s {d_stop_sprint:.1}u");
    // pilot_mp.set stand.speed / sprintspeed.
    assert!((walk - 173.5).abs() < 0.5, "walk {walk}");
    assert!((run - 260.0).abs() < 0.5, "sprint {run}");
    // Accelerate (0x1801f0ac0) at the stance acceleration 2500: 173.5 / 2500 = 0.069 s.
    assert!((t_walk - 0.069).abs() < 0.02, "walk ramp {t_walk}");
    assert!((t_sprint - 0.104).abs() < 0.02, "sprint ramp {t_sprint}");
    // Friction (0x1801f8090) with no input: deceleration -1 -> acceleration * 0.6 = 1500 u/s^2
    // (GetDeceleration 0x1801fcf10), linear: v^2 / 3000.
    assert!((t_stop_sprint - 260.0 / 1500.0).abs() < 0.02, "sprint stop time {t_stop_sprint}");
    assert!((d_stop_sprint - 260.0 * 260.0 / 3000.0).abs() < 3.0, "sprint stop distance {d_stop_sprint}");
    assert!((d_stop_walk - 173.5 * 173.5 / 3000.0).abs() < 3.0, "walk stop distance {d_stop_walk}");
}

/// Turning while running keeps the speed (Friction keeps the velocity along the input).
#[test]
fn sprint_turn_keeps_speed() {
    let (p, w) = (PilotParams::default(), floor());
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, &p, &w);
    let mut i = sprint();
    for _ in 0..120 {
        step(&mut s, &i, &p, &w, DT);
    }
    // Turn 90 degrees over half a second.
    let mut min_speed = f32::MAX;
    for k in 0..60 {
        i.yaw = std::f32::consts::FRAC_PI_2 * (k + 1) as f32 / 60.0;
        step(&mut s, &i, &p, &w, DT);
        min_speed = min_speed.min(s.horizontal_speed());
    }
    println!("speed through a 90 degree sprint turn: min {min_speed:.1}");
    assert!(min_speed > p.sprint_speed - 3.0, "sprint turn speed {min_speed}");
}

#[test]
fn jump_takeoff_and_height() {
    let (p, w) = (PilotParams::default(), floor());
    let (v0, top, air) = jump(&p, &w);
    println!("jump takeoff {v0:.1} u/s, apex {top:.1}, air time {air:.3}s");
    // The jump handler (0x180200cf9) takes sqrt(2 * sv_gravity * jumpheight) without the
    // gravityscale, which only scales the gravity applied in flight (0x1801f74b3): 300 u/s,
    // 60 / 0.8 = 75 units high, 2 * 300 / 600 = 1 s in the air.
    assert!((v0 - 300.0).abs() < 1.0, "takeoff {v0}");
    assert!((top - 75.0).abs() < 2.0, "apex {top}");
    assert!((air - 1.0).abs() < 0.03, "air time {air}");
}

/// The double jump at the apex of a jump: superjumpMaxHeight from zero vertical speed, so 75
/// more units; its horizontal part (superjumpHorzSpeed 180 along the input) takes 180 off the
/// old velocity.
#[test]
fn double_jump() {
    let (p, w) = (PilotParams::default(), floor());
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, &p, &w);
    for _ in 0..240 {
        step(&mut s, &sprint(), &p, &w, DT);
    }
    step(&mut s, &PilotInput { jump: true, ..sprint() }, &p, &w, DT);
    while s.vel.z > 0.0 {
        step(&mut s, &sprint(), &p, &w, DT);
    }
    let z0 = s.pos.z;
    let h0 = s.horizontal_speed();
    // Double jump pushing right only.
    let right = PilotInput { right: 1.0, ..Default::default() };
    step(&mut s, &PilotInput { jump: true, ..right }, &p, &w, DT);
    let v = s.vel;
    println!("double jump: vz {:.1}, horizontal {:?} from {h0:.1}", v.z, (v.x, v.y));
    assert!((v.z - 300.0).abs() < 2.0, "double jump vz {}", v.z);
    assert!((v.x - (h0 - 180.0)).abs() < 3.0 && (v.y + 180.0).abs() < 3.0, "double jump horizontal {v:?}");
    let mut top = s.pos.z;
    while s.vel.z > 0.0 {
        step(&mut s, &PilotInput::default(), &p, &w, DT);
        top = top.max(s.pos.z);
    }
    assert!((top - z0 - 75.0).abs() < 2.0, "double jump height {}", top - z0);
}

/// jump_keyboardgrace (AirMove 0x1801f1d62): pressing a key just after a jump turns the
/// velocity toward it at the same speed.
#[test]
fn keyboard_grace_after_jump() {
    let (p, w) = (PilotParams::default(), floor());
    let mut s = PilotState::new(Vec3::ZERO, 0.0);
    settle(&mut s, &p, &w);
    for _ in 0..240 {
        step(&mut s, &sprint(), &p, &w, DT);
    }
    step(&mut s, &PilotInput { jump: true, ..sprint() }, &p, &w, DT);
    let right = PilotInput { right: 1.0, ..Default::default() };
    for _ in 0..12 {
        step(&mut s, &right, &p, &w, DT);
    }
    let angle = (-s.vel.y).atan2(s.vel.x).to_degrees();
    println!("0.1 s of right after a sprint jump: heading {angle:.1} deg, speed {:.1}", s.horizontal_speed());
    // The first tick turns it furthest; its change is capped at 0.7 x 260 = 182 u/s along the
    // chord, which costs some speed (to about 220).
    assert!(angle > 85.0 && (s.horizontal_speed() - 220.0).abs() < 5.0, "heading {angle}, speed {}", s.horizontal_speed());
    // Well after a jump (0.5 s) the keys only steer at airAcceleration.
    let mut s = PilotState::new(Vec3::new(0.0, 0.0, 400.0), 0.0);
    s.vel.x = 260.0;
    for _ in 0..12 {
        step(&mut s, &right, &p, &w, DT);
    }
    let angle = (-s.vel.y).atan2(s.vel.x).to_degrees();
    assert!(angle < 15.0, "falling without a jump: heading {angle}");
}

/// Jumping keeps the horizontal speed, strafe keys held or not: the ground jump only sets the
/// vertical speed (jump handler 0x180200cf9) and the keyboard grace (0x1801f1d62) turns the
/// velocity toward the held keys at the same speed. (A takeoff touching a wall loses the part
/// of the velocity into the wall to the collision, which is what a recording that seemed to
/// show a strafe-jump speed loss had: the Pilot was against a wall both times.)
#[test]
fn strafe_jump_keeps_speed() {
    let (p, w) = (PilotParams::default(), floor());
    for side in [1.0, -1.0] {
        let mut s = PilotState::new(Vec3::ZERO, 0.0);
        settle(&mut s, &p, &w);
        let i = PilotInput { right: side, ..sprint() };
        for _ in 0..240 {
            step(&mut s, &sprint(), &p, &w, DT);
        }
        step(&mut s, &PilotInput { jump: true, ..i }, &p, &w, DT);
        let mut min_speed = s.horizontal_speed();
        for _ in 0..60 {
            step(&mut s, &i, &p, &w, DT);
            min_speed = min_speed.min(s.horizontal_speed());
        }
        println!("sprint, then jump holding forward + side {side}: min speed over 0.5 s {min_speed:.1}");
        assert!(min_speed > p.sprint_speed - 1.0, "strafe jump {side}: {min_speed}");
    }
}

/// Landing faster than the sprint speed: Friction (0x1801f8090) keeps the sprint speed along
/// the input and brakes the rest at the stance deceleration from the first ground tick. Above
/// the stance speed GetDeceleration (0x1801fcf10) would use sprintDeceleration, else
/// sprintAcceleration, but pilot_base.set / pilot_mp.set set neither (-1), so it is still
/// 0.6 x acceleration 2500 = 1500 u/s^2. There is no landing grace: the only no-deceleration
/// window (player+0x2450) follows velocity impulses (impulse_low_decel_duration_scalar,
/// 0x1802cc410). Only a jump on the landing tick skips the friction.
#[test]
fn landing_over_sprint_speed() {
    let (p, w) = (PilotParams::default(), floor());
    let mut s = PilotState::new(Vec3::new(0.0, 0.0, 20.0), 0.0);
    s.mode = PilotMove::Air;
    s.vel = Vec3::new(395.0, 0.0, -100.0);
    while !s.on_ground() {
        step(&mut s, &sprint(), &p, &w, DT);
    }
    let landed = s.horizontal_speed();
    let mut t = 0.0;
    while s.horizontal_speed() > p.sprint_speed + 0.5 {
        step(&mut s, &sprint(), &p, &w, DT);
        t += DT;
    }
    println!("landing at {landed:.1} standing: sprint speed again after {t:.3} s");
    assert!((t - (landed - p.sprint_speed) / p.ground_decel).abs() < 2.0 * DT, "{t}");
}
