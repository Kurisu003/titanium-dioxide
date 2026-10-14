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
//!
//! `moveref <dump> prog <programs.tsv> <outdir> [fps] [latency] [name,...]` runs the capture-2
//! input programs (TD.MoveRef's sh_td_moveref.gnut, one row per program: `name spot limit
//! program`) the way the mod's client script does, against the simulation: one step per frame
//! at `fps` (default 143, the real game's client rate), the program advanced after each frame
//! on the state just simulated, its commands reaching the step `latency` frames later (default
//! 1). Writes `<outdir>/<name>.sim.tsv`: `t x y z vx vy vz pitch yaw og phase frame ev eyez`
//! (`ev`: the program events of that frame, `kind:arg` joined by `|`).

use std::collections::VecDeque;
use std::fmt::Write as _;
use tf_sim::collision::CollisionWorld;
use tf_sim::glam::Vec3;
use tf_sim::engine_move::{self, Cmd, Pm};
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
    if a[2] == "prog" {
        // `fps` may also be a file with one frame time (seconds) per line: the real capture's
        // client frames, replayed one for one (the last one repeats when they run out).
        let (fps, dts): (f32, Option<Vec<f32>>) = match a.get(5) {
            Some(x) if std::path::Path::new(x).is_file() => {
                let v: Vec<f32> = std::fs::read_to_string(x).expect("dt file").lines().filter_map(|l| l.trim().parse().ok()).collect();
                (v.len() as f32 / v.iter().sum::<f32>(), Some(v))
            }
            Some(x) => (x.parse().expect("fps"), None),
            None => (143.0, None),
        };
        let lat: usize = a.get(6).map(|s| s.parse().expect("latency")).unwrap_or(1);
        let only: Option<Vec<&str>> = a.get(7).map(|s| s.split(',').collect());
        let text = std::fs::read_to_string(&a[3]).expect("programs file");
        std::fs::create_dir_all(&a[4]).expect("outdir");
        for line in text.lines().skip(1) {
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() < 4 || only.as_ref().is_some_and(|o| !o.contains(&c[0])) {
                continue;
            }
            let dts = dts.as_ref().and_then(|_| {
                // per-program frame times: <dtfile dir>/<name>.dts if present, else the shared list
                let per = std::path::Path::new(&a[5]).with_file_name(format!("{}.dts", c[0]));
                std::fs::read_to_string(per).ok().map(|t| t.lines().filter_map(|l| l.trim().parse().ok()).collect::<Vec<f32>>())
            }).or_else(|| dts.clone());
            let out = run_program(&w, c[1], c[3], fps, lat, dts.as_deref());
            std::fs::write(std::path::Path::new(&a[4]).join(format!("{}.sim.tsv", c[0])), out).expect("write");
        }
        return;
    }
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

/// The keys a program holds (console buttons) and the turn rate.
#[derive(Default, Clone, Copy)]
struct Keys {
    forward: bool,
    back: bool,
    moveleft: bool,
    moveright: bool,
    speed: bool,
    jump: bool,
    duck: bool,
    left: bool,
    right: bool,
}

#[derive(Clone, Copy)]
enum Wait {
    None,
    Frames(u32),
    Until(f32),
    Air(f32),
    Land(f32),
    Apex(f32),
    Below(f32, f32),
}

/// The simulation a program runs through: the old Pilot sim, or the engine port
/// (`TF_ENGINE_MOVE=1`).
enum Mover {
    Old(PilotState),
    Eng(Pm, engine_move::Settings, engine_move::Cvars),
}

