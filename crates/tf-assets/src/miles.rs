//! Titanfall 2's Miles Sound System 10 banks: `r2/sound/general.mbnk` (version 13) lists every
//! audio source; the audio itself lives in the `.mstr` stream banks as Bink Audio ("1FCB").
//!
//! Layout from LegionPlus (r-ex/LegionPlus, `MilesLib.cpp`, GPLv3):
//! - mbnk: source table offset at 0x48, name table at 0x70, per-language source count at
//!   0x9C, language-independent source count at 0xA0. The table holds the shared sources,
//!   followed by one block of localized sources per language.
//! - Source entry (0x58 bytes): name offset 0x10, sample rate 0x14, channels 0x18, stream
//!   header size/data size at 0x30/0x34, header offset 0x38, data offset 0x40, language 0x50
//!   (i16, -1 = none), patch index 0x52.
//! - mstr header: "RTSC", u16 version, i16 language, u32 data offset, u16 patch index.
//!   A source's Bink Audio file is its header/preload bytes (at the header offset) followed by
//!   the rest of the stream (at data offset + the bank's data offset).
//!
//! Sound events (found by inspecting the v13 bank): the event table at 0x60 has 8-byte entries
//! (name offset in the string table, record offset into the 0xB0-byte records at 0x68); a
//! record's u32 at 0x3C points into the sound-node data at 0x80. A node whose first byte is 1
//! plays one source (name offset at +0x20; 0x28 bytes); one whose first byte is 0 is a
//! container (second byte: weight, third: kind, 0x01 random or 0x83 for dialogue) picking
//! among the children listed as u32 offsets at +8, their count in the fourth byte. Child
//! offsets are relative to the record's root node (checked on the FD commander's nested
//! containers), not to the container.
//!
//! Event record fields (0xB0 bytes; records whose first u32 is below 0x1000 carry a 12-byte
//! prefix, and the fields below start after it), compared across events with known behaviour:
//! - 0x00 flags; bit 0x10 set = 2D (first-person / UI, not positioned in the world).
//! - 0x18 f32 linear volume.
//! - 0x38 u16 playback mode: 1 one-shot; 0 and small values 2..=61 loop (emitters, weapon fire
//!   loops, stim/cloak/vortex sustains); larger values are not modes (dialogue records).
//! - 0x3C u32 root sound node (see above).
//! - 0x54 f32 distance within which the sound is at full volume (rifle 30-50, explosion 150)
//!   and 0x64 f32 the distance at which it is silent (rifle fire 200, footsteps 400,
//!   explosions 1500), both read as feet; 0 on 2D sounds.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub header_offset: u64,
    pub header_size: u32,
    pub data_offset: u64,
    pub data_size: u32,
    pub language: i16,
    pub patch: u16,
}

struct StreamBank {
    path: PathBuf,
    data_offset: u64,
}

/// What a sound event plays and how.
#[derive(Clone, Debug, Default)]
pub struct EventInfo {
    pub sources: Vec<usize>,
    /// Linear volume.
    pub volume: f32,
    /// Positioned in the world (false for first-person and UI sounds).
    pub spatial: bool,
    pub looped: bool,
    /// Falloff start and silent distance in game units (inches), if the record has them.
    pub min_distance: Option<f32>,
    pub max_distance: Option<f32>,
}

