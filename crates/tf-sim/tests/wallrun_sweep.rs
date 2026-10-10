//! Wallrun-start sweep: many jumps at one wall from different approach angles, speeds,
//! distances, view directions and inputs. Prints a success map (`cargo test -p tf-sim --test
//! wallrun_sweep -- --nocapture`) and checks it against client.dll's rule: a wallrun starts
//! when the airborne move touches the wall while moving into it (CanWallrun 0x180202090 has no
//! speed, input or view requirement), and never without touching it.

use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3;
use tf_sim::pilot::*;

const WALL_X: f32 = 400.0;
const DT: f32 = 1.0 / 120.0;

fn quad(w: &mut CollisionWorld, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    w.add(a, b, c);
    w.add(a, c, d);
}

/// A floor and a tall wall at x = WALL_X facing -X.
fn arena() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    let s = 6000.0;
    quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    quad(&mut w, Vec3::new(WALL_X, -s, 0.0), Vec3::new(WALL_X, -s, 600.0), Vec3::new(WALL_X, s, 600.0), Vec3::new(WALL_X, s, 0.0));
    w
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    /// A wallrun started.
    Ran,
    /// Touched the wall in the air without starting one.
    Missed,
    /// Never reached the wall in the air.
    NoContact,
}

/// View relative to the velocity.
#[derive(Debug, Clone, Copy)]
enum View {
    Along,
    /// Looking along the wall, the way the Pilot is moving.
    Parallel,
    /// Looking straight at the wall.
    AtWall,
}

#[derive(Debug, Clone, Copy)]
struct Case {
    /// Degrees between the velocity and the wall's inward normal (0 head-on, 90 parallel).
    angle: f32,
    speed: f32,
    /// Distance from the hull's side to the wall at the jump.
    dist: f32,
    view: View,
    forward: bool,
}

fn run(w: &CollisionWorld, c: Case) -> Outcome {
    let p = PilotParams::default();
    let a = c.angle.to_radians();
    // Moving toward +X (into the wall) and along +Y.
    let dir = Vec3::new(a.cos(), a.sin(), 0.0);
    let start = Vec3::new(WALL_X - p.radius - c.dist, 0.0, 0.0);
    let mut s = PilotState::new(start, 0.0);
    // Settle on the floor, then give the speed and jump.
    for _ in 0..12 {
        step(&mut s, &PilotInput::default(), &p, w, DT);
    }
    s.pos = start;
    s.vel = dir * c.speed;
    let yaw = match c.view {
        View::Along => a,
        View::Parallel => std::f32::consts::FRAC_PI_2,
        View::AtWall => 0.0,
    };
    let forward = if c.forward { 1.0 } else { 0.0 };
    let input = PilotInput { forward, yaw, ..Default::default() };
    step(&mut s, &PilotInput { jump: true, ..input }, &p, w, DT);
    let mut touched = false;
    for _ in 0..240 {
        let was_air = s.mode == PilotMove::Air;
        step(&mut s, &input, &p, w, DT);
        if matches!(s.mode, PilotMove::WallRun { .. }) {
            return Outcome::Ran;
        }
        // In contact: the move's push-out leaves the hull 1/128 unit off the wall (a jump that
        // lands right at the wall's foot can pass within half a unit without touching it).
        if was_air && s.mode == PilotMove::Air && s.pos.x + p.radius >= WALL_X - 0.1 {
            touched = true;
        }
        if s.on_ground() {
            break;
        }
    }
    if touched { Outcome::Missed } else { Outcome::NoContact }
}