impl Mover {
    fn new(pos: Vec3, yaw_deg: f32) -> Self {
        if std::env::var("TF_ENGINE_MOVE").is_ok_and(|v| v == "1") {
            let mut pm = Pm::new(pos, yaw_deg);
            pm.vel = Vec3::ZERO;
            Mover::Eng(pm, engine_move::Settings::default(), engine_move::Cvars::default())
        } else {
            let mut s = PilotState::new(pos, yaw_deg.to_radians());
            s.mode = PilotMove::Air;
            Mover::Old(s)
        }
    }
    fn pos(&self) -> Vec3 {
        match self {
            Mover::Old(s) => s.pos,
            Mover::Eng(p, ..) => p.origin,
        }
    }
    fn vel(&self) -> Vec3 {
        match self {
            Mover::Old(s) => s.vel,
            Mover::Eng(p, ..) => p.vel,
        }
    }
    fn on_ground(&self) -> bool {
        match self {
            Mover::Old(s) => s.on_ground(),
            Mover::Eng(p, ..) => p.on_ground,
        }
    }
    fn eye_offset(&self, par: &PilotParams) -> f32 {
        match self {
            Mover::Old(s) => s.eye_offset(par),
            Mover::Eng(p, ..) => if p.flags & 2 != 0 { 32.0 } else { 60.0 },
        }
    }
    /// One frame: `keys` are the held keys, `yaw` degrees, `t` the game time after the frame.
    #[allow(clippy::too_many_arguments)]
    fn step(&mut self, w: &CollisionWorld, par: &PilotParams, keys: &Keys, latched: bool, jump_press: bool, yaw: f32, t: f32, dt: f32) {
        let fwd = keys.forward as i32 as f32 - keys.back as i32 as f32;
        let right = keys.moveright as i32 as f32 - keys.moveleft as i32 as f32;
        match self {
            Mover::Old(s) => {
                let input = PilotInput { forward: fwd, right, yaw: yaw.to_radians(), pitch: 0.0, sprint: latched, crouch: keys.duck, jump: jump_press, ads: false };
                step(s, &input, par, w, dt);
            }
            Mover::Eng(p, st, cv) => {
                let mut b = 0u32;
                if keys.forward { b |= engine_move::IN_FORWARD; }
                if keys.back { b |= engine_move::IN_BACK; }
                if keys.moveleft { b |= engine_move::IN_MOVELEFT; }
                if keys.moveright { b |= engine_move::IN_MOVERIGHT; }
                if keys.speed { b |= engine_move::IN_SPEED; }
                if keys.jump { b |= engine_move::IN_JUMP; }
                if keys.duck { b |= engine_move::IN_DUCK; }
                let cmd = Cmd { buttons: b, fmove: fwd, smove: right, yaw, pitch: 0.0 };
                engine_move::player_move(p, &cmd, st, cv, w, t, dt);
            }
        }
    }
}

