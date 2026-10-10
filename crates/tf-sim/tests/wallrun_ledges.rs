//! Wall-running past things sticking out of the wall: trims, window frames, pillars. On a
//! wall-run the wall is the Pilot's floor (client.dll turns the up vector, player+0x2494, to
//! the wall's normal and runs the surface move 0x18020cc30), so what blocks the run goes
//! through StepMove (0x1801fb0e0) measured from the wall: anything up to the step height (18)
//! out from it is stepped over, as a walk steps up a curb; deeper things still stop the run.
//! `cargo test -p tf-sim --test wallrun_ledges -- --nocapture` prints each run.

use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3;
use tf_sim::pilot::*;

const WALL_X: f32 = 400.0;
const DT: f32 = 1.0 / 120.0;
/// Where the obstacles start along the wall (the run starts at y = 0).
const AT_Y: f32 = 200.0;

fn quad(w: &mut CollisionWorld, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    w.add(a, b, c);
    w.add(a, c, d);
}

/// An axis-aligned solid box.
fn cuboid(w: &mut CollisionWorld, lo: Vec3, hi: Vec3) {
    let c = |x: bool, y: bool, z: bool| Vec3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
    let (f, t) = (false, true);
    quad(w, c(f, f, f), c(f, t, f), c(t, t, f), c(t, f, f));
    quad(w, c(f, f, t), c(t, f, t), c(t, t, t), c(f, t, t));
    quad(w, c(f, f, f), c(t, f, f), c(t, f, t), c(f, f, t));
    quad(w, c(f, t, f), c(f, t, t), c(t, t, t), c(t, t, f));
    quad(w, c(f, f, f), c(f, f, t), c(f, t, t), c(f, t, f));
    quad(w, c(t, f, f), c(t, t, f), c(t, t, t), c(t, f, t));
}

/// A floor and a tall wall at x = WALL_X facing -X.
fn arena() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    let s = 6000.0;
    quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    quad(&mut w, Vec3::new(WALL_X, -s, 0.0), Vec3::new(WALL_X, -s, 600.0), Vec3::new(WALL_X, s, 600.0), Vec3::new(WALL_X, s, 0.0));
    w
}

/// Something `depth` out from the wall, from y = AT_Y for `width`, between heights z0 and z1.
fn bump(w: &mut CollisionWorld, depth: f32, width: f32, z0: f32, z1: f32) {
    cuboid(w, Vec3::new(WALL_X - depth, AT_Y, z0), Vec3::new(WALL_X, AT_Y + width, z1));
}

struct Run {
    /// Furthest along the wall (y) and the slowest horizontal speed once the run is going.
    reach: f32,
    min_speed: f32,
    /// Still wall-running at the end, and the time the run ended (if it did).
    running: bool,
    ended: Option<f32>,
    /// The highest the Pilot got.
    top: f32,
}

/// Jump at the wall moving along +Y and wall-run forward for `secs`.
fn wallrun(w: &CollisionWorld, secs: f32) -> Run {
    let p = PilotParams::default();
    let yaw = std::f32::consts::FRAC_PI_2;
    let mut s = PilotState::new(Vec3::new(WALL_X - p.radius - 0.5, 0.0, 10.0), yaw);
    s.vel = Vec3::new(60.0, 300.0, 0.0);
    let input = PilotInput { forward: 1.0, yaw, ..Default::default() };
    let mut r = Run { reach: 0.0, min_speed: f32::MAX, running: false, ended: None, top: 0.0 };
    let mut started = false;
    for i in 0..(secs / DT) as usize {
        step(&mut s, &input, &p, w, DT);
        let on_wall = matches!(s.mode, PilotMove::WallRun { .. });
        if on_wall && !started {
            started = true;
        }
        if started && on_wall && i > 12 {
            r.min_speed = r.min_speed.min(s.horizontal_speed());
        }
        if started && !on_wall && r.ended.is_none() {
            r.ended = Some(i as f32 * DT);
        }
        r.reach = r.reach.max(s.pos.y);
        r.top = r.top.max(s.pos.z);
        r.running = on_wall;
    }
    assert!(started, "no wall-run started");
    r
}

#[test]
fn flat_wall_reference() {
    let r = wallrun(&arena(), 1.2);
    println!("flat wall: reach {:.0}, slowest {:.0}, running {}", r.reach, r.min_speed, r.running);
    assert!(r.running && r.reach > AT_Y + 150.0 && r.min_speed > 250.0);
}

