//! Offline Pilot movement checks against a map's collision dump (run the game once with
//! `TF_COLLISION_DUMP=<file>` to write one).
//!
//! `pilotscan <dump> run <x> <y> <z> <yaw deg> <secs> [sprint|walk|crouch|jump]` prints a sprint
//! (or walk, crouch walk, bunny hop) from a spot, every 0.05 s.
//!
//! `pilotscan <dump> scan <x0> <y0> <x1> <y1> <samples> [seed]` drops the Pilot on random
//! walkable spots in that area, sprints a second in a random direction and lists where the
//! sprint stopped dead against an obstacle no higher than 30 units in front of the feet (the
//! real game steps up 18 units; a box-hull probe measures what was in the way).

use tf_sim::collision::{CollisionWorld, CLIP_PILOT};
use tf_sim::glam::Vec3;
use tf_sim::pilot::*;

const DT: f32 = 1.0 / 120.0;

fn rng(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

/// The highest point (above the feet) of whatever blocks the hull's front face just ahead:
/// rays at 2-unit steps up the face and across it, reaching 6 units past it.
fn obstacle_height(w: &CollisionWorld, p: &PilotParams, pos: Vec3, dir: Vec3) -> Option<f32> {
    let side = Vec3::new(-dir.y, dir.x, 0.0);
    let mut top: Option<f32> = None;
    for k in 0..36 {
        let h = 1.0 + k as f32 * 2.0;
        for s in [-0.9f32, -0.45, 0.0, 0.45, 0.9] {
            let o = pos + Vec3::Z * h + side * (s * p.radius);
            if w.raycast_moving(o, dir, p.radius + 6.0, CLIP_PILOT).is_some() {
                top = Some(h);
            }
        }
    }
    top
}

/// Whether the standing hull fits a little further along `dir`, at most a step up: a stop
/// there is a snag (the real game's StepMove would go on), not a wall or a low ceiling.
fn passable(w: &CollisionWorld, p: &PilotParams, pos: Vec3, dir: Vec3) -> bool {
    let (mins, maxs) = (Vec3::new(-p.radius, -p.radius, 0.0), Vec3::new(p.radius, p.radius, p.height));
    (0..=18).any(|k| {
        let at = pos + dir * 4.0 + Vec3::Z * (k as f32 + 0.1);
        let free = !w.trace_box(at, at, mins, maxs, CLIP_PILOT).start_solid;
        // ...reachable from where the Pilot stands (straight up, then across)...
        let up = pos + Vec3::Z * (k as f32 + 0.1);
        free && !w.trace_box(pos + Vec3::Z * 0.05, up, mins, maxs, CLIP_PILOT).hit() && !w.trace_box(up, at, mins, maxs, CLIP_PILOT).hit()
    })
}

fn ground_at(w: &CollisionWorld, x: f32, y: f32, z: f32) -> Option<Vec3> {
    let h = w.raycast_moving(Vec3::new(x, y, z), -Vec3::Z, 20000.0, CLIP_PILOT)?;
    (h.normal.z > 0.7).then_some(h.point)
}

#[derive(Clone, Copy, PartialEq)]
enum Gait {
    Sprint,
    Walk,
    Crouch,
    Jump,
}

fn input(yaw: f32, g: Gait, tick: usize) -> PilotInput {
    PilotInput {
        forward: 1.0,
        yaw,
        sprint: g != Gait::Walk && g != Gait::Crouch,
        crouch: g == Gait::Crouch,
        jump: g == Gait::Jump && tick % 60 == 0,
        ..Default::default()
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: pilotscan <dump> run x y z yaw secs [sprint|walk|crouch|jump] | scan x0 y0 x1 y1 samples [seed]");
        return;
    }
    let t = std::time::Instant::now();
    let w = CollisionWorld::load(std::path::Path::new(&a[1])).expect("collision dump");
    w.finish();
    eprintln!("loaded {} triangles in {:?}", w.tris.len(), t.elapsed());
    let p = PilotParams::default();
    let f = |i: usize| a[i].parse::<f32>().unwrap();
    match a[2].as_str() {
        "run" => {
            let yaw = f(6).to_radians();
            let gait = match a.get(8).map(String::as_str) {
                Some("walk") => Gait::Walk,
                Some("crouch") => Gait::Crouch,
                Some("jump") => Gait::Jump,
                _ => Gait::Sprint,
            };
            let mut s = PilotState::new(Vec3::new(f(3), f(4), f(5)), yaw);
            let ticks = (f(7) / DT) as usize;
            for i in 0..ticks {
                step(&mut s, &input(yaw, gait, i), &p, &w, DT);
                if i % 6 == 0 {
                    println!("t {:5.2} pos {:8.1} {:8.1} {:7.1} hspeed {:5.0} vz {:5.0} {:?}", i as f32 * DT, s.pos.x, s.pos.y, s.pos.z, s.horizontal_speed(), s.vel.z, s.mode);
                }
            }
        }
        "scan" => {
            let (x0, y0, x1, y1) = (f(3), f(4), f(5), f(6));
            let n: usize = a[7].parse().unwrap();
            let mut seed: u64 = a.get(8).and_then(|s| s.parse().ok()).unwrap_or(12345) | 1;
            let (mut runs, mut stops, mut small, mut mantles, mut hitches) = (0, 0, 0, 0, 0);
            let mut hist = [0usize; 4];
            let mut dist_sum = 0.0;
            for _ in 0..n {
                let (x, y) = (x0 + (x1 - x0) * rng(&mut seed), y0 + (y1 - y0) * rng(&mut seed));
                let yaw = rng(&mut seed) * std::f32::consts::TAU;
                // A random height to start from, so floors under roofs get sampled too.
                let Some(g) = ground_at(&w, x, y, 4000.0 * rng(&mut seed) - 500.0) else { continue };
                // Room to stand and nothing within 48 units ahead at chest height or knee height
                // (a clear lane for at least the start of the run).
                let dir = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
                if w.raycast_moving(g + Vec3::Z * 2.0, Vec3::Z, p.height + 4.0, CLIP_PILOT).is_some() {
                    continue;
                }
                if [40.0, 24.0].iter().any(|&h| w.raycast_moving(g + Vec3::Z * h, dir, 48.0, CLIP_PILOT).is_some()) {
                    continue;
                }
                let mut s = PilotState::new(g + Vec3::Z * 0.5, yaw);
                s.mode = PilotMove::Ground;
                s.vel = dir * p.sprint_speed;
                runs += 1;
                let start = s.pos;
                let start_txt = format!("from {:.1} {:.1} {:.2} {:.1}", start.x, start.y, start.z, yaw.to_degrees());
                let mut was_fast = 0;
                let mut hitched = false;
                let report = |what: &str, s: &PilotState, i: usize, h: Option<f32>| {
                    println!(
                        "{what} at {:.0} {:.0} {:.0} yaw {:.0} after {:.2}s: obstacle top {:?} (TF_PILOT_TP=\"{:.0} {:.0} {:.0} {:.0}\") {start_txt}",
                        s.pos.x,
                        s.pos.y,
                        s.pos.z,
                        yaw.to_degrees(),
                        i as f32 * DT,
                        h,
                        s.pos.x - dir.x * 150.0,
                        s.pos.y - dir.y * 150.0,
                        s.pos.z + 20.0,
                        yaw.to_degrees()
                    )
                };
                for i in 0..120 {
                    let before = s.pos;
                    let grounded = s.on_ground();
                    step(&mut s, &input(yaw, Gait::Sprint, 1), &p, &w, DT);
                    let moved = (s.pos - before).truncate().dot(dir.truncate()) / DT;
                    if grounded && matches!(s.mode, PilotMove::Mantle { .. }) {
                        mantles += 1;
                        report("mantle from the ground", &s, i, obstacle_height(&w, &p, s.pos, dir));
                        break;
                    }
                    if moved > 200.0 {
                        was_fast += 1;
                    }
                    if was_fast >= 3 && moved < 60.0 && s.on_ground() {
                        stops += 1;
                        let h = obstacle_height(&w, &p, s.pos, dir);
                        let bucket = match h {
                            None => 0,
                            Some(h) if h <= 18.0 => 1,
                            Some(h) if h <= 30.0 => 2,
                            _ => 3,
                        };
                        hist[bucket] += 1;
                        if passable(&w, &p, s.pos, dir) {
                            small += 1;
                            report("snag (passable)", &s, i, h);
                        }
                        break;
                    }
                    // A hitch: a fast run losing more than 40% of its pace for a tick and going on.
                    if was_fast >= 3 && moved < 0.6 * p.sprint_speed && moved >= 60.0 && s.on_ground() && !hitched && passable(&w, &p, s.pos, dir) {
                        hitched = true;
                        hitches += 1;
                        report("hitch", &s, i, obstacle_height(&w, &p, s.pos, dir));
                    }
                }
                dist_sum += (s.pos - start).truncate().length();
            }
            println!(
                "runs {runs}, stopped {stops}: nothing found in front {}, obstacle <= 18 {}, 18-30 {}, taller {} ; snags where the hull fits a step further {small} ({:.1}% of runs); mantles from the ground {mantles}; hitches {hitches}; mean distance in 1 s {:.0}",
                hist[0],
                hist[1],
                hist[2],
                hist[3],
                100.0 * small as f32 / runs.max(1) as f32,
                dist_sum / runs.max(1) as f32
            );
        }
        "steps" => {
            // Where sprints step up a curb or stair (3 to 18 units in one tick, on the ground):
            // `steps x0 y0 x1 y1 samples max_z` lists the first few, for test spots.
            let (x0, y0, x1, y1) = (f(3), f(4), f(5), f(6));
            let (n, max_z): (usize, f32) = (a[7].parse().unwrap(), f(8));
            let mut seed: u64 = 99;
            for _ in 0..n {
                let (x, y) = (x0 + (x1 - x0) * rng(&mut seed), y0 + (y1 - y0) * rng(&mut seed));
                let yaw = rng(&mut seed) * std::f32::consts::TAU;
                let Some(g) = ground_at(&w, x, y, max_z) else { continue };
                let mut s = PilotState::new(g + Vec3::Z * 0.5, yaw);
                s.mode = PilotMove::Ground;
                for i in 0..120 {
                    let (z, grounded) = (s.pos.z, s.on_ground());
                    step(&mut s, &input(yaw, Gait::Sprint, 1), &p, &w, DT);
                    let rise = s.pos.z - z;
                    if grounded && s.on_ground() && (3.0..=18.5).contains(&rise) && i > 20 {
                        let back = Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 200.0;
                        println!("step up {rise:.1} at {:.0} {:.0} {:.0} yaw {:.0} (TF_PILOT_TP=\"{:.0} {:.0} {:.0} {:.0}\")", s.pos.x, s.pos.y, z, yaw.to_degrees(), s.pos.x - back.x, s.pos.y - back.y, z + 10.0, yaw.to_degrees());
                        break;
                    }
                }
            }
        }
        "tris" => {
            // Triangles within `r` of a point: `tris x y z r`.
            let (c, r) = (Vec3::new(f(3), f(4), f(5)), f(6));
            for (name, set) in [("solid", Some(&w)), ("pilot clip", w.clips[CLIP_PILOT].as_deref())] {
                for t in set.into_iter().flat_map(|s| s.tris.iter()) {
                    if t.a.min(t.b).min(t.c).cmple(c + r).all() && t.a.max(t.b).max(t.c).cmpge(c - r).all() {
                        println!("{name}: {:.1} | {:.1} | {:.1}  n {:.2}", t.a, t.b, t.c, t.n);
                    }
                }
            }
        }
        "replay" => {
            // A scan run again: on the ground at x y z, sprinting along yaw from sprint speed.
            let (pos, yaw) = (Vec3::new(f(3), f(4), f(5)), f(6).to_radians());
            let dir = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
            let mut s = PilotState::new(pos, yaw);
            s.mode = PilotMove::Ground;
            s.vel = dir * p.sprint_speed;
            for i in 0..120 {
                step(&mut s, &input(yaw, Gait::Sprint, 1), &p, &w, DT);
                if i % 6 == 5 {
                    println!("t {:5.2} pos {:8.1} {:8.1} {:7.1} hspeed {:5.0} {:?}", (i + 1) as f32 * DT, s.pos.x, s.pos.y, s.pos.z, s.horizontal_speed(), s.mode);
                }
            }
            println!("travelled {:.0}", (s.pos - pos).truncate().length());
        }
        "probe" => {
            // What is in front of the feet at a spot: per height, the nearest hit ahead.
            let (pos, yaw) = (Vec3::new(f(3), f(4), f(5)), f(6).to_radians());
            let dir = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
            let g = ground_at(&w, pos.x, pos.y, pos.z + 40.0);
            println!("ground under the spot: {g:?}");
            let base = g.unwrap_or(pos);
            for k in 0..20 {
                let h = 1.0 + k as f32 * 2.0;
                let hit = w.raycast_moving(base + Vec3::Z * h, dir, 64.0, CLIP_PILOT);
                println!("h {h:4.0}: {}", hit.map_or("clear".to_string(), |x| format!("hit at {:.1} normal {:.2} {:.2} {:.2}", x.t, x.normal.x, x.normal.y, x.normal.z)));
            }
            // Ground heights along the way.
            for d in (0..12).map(|i| i as f32 * 6.0) {
                let q = base + dir * d;
                println!("ahead {d:3.0}: ground {:?}", ground_at(&w, q.x, q.y, base.z + 60.0).map(|g| g.z - base.z));
            }
        }
        "bounds" => {
            for (name, set) in [("world", Some(&w)), ("pilot clips", w.clips[CLIP_PILOT].as_deref())] {
                let Some(set) = set else { continue };
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for t in &set.tris {
                    lo = lo.min(t.a.min(t.b).min(t.c));
                    hi = hi.max(t.a.max(t.b).max(t.c));
                }
                println!("{name}: {lo} .. {hi}");
            }
        }
        _ => eprintln!("unknown mode"),
    }
}