pub struct MilesBank {
    pub version: u32,
    pub sources: Vec<Source>,
    by_name: std::collections::BTreeMap<String, usize>,
    /// Sound event name (lowercase) -> what it plays.
    events: HashMap<String, EventInfo>,
    streams: HashMap<(i16, u16), StreamBank>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

impl MilesBank {
    /// Open `general.mbnk` in `dir` (the game's `r2/sound`) with English for voiced sources.
    pub fn open(dir: &Path) -> Result<Self> {
        let bank = std::fs::read(dir.join("general.mbnk")).context("general.mbnk")?;
        if &bank[0..4] != b"KNBC" {
            bail!("not a Miles bank");
        }
        let version = u32_at(&bank, 4);
        if !(12..=13).contains(&version) {
            bail!("unsupported mbnk version {version}");
        }
        let table = u64_at(&bank, 0x48) as usize;
        let names = u64_at(&bank, 0x70) as usize;
        let per_language = u32_at(&bank, 0x9C) as usize;
        let shared = u32_at(&bank, 0xA0) as usize;
        // Shared sources plus the English block (language 0) directly after them.
        let count = shared + per_language;
        let mut sources = Vec::with_capacity(count);
        for i in 0..count {
            let e = table + i * 0x58;
            if e + 0x58 > bank.len() {
                break;
            }
            let name_at = names + u32_at(&bank, e + 0x10) as usize;
            let end = bank[name_at..].iter().position(|&c| c == 0).unwrap_or(0);
            let language = u16_at(&bank, e + 0x50) as i16;
            if language != -1 && language != 0 {
                continue;
            }
            sources.push(Source {
                name: String::from_utf8_lossy(&bank[name_at..name_at + end]).to_ascii_lowercase(),
                sample_rate: u16_at(&bank, e + 0x14) as u32,
                channels: bank[e + 0x18] as u32,
                header_size: u32_at(&bank, e + 0x30),
                data_size: u32_at(&bank, e + 0x34),
                header_offset: u64_at(&bank, e + 0x38),
                data_offset: u64_at(&bank, e + 0x40),
                language,
                patch: u16_at(&bank, e + 0x52),
            });
        }
        let by_name: std::collections::BTreeMap<String, usize> = sources.iter().enumerate().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect();
        let events = parse_events(&bank, names, &by_name);
        let mut streams = HashMap::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "mstr") {
                continue;
            }
            let mut h = [0u8; 16];
            File::open(&path)?.read_exact(&mut h)?;
            if &h[0..4] != b"RTSC" {
                continue;
            }
            let language = u16_at(&h, 6) as i16;
            let patch = u16_at(&h, 12);
            streams.insert((language, patch), StreamBank { path, data_offset: u32_at(&h, 8) as u64 });
        }
        Ok(Self { version, sources, by_name, events, streams })
    }

    /// The sources a named sound event (from animation events or scripts) plays.
    pub fn event(&self, name: &str) -> Option<&[usize]> {
        self.events.get(&name.to_ascii_lowercase()).map(|v| v.sources.as_slice()).filter(|v| !v.is_empty())
    }

    /// An event's playback data (volume, 2D/3D, looping, distances).
    pub fn event_info(&self, name: &str) -> Option<&EventInfo> {
        self.events.get(&name.to_ascii_lowercase()).filter(|v| !v.sources.is_empty())
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Sources whose names start with `prefix` (lowercase).
    pub fn with_prefix(&self, prefix: &str) -> Vec<usize> {
        let p = prefix.to_ascii_lowercase();
        let mut names: Vec<(&String, &usize)> = self.by_name.range(p.clone()..).take_while(|(n, _)| n.starts_with(&p)).collect();
        names.sort();
        names.into_iter().map(|(_, &i)| i).collect()
    }

    /// The sources a sound event (as named by animation events and scripts) plays: the bank
    /// stores them as `<event>_<channels>_v<version>_<variant>`, sometimes without the event's
    /// `_1p`/`_3p` suffix.
    pub fn event_variants(&self, event: &str) -> Vec<usize> {
        if let Some(v) = self.event(event) {
            return v.to_vec();
        }
        let e = event.to_ascii_lowercase();
        let mut tries = vec![format!("{e}_")];
        for suffix in ["_1p", "_3p", "_1p_vs_3p"] {
            if let Some(base) = e.strip_suffix(suffix) {
                tries.push(format!("{base}_"));
            }
        }
        for t in tries {
            let v = self.with_prefix(&t);
            if !v.is_empty() {
                return v;
            }
        }
        self.by_name.get(&e).map(|&i| vec![i]).unwrap_or_default()
    }

    pub fn find(&self, name: &str) -> Option<&Source> {
        self.by_name.get(&name.to_ascii_lowercase()).map(|&i| &self.sources[i])
    }

    /// The complete Bink Audio ("1FCB") file for a source.
    pub fn read_binka(&self, s: &Source) -> Result<Vec<u8>> {
        let bank = self.streams.get(&(s.language, s.patch)).with_context(|| format!("no stream bank for language {} patch {}", s.language, s.patch))?;
        let mut f = File::open(&bank.path)?;
        let mut out = vec![0u8; s.header_size as usize];
        f.seek(SeekFrom::Start(s.header_offset))?;
        f.read_exact(&mut out)?;
        if out.len() < 24 || &out[0..4] != b"1FCB" {
            bail!("{}: not Bink Audio", s.name);
        }
        // The 1FCB header's total size (at 16) covers preload + streamed parts.
        let total = u32_at(&out, 16) as usize;
        let rest = total.saturating_sub(out.len());
        if rest > 0 {
            let start = out.len();
            out.resize(start + rest, 0);
            f.seek(SeekFrom::Start(s.data_offset + bank.data_offset))?;
            f.read_exact(&mut out[start..])?;
        }
        Ok(out)
    }
}

