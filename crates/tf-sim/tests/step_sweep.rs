//! Steps, curbs and seams against the Pilot's box hull and client.dll's StepMove (stepheight
//! 18 from pilot_mp.set, StepMove 0x1801fb0e0): a sprinting Pilot runs over anything up to 18
//! units without slowing down and stops at anything higher; bevels, seams between triangles and
//! staircases don't snag. `cargo test -p tf-sim --test step_sweep -- --nocapture` prints each
//! run.

use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3;
use tf_sim::pilot::*;

const DT: f32 = 1.0 / 120.0;

fn quad(w: &mut CollisionWorld, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    w.add(a, b, c);
    w.add(a, c, d);
}

/// A floor at z = 0 made of 32-unit squares split into triangles (seams everywhere), from
/// x = -400 to `x_end`.
fn floor(w: &mut CollisionWorld, x_end: f32) {
    let mut x = -400.0;
    while x < x_end {
        let x1 = (x + 32.0).min(x_end);
        let mut y = -2000.0;
        while y < 2000.0 {
            quad(w, Vec3::new(x, y, 0.0), Vec3::new(x1, y, 0.0), Vec3::new(x1, y + 32.0, 0.0), Vec3::new(x, y + 32.0, 0.0));
            y += 32.0;
        }
        x = x1;
    }
}

/// A block from x = `x0` on, `h` high, its front face at x0 (optionally bevelled: the top
/// `bevel` units of the face lean back 45 degrees).
fn ledge(w: &mut CollisionWorld, x0: f32, h: f32, bevel: f32) {
    let (y0, y1, x1) = (-2000.0, 2000.0, x0 + 600.0);
    let b = bevel.min(h);
    quad(w, Vec3::new(x0, y0, 0.0), Vec3::new(x0, y1, 0.0), Vec3::new(x0, y1, h - b), Vec3::new(x0, y0, h - b));
    if b > 0.0 {
        quad(w, Vec3::new(x0, y0, h - b), Vec3::new(x0, y1, h - b), Vec3::new(x0 + b, y1, h), Vec3::new(x0 + b, y0, h));
    }
    quad(w, Vec3::new(x0 + b, y0, h), Vec3::new(x0 + b, y1, h), Vec3::new(x1, y1, h), Vec3::new(x1, y0, h));
}

struct Run {
    pos: Vec3,
    /// The slowest horizontal speed after getting up to sprint speed.
    min_speed: f32,
    airborne: bool,
    /// Path length.
    dist: f32,
}

/// Sprint along +X from x = -300 for `secs`, already at sprint speed.
fn sprint(w: &CollisionWorld, yaw_deg: f32, secs: f32) -> Run {
    let p = PilotParams::default();
    let yaw = yaw_deg.to_radians();
    let mut s = PilotState::new(Vec3::new(-300.0, 0.0, 0.1), yaw);
    let input = PilotInput { forward: 1.0, yaw, sprint: true, ..Default::default() };
    // Settle onto the floor, then go.
    for _ in 0..12 {
        step(&mut s, &PilotInput { yaw, ..Default::default() }, &p, w, DT);
    }
    s.vel = Vec3::new(yaw.cos(), yaw.sin(), 0.0) * p.sprint_speed;
    let mut run = Run { pos: s.pos, min_speed: f32::MAX, airborne: false, dist: 0.0 };
    for _ in 0..(secs / DT) as usize {
        let before = s.pos;
        step(&mut s, &input, &p, w, DT);
        run.dist += (s.pos - before).length();
        run.min_speed = run.min_speed.min(s.horizontal_speed());
        run.airborne |= !s.on_ground();
    }
    run.pos = s.pos;
    run
}