/// Full-height trims and pilasters 2, 6, 12 and 17 units out (8 wide) are run past without
/// losing speed; up to the wall-run's sense distance (wallrun_allowed_wall_dist 13) the run
/// carries on along the wall behind them. 24 out is more than the step height: it stops.
#[test]
fn runs_past_trims_up_to_the_step_height() {
    let flat = wallrun(&arena(), 1.2);
    for depth in [2.0, 6.0, 12.0, 17.0, 24.0] {
        let mut w = arena();
        bump(&mut w, depth, 8.0, 0.0, 600.0);
        let r = wallrun(&w, 1.2);
        println!("trim {depth:>4} out: reach {:.0} (flat {:.0}), slowest {:.0}, running {}, ended {:?}", r.reach, flat.reach, r.min_speed, r.running, r.ended);
        if depth <= 18.0 {
            assert!(r.reach > AT_Y + 150.0, "stopped at a trim {depth} out: reached {}", r.reach);
            assert!(r.min_speed > 0.9 * flat.min_speed, "a trim {depth} out cost speed: {} vs {}", r.min_speed, flat.min_speed);
            assert!((r.reach - flat.reach).abs() < 10.0, "a trim {depth} out changed the run: {} vs {}", r.reach, flat.reach);
            if depth <= 12.0 {
                assert!(r.running, "fell off after a trim {depth} out");
            }
        } else {
            assert!(r.reach < AT_Y, "stepped past something {depth} out (more than the step height)");
        }
    }
}

/// A run of trims one after another (a fluted wall) and a wide pillar.
#[test]
fn runs_past_flutes_and_a_pillar() {
    let mut w = arena();
    for k in 0..5 {
        cuboid(&mut w, Vec3::new(WALL_X - 4.0, AT_Y + k as f32 * 24.0, 0.0), Vec3::new(WALL_X, AT_Y + k as f32 * 24.0 + 6.0, 600.0));
    }
    let r = wallrun(&w, 1.2);
    println!("flutes: reach {:.0}, slowest {:.0}, running {}", r.reach, r.min_speed, r.running);
    assert!(r.running && r.reach > AT_Y + 150.0 && r.min_speed > 250.0);

    let mut w = arena();
    bump(&mut w, 16.0, 48.0, 0.0, 600.0);
    let r = wallrun(&w, 1.2);
    println!("pillar 16 out, 48 wide: reach {:.0}, slowest {:.0}, running {}", r.reach, r.min_speed, r.running);
    assert!(r.reach > AT_Y + 150.0 && r.min_speed > 250.0);
}

/// A window frame (4 units out: jambs, sill and head around a 100 x 80 opening) at the height
/// the run goes through, and a frame whose sill catches only the top of the hull.
#[test]
fn runs_past_window_frames() {
    let frame = |w: &mut CollisionWorld, depth: f32, z0: f32| {
        let (y0, y1, z1) = (AT_Y, AT_Y + 106.0, z0 + 86.0);
        cuboid(w, Vec3::new(WALL_X - depth, y0, z0), Vec3::new(WALL_X, y0 + 6.0, z1));
        cuboid(w, Vec3::new(WALL_X - depth, y1 - 6.0, z0), Vec3::new(WALL_X, y1, z1));
        cuboid(w, Vec3::new(WALL_X - depth, y0, z0), Vec3::new(WALL_X, y1, z0 + 6.0));
        cuboid(w, Vec3::new(WALL_X - depth, y0, z1 - 6.0), Vec3::new(WALL_X, y1, z1));
    };
    for (depth, z0) in [(4.0, 20.0), (4.0, 100.0), (10.0, 60.0)] {
        let mut w = arena();
        frame(&mut w, depth, z0);
        let r = wallrun(&w, 1.2);
        println!("window frame {depth} out from z {z0}: reach {:.0}, slowest {:.0}, running {}", r.reach, r.min_speed, r.running);
        assert!(r.running && r.reach > AT_Y + 150.0 && r.min_speed > 250.0, "window frame {depth} out at z {z0}");
    }
}

/// A shallow cornice above: the run rising into its underside steps out around it (as on
/// mp_angel_city's clip boxes) instead of stopping its climb.
#[test]
fn rises_past_a_shallow_cornice() {
    let open = wallrun(&arena(), 0.8);
    let mut w = arena();
    cuboid(&mut w, Vec3::new(WALL_X - 6.0, -500.0, 110.0), Vec3::new(WALL_X, 2000.0, 118.0));
    let r = wallrun(&w, 0.8);
    println!("cornice: top {:.0} (open {:.0}), running {}", r.top, open.top, r.running);
    assert!(r.running && (r.top - open.top).abs() < 10.0);
    // One deeper than the step height still stops the climb.
    let mut w = arena();
    cuboid(&mut w, Vec3::new(WALL_X - 30.0, -500.0, 110.0), Vec3::new(WALL_X, 2000.0, 118.0));
    let r = wallrun(&w, 0.8);
    println!("deep cornice: top {:.0}", r.top);
    assert!(r.top < 110.0 - 72.0 + 1.0);
}
