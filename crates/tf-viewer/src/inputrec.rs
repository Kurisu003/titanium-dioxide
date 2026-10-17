//! Input recording and replay (testing). `TF_RECORD=<file>` writes the player's input every
//! frame, timed from the end of loading, with BT's and the Pilot's state after it for reading
//! back what happened. `TF_REPLAY=<file>` plays a recording back in place of the keyboard,
//! mouse and pad, so a play session can be re-run with `--shots`.
//!
//! One line per frame: `t yaw pitch forward right flags nav slot kit | state...`. Replay reads
//! only the part before `|`. Frames are matched by time: held buttons take the newest recorded
//! frame at or before the replay's time, one-shot presses fire once if any frame since the last
//! replayed one had them.

use crate::pilotctl::{Control, PlayerPilot};
use crate::player::{PlayerInput, PlayerTitan};
use bevy::prelude::*;
use std::io::Write;

/// The recorded buttons, in bit order, and whether each is a one-shot press.
const FLAGS: &[(&str, bool)] = &[
    ("sprint", false),
    ("dash", true),
    ("fire", false),
    ("ads", false),
    ("reload", true),
    ("ordnance", false),
    ("core", true),
    ("vortex", false),
    ("utility", false),
    ("melee", true),
    ("jump", true),
    ("crouch", false),
    ("interact", true),
    ("pfx_test", true),
    ("tactical", true),
    ("throw", true),
    ("titanfall", true),
    ("start", true),
    ("pause", true),
    ("pilot_fire", false),
    ("pilot_ads", false),
    ("pilot_reload", true),
    ("sprint_key", false),
    ("crouch_toggle_mode", false),
    ("crouch_toggle_key", false),
];

fn flag_refs(i: &mut PlayerInput) -> [&mut bool; 25] {
    [
        &mut i.sprint,
        &mut i.dash,
        &mut i.fire,
        &mut i.ads,
        &mut i.reload,
        &mut i.ordnance,
        &mut i.core,
        &mut i.vortex,
        &mut i.utility,
        &mut i.melee,
        &mut i.jump,
        &mut i.crouch,
        &mut i.interact,
        &mut i.pfx_test,
        &mut i.tactical,
        &mut i.throw,
        &mut i.titanfall,
        &mut i.start,
        &mut i.pause,
        &mut i.pilot_fire,
        &mut i.pilot_ads,
        &mut i.pilot_reload,
        &mut i.sprint_key,
        &mut i.crouch_toggle_mode,
        &mut i.crouch_toggle_key,
    ]
}

fn opt(v: Option<u8>) -> i32 {
    v.map_or(-1, i32::from)
}

fn unopt(v: i32) -> Option<u8> {
    (v >= 0).then_some(v as u8)
}

#[derive(Clone, Copy)]
struct Frame {
    t: f32,
    yaw: f32,
    pitch: f32,
    forward: f32,
    right: f32,
    flags: u32,
    nav: i32,
    slot: i32,
    kit: i32,
}

impl Frame {
    fn parse(line: &str) -> Option<Self> {
        let head = line.split('|').next()?;
        let v: Vec<&str> = head.split_whitespace().collect();
        if v.len() < 9 {
            return None;
        }
        let f = |i: usize| v[i].parse::<f32>().ok();
        let n = |i: usize| v[i].parse::<i64>().ok();
        Some(Self {
            t: f(0)?,
            yaw: f(1)?,
            pitch: f(2)?,
            forward: f(3)?,
            right: f(4)?,
            flags: n(5)? as u32,
            nav: n(6)? as i32,
            slot: n(7)? as i32,
            kit: n(8)? as i32,
        })
    }
}