#[test]
fn steps_up_to_18_and_not_higher() {
    let p = PilotParams::default();
    for (h, climbs) in [(4.0, true), (10.0, true), (17.0, true), (18.0, true), (19.0, false), (24.0, false)] {
        let mut w = CollisionWorld::default();
        floor(&mut w, 0.0);
        ledge(&mut w, 0.0, h, 0.0);
        let r = sprint(&w, 0.0, 2.0);
        println!("ledge {h:4}: ends at {:?}, slowest {:.0}", r.pos, r.min_speed);
        if climbs {
            assert!(r.pos.x > 100.0 && (r.pos.z - h).abs() < 0.1, "a {h}-unit ledge is stepped onto: {:?}", r.pos);
            assert!(r.min_speed > p.sprint_speed - 1.0, "no slowdown stepping up {h}: {}", r.min_speed);
            assert!(!r.airborne, "stays on the ground stepping up {h}");
        } else {
            assert!(r.pos.z < 1.0 && (r.pos.x + p.radius).abs() < 0.1, "a {h}-unit ledge stops the Pilot at its face: {:?}", r.pos);
        }
    }
}

#[test]
fn ledges_at_an_angle_and_bevelled_curbs() {
    let p = PilotParams::default();
    // An 8-unit curb whose top 4 units are a 45-degree bevel, and one bevelled all the way.
    for (h, bevel) in [(8.0, 4.0), (8.0, 8.0), (14.0, 3.0)] {
        for yaw in [0.0, 30.0, 60.0] {
            let mut w = CollisionWorld::default();
            floor(&mut w, 0.0);
            ledge(&mut w, 0.0, h, bevel);
            let r = sprint(&w, yaw, 3.0);
            println!("curb {h} bevel {bevel} at {yaw} deg: ends at {:?}, slowest {:.0}", r.pos, r.min_speed);
            assert!(r.pos.x > 50.0 && (r.pos.z - h).abs() < 0.1, "curb {h}/{bevel} at {yaw} degrees is crossed: {:?}", r.pos);
            assert!(r.min_speed > p.sprint_speed - 1.0, "no slowdown on curb {h}/{bevel} at {yaw}: {}", r.min_speed);
        }
    }
}

#[test]
fn seams_in_floor_and_wall_dont_snag() {
    let p = PilotParams::default();
    // A wall along y = 100 made of many thin triangle strips, the floor's seams under it; run
    // into it at a shallow angle and slide along.
    let mut w = CollisionWorld::default();
    floor(&mut w, 2000.0);
    let mut x = -400.0;
    while x < 2000.0 {
        quad(&mut w, Vec3::new(x, 100.0, 0.0), Vec3::new(x + 24.0, 100.0, 0.0), Vec3::new(x + 24.0, 100.0, 200.0), Vec3::new(x, 100.0, 200.0));
        x += 24.0;
    }
    let r = sprint(&w, 20.0, 2.0);
    println!("wall slide: ends at {:?}, slowest {:.0}", r.pos, r.min_speed);
    assert!((r.pos.y - (100.0 - p.radius)).abs() < 0.1, "against the wall: {:?}", r.pos);
    // Sliding keeps the part of the speed along the wall (cos 20 of the sprint) and builds back up.
    assert!(r.min_speed > p.sprint_speed * 0.9, "slides on: {}", r.min_speed);
    assert!(r.pos.x > 100.0 && !r.airborne);
}

#[test]
fn runs_up_and_down_stairs() {
    let p = PilotParams::default();
    // Ten 8-unit steps, 12 deep, from x = 0, then a landing.
    let mut w = CollisionWorld::default();
    floor(&mut w, 0.0);
    for i in 0..10 {
        let (x0, h) = (i as f32 * 12.0, (i + 1) as f32 * 8.0);
        let top = if i == 9 { 2000.0 } else { 12.0 };
        quad(&mut w, Vec3::new(x0, -320.0, h - 8.0), Vec3::new(x0, 320.0, h - 8.0), Vec3::new(x0, 320.0, h), Vec3::new(x0, -320.0, h));
        quad(&mut w, Vec3::new(x0, -320.0, h), Vec3::new(x0, 320.0, h), Vec3::new(x0 + top, 320.0, h), Vec3::new(x0 + top, -320.0, h));
    }
    let r = sprint(&w, 0.0, 2.5);
    println!("stairs up: ends at {:?}, slowest {:.0}", r.pos, r.min_speed);
    assert!((r.pos.z - 80.0).abs() < 0.1 && r.pos.x > 100.0, "up the stairs: {:?}", r.pos);
    assert!(r.min_speed > p.sprint_speed - 1.0 && !r.airborne, "no slowdown or hops on the stairs: {}", r.min_speed);
    // Down: start on the landing, run back down; the Pilot stays on the steps (StayOnGround).
    let mut s = PilotState::new(Vec3::new(300.0, 0.0, 80.1), std::f32::consts::PI);
    let input = PilotInput { forward: 1.0, yaw: std::f32::consts::PI, sprint: true, ..Default::default() };
    for _ in 0..12 {
        step(&mut s, &PilotInput { yaw: input.yaw, ..Default::default() }, &p, &w, DT);
    }
    s.vel = Vec3::new(-p.sprint_speed, 0.0, 0.0);
    let mut air = 0;
    for _ in 0..(1.5 / DT) as usize {
        step(&mut s, &input, &p, &w, DT);
        air += !s.on_ground() as u32;
    }
    println!("stairs down: ends at {:?}, ticks in the air {air}", s.pos);
    assert!(s.pos.z < 1.0 && s.pos.x < -50.0 && air == 0, "down the stairs on the ground: {:?}, {air} ticks airborne", s.pos);
}

