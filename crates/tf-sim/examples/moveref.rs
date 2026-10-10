//! Replays a real Titanfall 2 movement capture (the TD.MoveRef mod's per-frame log, turned into
//! per-frame inputs by the capture parser) through the Pilot simulation over a collision dump of
//! the same map, and prints the simulated frames next to the real ones.
//!
//! `moveref <dump> <replay.tsv> [frame|sub120|<hz>]`
//!
//! The replay file has one row per real client frame: `t dt forward right sprint crouch jump yaw
//! pitch x y z vx vy vz og phase` (yaw/pitch in radians, the real view of that frame). The sim
//! starts on the ground at the first row's position and steps once per row with the row's `dt`
//! (`frame`, the default: the real game runs one movement step per client frame, and so does
//! the viewer), or in substeps of at most 1/120 s (`sub120`, how the viewer stepped before), or
//! at a fixed tick rate (`<hz>`, inputs sampled at each tick). Output: the same columns as the parser's client series (`t x y z vx
//! vy vz pitch yaw og phase`, degrees), so the same summary runs on both.

use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3;
use tf_sim::pilot::*;

struct Row {
    t: f32,
    dt: f32,
    input: PilotInput,
    pos: Vec3,
    phase: String,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: moveref <dump> <replay.tsv> [frame|sub120|<hz>]");
        return;
    }
    let w = CollisionWorld::load(std::path::Path::new(&a[1])).expect("collision dump");
    w.finish();
    let text = std::fs::read_to_string(&a[2]).expect("replay file");
    let rows: Vec<Row> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let f = |i: usize| c[i].parse::<f32>().ok();
            Some(Row {
                t: f(0)?,
                dt: f(1)?,
                input: PilotInput {
                    forward: f(2)?,
                    right: f(3)?,
                    sprint: f(4)? > 0.5,
                    crouch: f(5)? > 0.5,
                    jump: f(6)? > 0.5,
                    yaw: f(7)?,
                    pitch: f(8)?,
                    ads: false,
                },
                pos: Vec3::new(f(9)?, f(10)?, f(11)?),
                phase: c.get(16)?.to_string(),
            })
        })
        .collect();
    let mode = a.get(3).map(String::as_str).unwrap_or("frame");
    let p = PilotParams::default();
    let mut s = PilotState::new(rows[0].pos, rows[0].input.yaw);
    s.mode = PilotMove::Ground;
    let out = |s: &PilotState, t: f32, phase: &str| {
        println!(
            "{t:.4}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{phase}",
            s.pos.x,
            s.pos.y,
            s.pos.z,
            s.vel.x,
            s.vel.y,
            if s.on_ground() { 0.0 } else { s.vel.z },
            s.pitch.to_degrees(),
            s.yaw.to_degrees(),
            s.on_ground() as u8
        )
    };
    println!("t\tx\ty\tz\tvx\tvy\tvz\tpitch\tyaw\tog\tphase");
    out(&s, rows[0].t, &rows[0].phase);
    match mode {
        "frame" | "sub120" => {
            for r in &rows[1..] {
                let dt = r.dt.clamp(1e-4, 0.1);
                let n = if mode == "sub120" { (dt * 120.0).ceil().max(1.0) as usize } else { 1 };
                for k in 0..n {
                    step(&mut s, &PilotInput { jump: r.input.jump && k == 0, ..r.input }, &p, &w, dt / n as f32);
                }
                out(&s, r.t, &r.phase);
            }
        }
        hz => {
            let hz: f32 = hz.parse().expect("frame, sub120 or a tick rate");
            let dt = 1.0 / hz;
            let mut t = rows[0].t;
            let mut i = 0;
            let mut jump_pending = false;
            while i + 1 < rows.len() {
                t += dt;
                // The input of the last frame at or before this tick; a jump press in any frame
                // since the last tick fires on this one.
                while i + 1 < rows.len() && rows[i + 1].t <= t {
                    i += 1;
                    jump_pending |= rows[i].input.jump;
                }
                let r = &rows[i];
                step(&mut s, &PilotInput { jump: std::mem::take(&mut jump_pending), ..r.input }, &p, &w, dt);
                out(&s, t, &r.phase);
            }
        }
    }
}