/// Write this frame's input (after gathering and gating) and the player's state.
pub fn record_input(
    mut input: ResMut<PlayerInput>,
    loaded_at: Option<Res<crate::ui::LoadedAt>>,
    control: Res<Control>,
    titans: Query<&PlayerTitan>,
    pilots: Query<&PlayerPilot>,
    mut file: Local<Option<Option<std::io::BufWriter<std::fs::File>>>>,
) {
    let file = file.get_or_insert_with(|| {
        let path = std::env::var_os("TF_RECORD")?;
        match std::fs::File::create(&path) {
            Ok(f) => {
                log::info!("recording input to {}", path.to_string_lossy());
                Some(std::io::BufWriter::new(f))
            }
            Err(e) => {
                log::warn!("TF_RECORD {}: {e}", path.to_string_lossy());
                None
            }
        }
    });
    let (Some(w), Some(l)) = (file.as_mut(), loaded_at) else {
        return;
    };
    let mut flags = 0u32;
    for (i, b) in flag_refs(&mut input).into_iter().enumerate() {
        if *b {
            flags |= 1 << i;
        }
    }
    let i = &*input;
    let mut line = format!(
        "{:.4} {:.5} {:.5} {:.2} {:.2} {flags} {} {} {} |",
        l.secs(),
        i.yaw,
        i.pitch,
        i.forward,
        i.right,
        opt(i.menu_nav),
        opt(i.pilot_slot),
        opt(i.titan_kit)
    );
    line += &format!(" ctl={:?}", *control);
    if let Ok(t) = titans.single() {
        let s = &t.state;
        line += &format!(
            " bt=({:.1},{:.1},{:.1}) btvel=({:.1},{:.1},{:.1}) btyaw={:.3} anim={:?}",
            s.pos.x, s.pos.y, s.pos.z, s.vel.x, s.vel.y, s.vel.z, s.yaw, t.override_anim
        );
    }
    if let Ok(p) = pilots.single() {
        let s = &p.state;
        line += &format!(
            " pilot=({:.1},{:.1},{:.1}) pvel=({:.1},{:.1},{:.1})",
            s.pos.x, s.pos.y, s.pos.z, s.vel.x, s.vel.y, s.vel.z
        );
    }
    let names: Vec<&str> = FLAGS
        .iter()
        .enumerate()
        .filter(|(k, _)| flags & (1 << k) != 0)
        .map(|(_, f)| f.0)
        .collect();
    if !names.is_empty() {
        line += &format!(" keys={}", names.join(","));
    }
    let _ = writeln!(w, "{line}");
    let _ = w.flush();
}

#[derive(Default)]
pub struct Replay {
    frames: Vec<Frame>,
    next: usize,
    loaded: bool,
    /// The flags of the last replayed frame (for finding presses).
    prev: u32,
}

/// Replace this frame's input with the recording's (TF_REPLAY).
pub fn replay_input(
    mut input: ResMut<PlayerInput>,
    loaded_at: Option<Res<crate::ui::LoadedAt>>,
    mut replay: Local<Replay>,
) {
    if !replay.loaded {
        replay.loaded = true;
        if let Some(path) = std::env::var_os("TF_REPLAY") {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    replay.frames = text.lines().filter_map(Frame::parse).collect();
                    log::info!(
                        "replaying {} frames from {}",
                        replay.frames.len(),
                        path.to_string_lossy()
                    );
                }
                Err(e) => log::warn!("TF_REPLAY {}: {e}", path.to_string_lossy()),
            }
        }
    }
    if replay.frames.is_empty() {
        return;
    }
    let Some(l) = loaded_at else { return };
    let t = l.secs();
    // Every recorded frame up to now: one-shot presses that start in any of them, the rest from
    // the newest.
    let mut once = 0u32;
    let mut newest = None;
    while replay.next < replay.frames.len() && replay.frames[replay.next].t <= t {
        let f = replay.frames[replay.next];
        once |= f.flags & !replay.prev;
        replay.prev = f.flags;
        newest = Some(f);
        replay.next += 1;
    }
    let base = newest.or_else(|| replay.next.checked_sub(1).map(|k| replay.frames[k]));
    let Some(f) = base else { return };
    input.yaw = f.yaw;
    input.pitch = f.pitch;
    input.forward = f.forward;
    input.right = f.right;
    for (k, b) in flag_refs(&mut input).into_iter().enumerate() {
        let bit = 1 << k;
        *b = if FLAGS[k].1 {
            once & bit != 0
        } else {
            f.flags & bit != 0
        };
    }
    // Menu, weapon slot and kit choices are one-shot too.
    let fresh = newest.is_some();
    input.menu_nav = if fresh { unopt(f.nav) } else { None };
    input.pilot_slot = if fresh { unopt(f.slot) } else { None };
    input.titan_kit = if fresh { unopt(f.kit) } else { None };
}
