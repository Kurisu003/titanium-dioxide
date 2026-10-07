//! Dialogue: BT's gameplay lines and the Frontier Defense commander, with the game's own
//! closed captions as subtitles.
//!
//! Captions come from `resource/subtitles_english.dat`, a Valve VCCD file: header "VCCD",
//! version 1, block count, block size, directory size, data offset; then 12-byte directory
//! entries (CRC32 of the lowercased token, block, offset, length) pointing at UTF-16 text in
//! the data blocks. Tokens are the dialogue source names without their channel/version suffix
//! (`diag_gs_titanbt_coreburstready_01`), so a line is looked up by the source that played.
//!
//! The announcer follows the wave game: the FD commander's intro for the difficulty, the
//! wave-start prefix (first / new / final wave) and the incoming-Titans line, the wave
//! victory, and the defeat; the Titanfall-ready nag when a replacement BT is ready.

use bevy::prelude::*;
use std::collections::{HashMap, VecDeque};

/// Lines decoded at load so the first one doesn't stall.
pub const PRELOAD: &[&str] = &[
    "diag_mcor_cmdr_fd_introEasy",
    "diag_mcor_cmdr_fd_introMedium",
    "diag_mcor_cmdr_fd_introHard",
    "diag_mcor_cmdr_fd_firstWaveStartPrefix",
    "diag_mcor_cmdr_fd_newWaveStartPrefix",
    "diag_mcor_cmdr_fd_finalWaveStartPrefix",
    "diag_mcor_cmdr_fd_waveTypeTitanReg",
    "diag_mcor_cmdr_fd_waveTypeInfantry",
    "diag_mcor_cmdr_fd_waveVictory",
    "diag_mcor_cmdr_fd_matchDefeat",
    "diag_mcor_cmdr_fd_titanReadyNag",
    "diag_gs_titanBt_coreOnline",
    "diag_gs_titanBt_coreActivated",
    "diag_gs_titanBt_doomState",
    "diag_gs_titanBt_briefCriticalDamage",
];

/// Closed-caption text by token.
#[derive(Default)]
pub struct Captions {
    by_crc: HashMap<u32, String>,
}

fn crc32(s: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in s {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Strip `<clr:...>`-style markup and trailing NULs; `<cr>` (a new speaker's line) becomes a
/// line break.
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut tag: Option<String> = None;
    for ch in s.chars() {
        match (&mut tag, ch) {
            (None, '<') => tag = Some(String::new()),
            (Some(t), '>') => {
                if t.eq_ignore_ascii_case("cr") {
                    out.push('\n');
                }
                tag = None;
            }
            (Some(t), c) => t.push(c),
            (None, '\0') => {}
            (None, c) => out.push(c),
        }
    }
    out.trim().to_string()
}

impl Captions {
    pub fn load(gd: &crate::gamedata::GameData) -> Self {
        let Ok(d) = gd.read_file("resource/subtitles_english.dat") else {
            log::warn!("no closed captions");
            return Self::default();
        };
        let i32_at = |o: usize| d.get(o..o + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
        if d.len() < 24 || &d[0..4] != b"VCCD" {
            return Self::default();
        }
        let (block_size, entries, data) = (i32_at(12) as usize, i32_at(16) as usize, i32_at(20) as usize);
        let mut by_crc = HashMap::new();
        for k in 0..entries {
            let e = 24 + k * 12;
            if e + 12 > d.len() {
                break;
            }
            let crc = u32::from_le_bytes(d[e..e + 4].try_into().unwrap());
            let block = i32_at(e + 4) as usize;
            let off = u16::from_le_bytes([d[e + 8], d[e + 9]]) as usize;
            let len = u16::from_le_bytes([d[e + 10], d[e + 11]]) as usize;
            let at = data + block * block_size + off;
            let Some(bytes) = d.get(at..at + len) else { continue };
            let utf16: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            by_crc.insert(crc, plain(&String::from_utf16_lossy(&utf16)));
        }
        Self { by_crc }
    }

    pub fn len(&self) -> usize {
        self.by_crc.len()
    }

    /// Caption for a source or event name: the name itself, then with trailing `_xxx`
    /// segments (channels, version, variant) removed one at a time.
    pub fn find(&self, name: &str) -> Option<String> {
        let mut n = name.to_ascii_lowercase();
        for _ in 0..6 {
            if let Some(t) = self.by_crc.get(&crc32(n.as_bytes())) {
                return Some(t.clone());
            }
            let cut = n.rfind('_')?;
            n.truncate(cut);
        }
        None
    }
}

/// Lines waiting their turn (dialogue never talks over itself).
#[derive(Default)]
pub struct Queue {
    lines: VecDeque<(&'static str, f32)>,
    state: Option<crate::game::GameState>,
    wave: u32,
    titan_ready: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn announcer(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<crate::game::Game>,
    difficulty: Res<crate::vitals::Difficulty>,
    mixer: Res<crate::audio::Mixer>,
    titanfall: Query<&crate::pilotctl::Titanfall>,
    mut q: Local<Queue>,
) {
    use crate::game::GameState::*;
    let now = time.elapsed_secs();
    let state = game.state;
    if q.state != Some(state) || q.wave != game.wave {
        let prev = q.state;
        q.state = Some(state);
        q.wave = game.wave;
        match state {
            Intermission if game.wave == 0 && prev != Some(Intermission) => {
                q.lines.clear();
                let intro = match *difficulty {
                    crate::vitals::Difficulty::Easy => "diag_mcor_cmdr_fd_introEasy",
                    crate::vitals::Difficulty::Normal => "diag_mcor_cmdr_fd_introMedium",
                    _ => "diag_mcor_cmdr_fd_introHard",
                };
                q.lines.push_back((intro, now));
            }
            Playing if prev != Some(Playing) => {
                let prefix = if game.wave <= 1 {
                    "diag_mcor_cmdr_fd_firstWaveStartPrefix"
                } else if game.wave % 5 == 0 {
                    "diag_mcor_cmdr_fd_finalWaveStartPrefix"
                } else {
                    "diag_mcor_cmdr_fd_newWaveStartPrefix"
                };
                q.lines.push_back((prefix, now));
                q.lines.push_back(("diag_mcor_cmdr_fd_waveTypeTitanReg", now));
            }
            Intermission if prev == Some(Playing) => {
                q.lines.clear();
                q.lines.push_back(("diag_mcor_cmdr_fd_waveVictory", now));
            }
            GameOver => {
                q.lines.clear();
                q.lines.push_back(("diag_mcor_cmdr_fd_matchDefeat", now));
            }
            _ => {}
        }
    }
    // A replacement Titan is ready to call in.
    if let Ok(tf) = titanfall.single() {
        let ready = tf.rebuilding && tf.cooldown <= 0.0;
        if ready && !q.titan_ready && matches!(state, Playing | Intermission) {
            q.lines.push_back(("diag_mcor_cmdr_fd_titanReadyNag", now));
        }
        q.titan_ready = ready;
    }
    // One line at a time, a beat after the previous one; stale lines are dropped.
    q.lines.retain(|(_, t)| now - t < 12.0);
    if now >= mixer.dialogue_until + 0.4 {
        if let Some((line, _)) = q.lines.pop_front() {
            crate::audio::event(&mut commands, line);
        }
    }
}