/// One capture-2 program from `spot` ("x y z yawdeg"), as cl_td_moveref.gnut runs it: each
/// frame the Pilot steps, the frame is logged, then the program advances on that state.
fn run_program(w: &CollisionWorld, spot: &str, prog: &str, fps: f32, lat: usize, dts: Option<&[f32]>) -> String {
    let sp: Vec<f32> = spot.split_whitespace().map(|x| x.parse().expect("spot")).collect();
    let p = PilotParams::default();
    let dt = 1.0 / fps;
    let mut yaw = sp[3];
    let mut s = Mover::new(Vec3::new(sp[0], sp[1], sp[2]), yaw);
    // The server's teleport: dropped at the spot, standing still until settled.
    let mut t = -0.5f32;
    for _ in 0..(0.5 * fps) as usize {
        t += dt;
        s.step(w, &p, &Keys::default(), false, false, yaw, t, dt);
    }
    let settle_end = 0.0f32;
    let _ = t;
    let steps: Vec<Vec<&str>> = prog.split(';').map(|x| x.split_whitespace().collect()).collect();
    let mut pc = 0;
    let mut wait = Wait::None;
    let mut keys = Keys::default();
    let mut yawspeed = 210.0f32;
    let mut pending: VecDeque<(usize, String)> = VecDeque::new();
    let mut latched = false;
    let mut jump_prev = false;
    let mut phase = String::from("start");
    let mut out = String::from("t\tx\ty\tz\tvx\tvy\tvz\tpitch\tyaw\tog\tphase\tframe\tev\teyez\n");
    let mut ended: Option<usize> = None;
    let mut t = settle_end;
    let mut frame = 0usize;
    loop {
        let mut ev: Vec<String> = Vec::new();
        if frame > 0 {
            let dt = dts.map(|d| d[(frame - 1).min(d.len() - 1)]).unwrap_or(dt);
            // The commands that reach this frame's user command.
            let mut pressed_speed = false;
            while pending.front().is_some_and(|(f, _)| *f <= frame) {
                let (_, c) = pending.pop_front().unwrap();
                if let Some(v) = c.strip_prefix("cl_yawspeed ") {
                    yawspeed = v.trim().parse().unwrap_or(yawspeed);
                    continue;
                }
                let (on, name) = match c.as_bytes().first() {
                    Some(b'+') => (true, &c[1..]),
                    Some(b'-') => (false, &c[1..]),
                    _ => continue,
                };
                match name {
                    "forward" => keys.forward = on,
                    "back" => keys.back = on,
                    "moveleft" => keys.moveleft = on,
                    "moveright" => keys.moveright = on,
                    "speed" => {
                        pressed_speed |= on && !keys.speed;
                        keys.speed = on
                    }
                    "jump" => keys.jump = on,
                    "duck" => keys.duck = on,
                    "left" => keys.left = on,
                    "right" => keys.right = on,
                    _ => {}
                }
            }
            // CL_AdjustAngles: +left/+right turn at cl_yawspeed (cl_anglespeedkey 0.67 with +speed).
            let turn = keys.left as i32 as f32 - keys.right as i32 as f32;
            yaw += yawspeed * dt * turn * if keys.speed { 0.67 } else { 1.0 };
            let fwd = keys.forward as i32 as f32 - keys.back as i32 as f32;
            // Sprint latches as in the viewer (and make_replay.py): a press starts it, letting go
            // of forward ends it; crouching does not (the sim stops sprinting while crouched).
            if pressed_speed || (keys.speed && fwd > 0.5) {
                latched = true;
            }
            if fwd < 0.5 {
                latched = false;
            }
            let jump_press = keys.jump && !jump_prev;
            jump_prev = keys.jump;
            t += dt;
            s.step(w, &p, &keys, latched, jump_press, yaw, t, dt);
        }
        // Advance the program on this frame's state.
        if ended.is_none() {
            let og = s.on_ground();
            let done = |wait: &mut Wait, ev: &mut Vec<String>| -> bool {
                let timed_out = |until: f32, ev: &mut Vec<String>, k: u32| {
                    if t >= until - 1e-6 {
                        ev.push(format!("timeout:{k}"));
                        true
                    } else {
                        false
                    }
                };
                match *wait {
                    Wait::None => true,
                    Wait::Frames(n) => {
                        *wait = Wait::Frames(n.saturating_sub(1));
                        n <= 1
                    }
                    Wait::Until(u) => t >= u - 1e-6,
                    Wait::Air(u) => timed_out(u, ev, 3) || !og,
                    Wait::Land(u) => timed_out(u, ev, 4) || og,
                    Wait::Below(z, u) => timed_out(u, ev, 5) || (!og && s.vel().z < 0.0 && s.pos().z < z),
                    Wait::Apex(u) => timed_out(u, ev, 6) || (!og && s.vel().z <= 0.0),
                }
            };
            let mut go = match wait {
                Wait::None => true,
                _ => done(&mut wait, &mut ev),
            };
            while go {
                wait = Wait::None;
                let Some(st) = steps.get(pc) else {
                    // The end: every key let go.
                    for c in ["-forward", "-back", "-moveleft", "-moveright", "-speed", "-jump", "-duck", "-left", "-right"] {
                        pending.push_back((frame + lat, c.to_string()));
                    }
                    ev.push("end:done".into());
                    ended = Some(frame);
                    break;
                };
                pc += 1;
                if st.is_empty() {
                    continue;
                }
                let arg = |i: usize| st.get(i).copied().unwrap_or("0");
                match st[0] {
                    "in" => {
                        let c = st[1..].join(" ");
                        for x in c.split(',') {
                            pending.push_back((frame + lat, x.trim().to_string()));
                        }
                        ev.push(format!("in:{c}"));
                    }
                    "ph" => {
                        phase = arg(1).to_string();
                        ev.push(format!("phase:{phase}"));
                    }
                    "mark" => ev.push(format!("mark:{}", arg(1))),
                    "f" => {
                        let n: u32 = arg(1).parse().unwrap_or(0);
                        if n > 0 {
                            wait = Wait::Frames(n);
                            go = false;
                        }
                    }
                    "t" => {
                        wait = Wait::Until(t + arg(1).parse::<f32>().unwrap_or(0.0));
                        go = false;
                    }
                    "air" | "land" | "apex" | "below" => {
                        let (z, tm) = if st[0] == "below" { (arg(1).parse().unwrap_or(0.0), arg(2)) } else { (0.0, arg(1)) };
                        let until = t + tm.parse::<f32>().unwrap_or(0.0);
                        wait = match st[0] {
                            "air" => Wait::Air(until),
                            "land" => Wait::Land(until),
                            "apex" => Wait::Apex(until),
                            _ => Wait::Below(z, until),
                        };
                        go = done(&mut wait, &mut ev);
                    }
                    op => ev.push(format!("badop:{op}")),
                }
            }
        }
        let _ = writeln!(
            out,
            "{t:.4}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{phase}\t{frame}\t{}\t{:.3}",
            s.pos().x,
            s.pos().y,
            s.pos().z,
            s.vel().x,
            s.vel().y,
            if s.on_ground() { 0.0 } else { s.vel().z },
            0.0,
            yaw,
            s.on_ground() as u8,
            if ev.is_empty() { "-".to_string() } else { ev.join("|") },
            s.eye_offset(&p)
        );
        frame += 1;
        if ended.is_some_and(|e| frame > e + lat + 10) || t > 40.0 {
            break;
        }
    }
    out
}