#[test]
fn walks_up_a_ramp_at_full_speed_along_it() {
    let p = PilotParams::default();
    // A 30-degree ramp from x = 0.
    let mut w = CollisionWorld::default();
    floor(&mut w, 0.0);
    let t = 30f32.to_radians().tan();
    quad(&mut w, Vec3::new(0.0, -320.0, 0.0), Vec3::new(0.0, 320.0, 0.0), Vec3::new(2000.0, 320.0, 2000.0 * t), Vec3::new(2000.0, -320.0, 2000.0 * t));
    let r = sprint(&w, 0.0, 2.0);
    println!("ramp: ends at {:?}, {:.0} units in 2 s", r.pos, r.dist);
    // The ground move runs the velocity along the slope at its full length (client.dll
    // 0x18020dd06): 2 s of sprint speed. The box rests on the ramp by its uphill edge.
    assert!(r.dist > p.sprint_speed * 2.0 - 5.0 && (r.pos.z - (r.pos.x + p.radius) * t).abs() < 0.1 && !r.airborne, "{} {:?}", r.dist, r.pos);
}

#[test]
fn hops_onto_a_ledge_in_the_air() {
    // AirMove's blocked trace also goes through StepMove (client.dll 0x1801f25db): rising (or
    // falling fast across) into a ledge with the feet less than 18 units under its top steps onto
    // it instead of sliding up its face. (Falling slowly onto it with no floor within the step
    // height below, 0x1801f23cc, only slides.)
    let p = PilotParams::default();
    let mut w = CollisionWorld::default();
    floor(&mut w, 0.0);
    ledge(&mut w, 0.0, 40.0, 0.0);
    let mut s = PilotState::new(Vec3::new(-40.0, 0.0, 26.0), 0.0);
    s.vel = Vec3::new(240.0, 0.0, 100.0);
    let input = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
    for _ in 0..60 {
        step(&mut s, &input, &p, &w, DT);
    }
    println!("air step: ends at {:?} {:?}", s.pos, s.mode);
    assert!((s.pos.z - 40.0).abs() < 0.1 && s.pos.x > 20.0, "onto the ledge: {:?}", s.pos);
}

#[test]
fn stays_crouched_under_a_low_ceiling() {
    let p = PilotParams::default();
    let mut w = CollisionWorld::default();
    floor(&mut w, 2000.0);
    // A 60-unit-high ceiling from x = 0.
    quad(&mut w, Vec3::new(0.0, -320.0, 60.0), Vec3::new(0.0, 320.0, 60.0), Vec3::new(2000.0, 320.0, 60.0), Vec3::new(2000.0, -320.0, 60.0));
    let mut s = PilotState::new(Vec3::new(-100.0, 0.0, 0.1), 0.0);
    let crouch = PilotInput { forward: 1.0, crouch: true, ..Default::default() };
    for _ in 0..240 {
        step(&mut s, &crouch, &p, &w, DT);
    }
    assert!(s.pos.x > 20.0 && s.crouched, "crawled under: {:?}", s.pos);
    for _ in 0..60 {
        step(&mut s, &PilotInput::default(), &p, &w, DT);
    }
    assert!(s.crouched && s.pos.z < 1.0, "still crouched under the ceiling: {:?}", s.pos);
}