fn cstr_at(b: &[u8], o: usize) -> Option<String> {
    let end = b.get(o..)?.iter().position(|&c| c == 0)?;
    Some(String::from_utf8_lossy(&b[o..o + end]).to_ascii_lowercase())
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// A float field that holds a plausible value (not an offset or index read as a float).
fn plausible(v: f32, max: f32) -> Option<f32> {
    (v.is_finite() && v > 1e-3 && v < max).then_some(v)
}

fn parse_events(bank: &[u8], names: usize, by_name: &std::collections::BTreeMap<String, usize>) -> HashMap<String, EventInfo> {
    let mut out = HashMap::new();
    if bank.len() < 0xB0 {
        return out;
    }
    let (table, records, nodes) = (u64_at(bank, 0x60) as usize, u64_at(bank, 0x68) as usize, u64_at(bank, 0x80) as usize);
    let count = u32_at(bank, 0xA8) as usize;
    fn walk(bank: &[u8], names: usize, by_name: &std::collections::BTreeMap<String, usize>, root: usize, at: usize, depth: u32, out: &mut Vec<usize>) {
        if depth > 6 || at + 0x28 > bank.len() {
            return;
        }
        if bank[at] == 1 {
            if let Some(i) = cstr_at(bank, names + u32_at(bank, at + 0x20) as usize).and_then(|n| by_name.get(&n).copied()) {
                out.push(i);
            }
        } else if bank[at] == 0 && bank[at + 2] != 0 {
            for k in 0..(bank[at + 3] as usize).min(32) {
                let child = u32_at(bank, at + 8 + k * 4) as usize;
                if child > 0 && child < 0x10000 {
                    walk(bank, names, by_name, root, root + child, depth + 1, out);
                }
            }
        }
    }
    for i in 0..count {
        let e = table + i * 8;
        if e + 8 > records.min(bank.len()) {
            break;
        }
        let Some(name) = cstr_at(bank, names + u32_at(bank, e) as usize) else { continue };
        let mut r = records + u32_at(bank, e + 4) as usize;
        if r + 0xBC > bank.len() {
            continue;
        }
        if u32_at(bank, r) < 0x1000 {
            r += 12;
        }
        let mut srcs = Vec::new();
        let root = nodes + u32_at(bank, r + 0x3C) as usize;
        walk(bank, names, by_name, root, root, 0, &mut srcs);
        let flags = u32_at(bank, r);
        let mode = u16_at(bank, r + 0x38);
        const FEET: f32 = 12.0;
        out.insert(
            name,
            EventInfo {
                sources: srcs,
                volume: plausible(f32_at(bank, r + 0x18), 16.0).unwrap_or(1.0),
                spatial: flags & 0x10 == 0 || plausible(f32_at(bank, r + 0x64), 1e5).is_some(),
                looped: mode != 1 && mode <= 61,
                min_distance: plausible(f32_at(bank, r + 0x54), 1e5).map(|v| v * FEET),
                max_distance: plausible(f32_at(bank, r + 0x64), 1e5).map(|v| v * FEET),
            },
        );
    }
    out
}