#[test]
fn wallrun_start_sweep() {
    let w = arena();
    let angles = [0.0, 15.0, 30.0, 45.0, 60.0, 70.0, 80.0, 85.0, 88.0];
    let mut map = String::new();
    let (mut ran, mut missed, mut none) = (0, 0, 0);
    let mut failures = Vec::new();
    map += &format!("{:<34}", "speed dist view input \\ angle");
    for a in angles {
        map += &format!("{a:>4}");
    }
    map += "\n";
    for speed in [120.0, 162.5, 243.0, 350.0, 500.0] {
        for dist in [0.0, 20.0, 60.0, 120.0] {
            for view in [View::Along, View::Parallel, View::AtWall] {
                for forward in [true, false] {
                    map += &format!("{:<34}", format!("{speed:>5} {dist:>4} {view:<8?} {}", if forward { "fwd" } else { "none" }));
                    for angle in angles {
                        let c = Case { angle, speed, dist, view, forward };
                        let o = run(&w, c);
                        map += match o {
                            Outcome::Ran => "   W",
                            Outcome::Missed => "   x",
                            Outcome::NoContact => "   .",
                        };
                        match o {
                            Outcome::Ran => ran += 1,
                            Outcome::Missed => {
                                missed += 1;
                                failures.push(c);
                            }
                            Outcome::NoContact => none += 1,
                        }
                    }
                    map += "\n";
                }
            }
        }
    }
    println!("W = wallrun started, x = touched the wall in the air without one, . = never reached it\n{map}");
    println!("ran {ran}, missed {missed}, no contact {none}");
    assert!(failures.is_empty(), "{} jumps touched the wall without a wallrun, e.g. {:?}", failures.len(), &failures[..failures.len().min(5)]);
}

/// Walking off a ledge into a wall: no wallrun for 0.5 s after leaving the ground without a
/// jump (CanWallrun's lastGroundTime check, 0x1802020e4); a jump has no such lockout.
#[test]
fn no_wallrun_right_after_walking_off_a_ledge() {
    let mut w = CollisionWorld::default();
    let s = 6000.0;
    // A 300-unit-high floor ending in a ledge at x = 0, the ground below, and a tall wall 40
    // units past the ledge.
    quad(&mut w, Vec3::new(-s, -s, 300.0), Vec3::new(0.0, -s, 300.0), Vec3::new(0.0, s, 300.0), Vec3::new(-s, s, 300.0));
    quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    quad(&mut w, Vec3::new(40.0 + 16.0, -s, 0.0), Vec3::new(56.0, -s, 900.0), Vec3::new(56.0, s, 900.0), Vec3::new(56.0, s, 0.0));
    let p = PilotParams::default();
    let mut st = PilotState::new(Vec3::new(-40.0, 0.0, 300.0), 0.0);
    for _ in 0..12 {
        step(&mut st, &PilotInput::default(), &p, &w, DT);
    }
    st.vel = Vec3::new(240.0, 240.0, 0.0);
    // Steering into the wall keeps the hull hitting it every tick.
    let input = PilotInput { forward: 1.0, ..Default::default() };
    let mut t_air = 0.0;
    let mut first_run = None;
    for _ in 0..240 {
        step(&mut st, &input, &p, &w, DT);
        if !st.on_ground() {
            t_air += DT;
        }
        if matches!(st.mode, PilotMove::WallRun { .. }) {
            first_run = Some(t_air);
            break;
        }
    }
    let t = first_run.expect("wallruns once the lockout is over");
    assert!(t >= 0.5, "started {t:.3} s after walking off the ledge");
}

/// The two start probes (knee height and mid-hull, each a cube of the hull's half-width) must
/// both find the wall within wallrun_allowed_wall_dist: a waist-high wall can't be run, a wall
/// reaching the mid probe's cube can.
#[test]
fn short_walls() {
    let p = PilotParams::default();
    let runs = |h: f32| {
        let mut w = CollisionWorld::default();
        let s = 6000.0;
        quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
        quad(&mut w, Vec3::new(WALL_X, -s, 0.0), Vec3::new(WALL_X, -s, h), Vec3::new(WALL_X, s, h), Vec3::new(WALL_X, s, 0.0));
        // Falling from 6 units up: the mid probe's cube (52 +- 14.4 above the feet) reaches
        // below 45 at once.
        let mut st = PilotState::new(Vec3::new(WALL_X - p.radius - 0.5, 0.0, 6.0), std::f32::consts::FRAC_PI_2);
        st.vel = Vec3::new(60.0, 300.0, 0.0);
        let mut ran = false;
        for _ in 0..30 {
            step(&mut st, &PilotInput { forward: 1.0, yaw: std::f32::consts::FRAC_PI_2, ..Default::default() }, &p, &w, DT);
            ran |= matches!(st.mode, PilotMove::WallRun { .. });
        }
        ran
    };
    assert!(!runs(30.0), "a 30-unit wall isn't runnable");
    assert!(runs(45.0), "a 45-unit wall reaches the mid probe");
    assert!(runs(300.0));
}
