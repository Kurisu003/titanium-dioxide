//! Sound from the game's own Miles banks (`r2/sound`): sources are decoded from Bink Audio,
//! downmixed to stereo and cached.
//!
//! Everything plays as the game's sound events where it can: animation events, the events the
//! weapon/ability/pilot scripts name, and BT/announcer dialogue. An event's record supplies its
//! volume, whether it is positioned in the world, its falloff distances and whether it loops
//! (see `tf_assets::miles`). Gameplay code spawns a `SoundCue` (mapped to event names in
//! `Cue::events`, or for weapons to the weapon script's fire events), a `SoundEvent`, or a
//! `SoundLoop`.
//!
//! The mixer: distance falloff and equal-power panning from the camera; occlusion (a ray
//! through the world muffles and low-passes the sound); a voice limit (48, at most 4 of one
//! world event); explosions duck the music, dialogue ducks music and effects. Cues whose
//! events the bank lacks fall back to hand-mapped source names (`Cue::sources`).

use crate::player::{to_bevy, MainCamera};
use bevy::audio::{AddAudioSource, Decodable, Source};
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tf_assets::miles::{EventInfo, MilesBank};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GunPart {
    First,
    Shot,
    Tail,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cue {
    /// A pilot weapon's fire sound (index into pilotweapon::ARSENAL).
    Gun(u8, GunPart),
    /// One of BT's loadouts (index into weapons::TITAN_ARSENAL).
    TitanGun(u8, GunPart),
    /// An enemy Titan's shot with that weapon, heard in the world.
    EnemyGun(u8),
    Xo16First,
    Xo16Shot,
    Xo16Tail,
    Xo16ReloadStart,
    Xo16ReloadMid,
    Xo16ReloadEnd,
    MissileLock,
    MissileFire,
    MissileExplode,
    VortexStart,
    VortexEnd,
    VortexThrow,
    VortexAbsorb,
    BtStep,
    BtDash,
    BtPunch,
    PunchImpact,
    TitanLand,
    TitanDeath,
    EnemyStep,
    SplitterBurst,
    R201First,
    R201Shot,
    R201Tail,
    BtCoreReady,
    BtCoreActivated,
    BtDoomed,
    BtCritical,
    PilotStep,
    PilotJump,
    PilotDoubleJump,
    PilotLand,
    PilotWallrunStart,
    PilotWallrunStep,
    PilotSlide,
    PilotMantle,
    HitBeep,
    /// Plasma Railgun charge (Weapon_Titan_Sniper_WindUp / WindDown / LevelTick_N / _Final).
    RailgunWindUp,
    RailgunWindDown,
    RailgunTick,
    RailgunTickFinal,
    /// Railgun shot by charge tier 1-4 (Weapon_Titan_Sniper_Level_N_1P).
    RailgunFire(u8),
    /// Charge Rifle / Cold War charge (Weapon_ChargeRifle_WindUp_1P / WindDown_1P / TriggerOn).
    ChargeRifleWindUp,
    ChargeRifleWindDown,
    ChargeRifleTrigger,
}

impl Cue {
    /// Source-name prefixes; every source starting with one is a variant. `all` plays every
    /// listed part together (layers) instead of one random variant.
    fn sources(self) -> (&'static [&'static str], bool) {
        use Cue::*;
        match self {
            EnemyGun(i) => {
                let g = &crate::weapons::TITAN_ARSENAL[i as usize];
                return (if g.shot.is_empty() { g.first } else { g.shot }, false);
            }
            TitanGun(i, part) => {
                let g = &crate::weapons::TITAN_ARSENAL[i as usize];
                let pick = |a: &'static [&'static str], b: &'static [&'static str]| if a.is_empty() { b } else { a };
                return match part {
                    GunPart::First => (pick(g.first, g.shot), false),
                    GunPart::Shot => (pick(g.shot, g.first), false),
                    GunPart::Tail => (g.tail, false),
                };
            }
            Gun(i, part) => {
                let s = &crate::pilotweapon::ARSENAL[i as usize].sound;
                let pick = |a: &'static [&'static str], b: &'static [&'static str]| if a.is_empty() { b } else { a };
                return match part {
                    GunPart::First => (pick(s.first, s.shot), false),
                    GunPart::Shot => (pick(s.shot, s.first), false),
                    GunPart::Tail => (s.tail, false),
                };
            }
            Xo16First => (&["wpn_xo16_1p_wpnfire_firstshot_core"], false),
            Xo16Shot => (&["wpn_xo16_1p_wpnfire_secondshot_core"], false),
            Xo16Tail => (&["wpn_xo16_1p_wpnfire_tail_core", "wpn_xo16_1p_wpnfire_tail_mech_"], true),
            Xo16ReloadStart => (&["wpn_xo16_1p_reload_pt1_"], false),
            Xo16ReloadMid => (&["wpn_xo16_1p_reload_pt5_"], false),
            Xo16ReloadEnd => (&["wpn_xo16_1p_reload_pt9_"], false),
            MissileLock => (&["wpn_shoulderrocket_homing_1p_acquiretarget"], false),
            MissileFire => (&["wpn_shoulderrocket_homing_1p_wpnfire_6ch"], false),
            MissileExplode => (&["titan_rocket_explosion_close"], false),
            VortexStart => (&["vortex_3p_shield_start_1_long"], false),
            VortexEnd => (&["vortex_3p_shield_end_1"], false),
            VortexThrow => (&["vortex_1p_shield_throw"], false),
            VortexAbsorb => (&["vortex_shield_absorb_bulletlarge_1ch"], false),
            BtStep => (&["bt_footstep_jog_1p_6ch"], false),
            BtDash => (&["bt_dodge_left_0"], false),
            BtPunch => (&["autotitan_melee_chestpunch_0"], false),
            PunchImpact => (&["titan_bigpunch_impact"], false),
            TitanLand => (&["titan_land_generic_6ch"], false),
            TitanDeath => (&["titan_atlas_deathexplo"], false),
            EnemyStep => (&["atlas_footstep_generic_close_m"], false),
            SplitterBurst => (&["wpn_particleaccel_3p_shot_loop_close", "wpn_particleaccel_3p_ballisticloop_2ch_v2"], true),
            R201First => (&["wpn_r101_1p_wpnfire_firstshot_core"], false),
            R201Shot => (&["wpn_r101_1p_wpnfire_loop_core2"], false),
            R201Tail => (&["wpn_r101_1p_wpnfire_tail_core"], false),
            BtCoreReady => (&["diag_gs_titanbt_coreburstready_01"], false),
            BtCoreActivated => (&["diag_gs_titanbt_coreburstactivated_01"], false),
            BtDoomed => (&["diag_gs_titanbt_doomstate_01"], false),
            BtCritical => (&["diag_gs_titanbt_briefcriticaldamage_0"], false),
            PilotStep => (&["mv_footstep_left", "mv_footstep_right"], false),
            PilotJump => (&["mvmt_human_1p_fs_jump_2ch", "jumpjet_jumpstart_st"], true),
            PilotDoubleJump => (&["jumpjet_jetstart_chuff_st", "jumpjet_jetstart_flame_st"], true),
            PilotLand => (&["mvmt_human_1p_fs_jumpland_gear", "jumpland_concrete"], true),
            PilotWallrunStart => (&["wallrun_start_2ch", "jumpjet_wallrunbody_st"], true),
            PilotWallrunStep => (&["mvmt_human_1p_fs_wallrun_concrete"], false),
            PilotSlide => (&["mvmt_human_1p_crouchdown_2ch"], false),
            PilotMantle => (&["player_mantle_default_0"], false),
            HitBeep => (&["hitbeep2"], false),
            RailgunWindUp => (&["wpn_titansniper_1p_charge_start_2ch"], false),
            RailgunWindDown => (&["wpn_titansniper_1p_charge_end_2ch"], false),
            RailgunTick => (&["wpn_titansniper_1p_bullet_tick_2ch"], false),
            RailgunTickFinal => (&["wpn_titansniper_1p_final_tick_2ch"], false),
            RailgunFire(1) => (&["wpn_titansniper_1p_wpnfire_level1_6ch", "wpn_titansniperrifle_1p_scifi_layer"], true),
            RailgunFire(2) => (&["wpn_titansniper_1p_wpnfire_level2_6ch", "wpn_titansniperrifle_1p_scifi_layer"], true),
            RailgunFire(3) => (&["wpn_titansniper_1p_wpnfire_level4_6ch_v1", "wpn_titansniperrifle_1p_core_bass_layer"], true),
            RailgunFire(_) => (&["wpn_titansniper_1p_wpnfire_level4_6ch_v1", "wpn_titansniperrifle_1p_core_bass_layer", "wpn_titansniper_1p_wpnfire_uber_tail_2ch_v4"], true),
            ChargeRifleWindUp => (&["wpn_chargerifle_1p_chargeup_6ch_v2"], false),
            ChargeRifleWindDown => (&["wpn_chargerifle_1p_chargedown_test1_6ch"], false),
            ChargeRifleTrigger => (&["wpn_chargerifle_1p_trigger_1ch"], false),
        }
    }
    /// The game's sound events for this cue, played together (from the scripts that play
    /// them: pilot_base.set's jumpjet/slide keys, the Vortex, missile and core scripts, BT's
    /// gameplay dialogue). Empty, or any name missing from the bank, uses `sources` instead.
    /// Weapon fire (`Gun`, `TitanGun`, `EnemyGun`) comes from each weapon's script.
    pub fn events(self) -> &'static [&'static str] {
        use Cue::*;
        match self {
            MissileLock => &["ShoulderRocket_Homing_AcquireTarget"],
            MissileFire => &["ShoulderRocket_Homing_Fire_1P"],
            VortexStart => &["vortex_shield_start_1P"],
            VortexEnd => &["vortex_shield_end_1P"],
            VortexThrow => &["vortex_shield_throw_1P"],
            VortexAbsorb => &["Vortex_Shield_AbsorbBulletLarge"],
            BtDash => &["bt_dash_forward_01"],
            PunchImpact => &["titan_melee_hit"],
            TitanDeath => &["titan_death_explode"],
            BtCoreReady => &["diag_gs_titanBt_coreOnline"],
            BtCoreActivated => &["diag_gs_titanBt_coreActivated"],
            BtDoomed => &["diag_gs_titanBt_doomState"],
            BtCritical => &["diag_gs_titanBt_briefCriticalDamage"],
            PilotJump => &["Jumpjet_Jump_Start_1P"],
            PilotDoubleJump => &["Jumpjet_Jet_Start_1P"],
            PilotWallrunStart => &["Jumpjet_Wallrun_Start_1P"],
            PilotSlide => &["jumpjet_slide_start_1p"],
            RailgunWindUp => &["Weapon_Titan_Sniper_WindUp"],
            RailgunWindDown => &["Weapon_Titan_Sniper_WindDown"],
            RailgunFire(1) => &["Weapon_Titan_Sniper_Level_1_1P"],
            RailgunFire(2) => &["Weapon_Titan_Sniper_Level_2_1P"],
            RailgunFire(3) => &["Weapon_Titan_Sniper_Level_3_1P"],
            RailgunFire(_) => &["Weapon_Titan_Sniper_Level_4_1P"],
            ChargeRifleWindUp => &["Weapon_ChargeRifle_WindUp_1P"],
            ChargeRifleWindDown => &["Weapon_ChargeRifle_WindDown_1P"],
            _ => &[],
        }
    }

    /// (full volume within, silent beyond) in game units, and a base gain.
    fn falloff(self) -> (f32, f32, f32) {
        use Cue::*;
        match self {
            MissileExplode | TitanDeath | TitanLand => (600.0, 9000.0, 1.0),
            SplitterBurst => (400.0, 7000.0, 0.8),
            EnemyGun(_) => (400.0, 7000.0, 0.55),
            EnemyStep => (200.0, 3500.0, 0.6),
            VortexAbsorb => (300.0, 3000.0, 0.7),
            BtCoreReady | BtCoreActivated | BtDoomed | BtCritical => (1e9, 1e9, 0.9),
            Gun(_, GunPart::Tail) => (1e9, 1e9, 0.35),
            PilotStep | PilotWallrunStep => (1e9, 1e9, 0.3),
            HitBeep => (1e9, 1e9, 0.35),
            PilotJump | PilotDoubleJump | PilotLand | PilotWallrunStart | PilotSlide | PilotMantle => (1e9, 1e9, 0.45),
            Gun(..) => (1e9, 1e9, 0.5),
            TitanGun(_, GunPart::Tail) => (1e9, 1e9, 0.5),
            TitanGun(..) => (1e9, 1e9, 0.6),
            BtStep => (1e9, 1e9, 0.35),
            Xo16Shot | R201Shot => (1e9, 1e9, 0.45),
            _ => (1e9, 1e9, 0.6),
        }
    }
}

/// Ask for a sound. `at` is a game-space position (None = on the player, no falloff or pan).
#[derive(Component)]
pub struct SoundCue {
    pub cue: Cue,
    pub at: Option<Vec3>,
    pub delay: f32,
    /// Cut the sound off after this long (for looping sources played for a burst).
    pub duration: Option<f32>,
}

pub fn cue(commands: &mut Commands, cue: Cue, at: Option<Vec3>) {
    commands.spawn(SoundCue { cue, at, delay: 0.0, duration: None });
}


/// A sound event by its game name (from animation events and scripts), resolved through the
/// bank's event table. Its volume, 2D/3D placement, falloff distances and looping come from
/// the event record.
#[derive(Component)]
pub struct SoundEvent {
    pub name: String,
    /// Game-space position (None = on the player).
    pub at: Option<Vec3>,
}

pub fn event(commands: &mut Commands, name: &str) {
    commands.spawn(SoundEvent { name: name.to_string(), at: None });
}

/// A sound event in the world.
pub fn event_at(commands: &mut Commands, name: &str, at: Vec3) {
    commands.spawn(SoundEvent { name: name.to_string(), at: Some(at) });
}

/// Start (or keep alive) a looping sound event under `key`, or stop it.
#[derive(Component)]
pub struct SoundLoop {
    pub key: String,
    /// Event to loop (None = stop the loop).
    pub event: Option<String>,
    pub at: Option<Vec3>,
    /// Played when the loop stops.
    pub end: Option<String>,
    /// Stop by itself when not refreshed for this long.
    pub timeout: Option<f32>,
    /// Play once (not looped) but stay stoppable under `key`; a new request restarts it.
    pub once: bool,
}

/// Play `event` once under `key`, so `loop_stop(key)` can cut it short (the jump-jet body
/// tails, which the engine stops at the end of the jump).
pub fn keyed(commands: &mut Commands, key: &str, event: &str) {
    commands.spawn(SoundLoop { key: key.into(), event: Some(event.into()), at: None, end: None, timeout: None, once: true });
}

/// Loop `event` until `loop_stop(key)` (stim and cloak sustains, vortex hum, weapon fire).
#[allow(dead_code)]
pub fn loop_start(commands: &mut Commands, key: &str, event: &str, at: Option<Vec3>) {
    commands.spawn(SoundLoop { key: key.into(), event: Some(event.into()), at, end: None, timeout: None, once: false });
}

/// Start or refresh the loop under `key`; it stops by itself half a second after the last hold
/// (for abilities that hold a sustain while they run).
pub fn loop_hold(commands: &mut Commands, key: &str, event: &str, at: Option<Vec3>) {
    commands.spawn(SoundLoop { key: key.into(), event: Some(event.into()), at, end: None, timeout: Some(0.5), once: false });
}

/// Stop the loop under `key`, playing `end` (if any) as it stops.
pub fn loop_stop(commands: &mut Commands, key: &str, end: Option<&str>) {
    commands.spawn(SoundLoop { key: key.into(), event: None, at: None, end: end.map(str::to_string), timeout: None, once: false });
}

/// Gain for first-person animation sounds (reloads, draws) and other 2D events.
const EVENT_GAIN: f32 = 0.8;
/// Every event plays at its record volume times this (the records run a little hotter than
/// the rest of our mix).
const EVENT_TRIM: f32 = 0.9;
/// At most this many sounds at once; extra world sounds are dropped (Miles' voice limit).
const MAX_VOICES: usize = 48;
/// At most this many instances of one world event at once.
const MAX_INSTANCES: usize = 4;
/// Occluded world sounds: this much quieter and low-passed at this cutoff (Hz).
const OCCLUDED_GAIN: f32 = 0.55;
const OCCLUDED_CUTOFF: f32 = 1400.0;

/// Every cue, for preloading.
pub const ALL_CUES: &[Cue] = &[
    Cue::Xo16First, Cue::Xo16Shot, Cue::Xo16Tail, Cue::Xo16ReloadStart, Cue::Xo16ReloadMid, Cue::Xo16ReloadEnd,
    Cue::MissileLock, Cue::MissileFire, Cue::MissileExplode, Cue::VortexStart, Cue::VortexEnd, Cue::VortexThrow,
    Cue::VortexAbsorb, Cue::BtStep, Cue::BtDash, Cue::BtPunch, Cue::PunchImpact, Cue::TitanLand, Cue::TitanDeath,
    Cue::EnemyStep, Cue::SplitterBurst, Cue::R201First, Cue::R201Shot, Cue::R201Tail, Cue::BtCoreReady,
    Cue::BtCoreActivated, Cue::BtDoomed, Cue::BtCritical, Cue::PilotStep, Cue::PilotJump, Cue::PilotDoubleJump,
    Cue::PilotLand, Cue::PilotWallrunStart, Cue::PilotWallrunStep, Cue::PilotSlide, Cue::PilotMantle, Cue::HitBeep,
    Cue::RailgunWindUp, Cue::RailgunWindDown, Cue::RailgunTick, Cue::RailgunTickFinal, Cue::RailgunFire(1), Cue::RailgunFire(2),
    Cue::RailgunFire(3), Cue::RailgunFire(4), Cue::ChargeRifleWindUp, Cue::ChargeRifleWindDown, Cue::ChargeRifleTrigger,
];

/// Play a cue for at most `duration` seconds (a wind-up cut short when the charge stops).
pub fn cue_for(commands: &mut Commands, cue: Cue, at: Option<Vec3>, duration: f32) {
    commands.spawn(SoundCue { cue, at, delay: 0.0, duration: Some(duration) });
}

/// Decoded, stereo-interleaved PCM played with fixed per-ear gains, optionally low-passed
/// (sounds heard through walls).
#[derive(Asset, TypePath, Clone)]
pub struct Sound {
    pcm: Arc<Vec<f32>>,
    rate: u32,
    gains: [f32; 2],
    /// One-pole low-pass coefficient (1 = off).
    lowpass: f32,
}

impl Sound {
    pub fn new(pcm: Arc<Vec<f32>>, rate: u32, gains: [f32; 2]) -> Self {
        Self { pcm, rate, gains, lowpass: 1.0 }
    }
}

pub struct SoundIter {
    pcm: Arc<Vec<f32>>,
    pos: usize,
    rate: u32,
    gains: [f32; 2],
    lowpass: f32,
    state: [f32; 2],
}

/// The master volume as f32 bits, applied while mixing so it changes sounds already playing
/// (loops, music) too; Bevy's `GlobalVolume` only applies when a sound starts.
static MASTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The player's master volume (0-1): the settings menu, `-` / `=` and `--volume` set it.
#[derive(Resource, Clone, Copy)]
pub struct MasterVolume {
    pub volume: bevy::audio::Volume,
}

impl MasterVolume {
    pub fn new(volume: bevy::audio::Volume) -> Self {
        Self { volume }
    }
}

fn apply_master_volume(master: Res<MasterVolume>) {
    if master.is_changed() {
        MASTER.store(master.volume.to_linear().clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

impl Iterator for SoundIter {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = *self.pcm.get(self.pos)?;
        let ch = self.pos & 1;
        self.pos += 1;
        let y = &mut self.state[ch];
        *y += self.lowpass * (s - *y);
        let master = f32::from_bits(MASTER.load(std::sync::atomic::Ordering::Relaxed));
        Some(*y * self.gains[ch] * master)
    }
}

impl Source for SoundIter {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.pcm.len() as f32 / 2.0 / self.rate as f32))
    }
}

impl Decodable for Sound {
    type DecoderItem = f32;
    type Decoder = SoundIter;
    fn decoder(&self) -> SoundIter {
        SoundIter { pcm: self.pcm.clone(), pos: 0, rate: self.rate, gains: self.gains, lowpass: self.lowpass, state: [0.0; 2] }
    }
}

/// A weapon's fire sounds, from its script (`fire_sound_*`, `burst_or_looping_fire_sound_*`,
/// `looping_sounds`). Event names that the bank doesn't have are left out.
#[derive(Clone, Debug, Default)]
pub struct WeaponSounds {
    /// `looping_sounds` 1 with a middle event: start once, loop the middle while the trigger is
    /// held, end on release.
    pub looping: bool,
    pub start_1p: Vec<String>,
    pub middle_1p: Option<String>,
    pub end_1p: Vec<String>,
    /// Played together on every shot (`fire_sound_1_player_1p` + `fire_sound_2_player_1p`).
    pub shot_1p: Vec<String>,
    /// What others hear (`fire_sound_*_npc`, else `*_player_3p`).
    pub shot_npc: Vec<String>,
}

/// The game's sound bank, the sources decoded so far and per-weapon sound sets.
#[derive(Resource)]
pub struct Sfx {
    bank: Option<Arc<MilesBank>>,
    variants: HashMap<String, Vec<usize>>,
    decoded: HashMap<usize, Option<(Arc<Vec<f32>>, u32)>>,
    infos: HashMap<String, Option<EventInfo>>,
    /// Weapon script name -> its sounds.
    weapons: HashMap<String, WeaponSounds>,
    /// Closed captions (resource/subtitles_english.dat).
    captions: crate::dialogue::Captions,
    rng: u64,
}

impl Sfx {
    pub fn open(game: &str) -> Self {
        let bank = match MilesBank::open(&std::path::Path::new(game).join("r2/sound")) {
            Ok(b) => {
                log::info!("sound bank: {} sources, {} events", b.sources.len(), b.event_count());
                Some(Arc::new(b))
            }
            Err(e) => {
                log::warn!("no sound: {e:#}");
                None
            }
        };
        Self {
            bank,
            variants: HashMap::new(),
            decoded: HashMap::new(),
            infos: HashMap::new(),
            weapons: HashMap::new(),
            captions: Default::default(),
            rng: 0x5EED_50FD,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub(crate) fn pick(&mut self, v: &[usize]) -> Option<usize> {
        (!v.is_empty()).then(|| v[((self.rand() * v.len() as f32) as usize).min(v.len() - 1)])
    }

    fn variants_of(&mut self, prefix: &str) -> Vec<usize> {
        if let Some(v) = self.variants.get(prefix) {
            return v.clone();
        }
        let v: Vec<usize> = self
            .bank
            .as_ref()
            .map(|b| b.sources.iter().enumerate().filter(|(_, s)| s.name.starts_with(prefix)).map(|(i, _)| i).collect())
            .unwrap_or_default();
        if v.is_empty() && self.bank.is_some() {
            log::warn!("no sound source matches {prefix}");
        }
        self.variants.insert(prefix.to_string(), v.clone());
        v
    }

    /// An event's playback data; names that aren't events but match sources by prefix (older
    /// source-named cues, music tracks) get a plain one-shot.
    pub fn info(&mut self, name: &str) -> Option<EventInfo> {
        let key = name.to_ascii_lowercase();
        if let Some(i) = self.infos.get(&key) {
            return i.clone();
        }
        let info = self.bank.as_ref().and_then(|b| {
            b.event_info(name).cloned().or_else(|| {
                let sources = b.event_variants(name);
                (!sources.is_empty()).then(|| EventInfo { sources, volume: 1.0, spatial: true, ..Default::default() })
            })
        });
        if info.is_none() && self.bank.is_some() {
            log::warn!("no sound for event {name}");
        }
        self.infos.insert(key, info.clone());
        info
    }

    fn has_event(&mut self, name: &str) -> bool {
        !name.is_empty() && self.bank.as_ref().is_some_and(|b| b.event(name).is_some())
    }

    fn event_sources(&mut self, name: &str) -> Vec<usize> {
        self.info(name).map(|i| i.sources).unwrap_or_default()
    }

    /// Decode the events the code plays by name (abilities, ordnance, movement), so their
    /// first play doesn't stall a frame (the stim loop took 50 ms).
    pub fn preload_code_events(&mut self) {
        let mut names: Vec<&str> = CODE_EVENTS.to_vec();
        for t in crate::pilotability::TACTICALS {
            names.push(t.4);
        }
        for o in crate::pilotability::ORDNANCE {
            names.extend([o.4, o.5]);
        }
        self.preload_events(names);
    }

    /// Decode every source of the named events now (enemy abilities).
    pub fn preload_events<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        let mut want = Vec::new();
        for n in names {
            want.extend(self.event_sources(n));
        }
        self.decode_parallel(want);
    }

    /// Read every weapon's sound set from its script, the closed captions, and decode what the
    /// weapons and dialogue lines need. Call once at load.
    pub fn load_game_data(&mut self, gd: &crate::gamedata::GameData) {
        let t = std::time::Instant::now();
        self.captions = crate::dialogue::Captions::load(gd);
        let ids: Vec<&str> = crate::pilotweapon::ARSENAL.iter().map(|a| a.id).chain(crate::weapons::TITAN_ARSENAL.iter().map(|g| g.id)).collect();
        let mut want = Vec::new();
        for id in ids {
            let ws = self.read_weapon(gd, id);
            for n in ws.start_1p.iter().chain(&ws.end_1p).chain(&ws.shot_1p).chain(&ws.shot_npc).chain(ws.middle_1p.iter()) {
                want.extend(self.event_sources(n));
            }
            self.weapons.insert(id.to_string(), ws);
        }
        for c in ALL_CUES {
            for n in c.events() {
                want.extend(self.event_sources(n));
            }
        }
        for n in crate::dialogue::PRELOAD {
            want.extend(self.event_sources(n));
        }
        self.decode_parallel(want);
        log::info!("weapon sounds from {} scripts, {} captions, in {:?}", self.weapons.len(), self.captions.len(), t.elapsed());
    }

    fn read_weapon(&mut self, gd: &crate::gamedata::GameData, id: &str) -> WeaponSounds {
        let mut read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
        let Some(s) = tf_assets::settings::PlayerSettings::load(&format!("scripts/weapons/{id}.txt"), true, &mut read) else {
            return WeaponSounds::default();
        };
        let key = |k: &str| s.get(&format!("sp_base.{k}")).or_else(|| s.get(&format!(".{k}"))).map(str::to_string).unwrap_or_default();
        let mut ev = |k: &str| -> Option<String> {
            let n = key(k);
            self.has_event(&n).then_some(n)
        };
        let start_1p: Vec<String> = ev("burst_or_looping_fire_sound_start_1p").into_iter().collect();
        let middle_1p = ev("burst_or_looping_fire_sound_middle_1p");
        let end_1p: Vec<String> = ev("burst_or_looping_fire_sound_end_1p").into_iter().collect();
        let shot_1p: Vec<String> = [ev("fire_sound_1_player_1p"), ev("fire_sound_2_player_1p")].into_iter().flatten().collect();
        let mut shot_npc: Vec<String> = [ev("fire_sound_1_npc"), ev("fire_sound_2_npc")].into_iter().flatten().collect();
        if shot_npc.is_empty() {
            shot_npc = [ev("fire_sound_1_player_3p"), ev("fire_sound_2_player_3p")].into_iter().flatten().collect();
        }
        let looping = key("looping_sounds") == "1" && middle_1p.is_some();
        let ws = WeaponSounds { looping, start_1p, middle_1p, end_1p, shot_1p, shot_npc };
        log::debug!("weapon sounds {id}: {ws:?}");
        ws
    }

    fn weapon(&self, id: &str) -> Option<&WeaponSounds> {
        self.weapons.get(id).filter(|w| w.looping || !w.shot_1p.is_empty() || !w.start_1p.is_empty())
    }

    fn pcm(&mut self, index: usize) -> Option<(Arc<Vec<f32>>, u32)> {
        if let Some(p) = self.decoded.get(&index) {
            return p.clone();
        }
        let bank = self.bank.as_ref()?;
        let src = &bank.sources[index];
        let t0 = std::time::Instant::now();
        let result = bank
            .read_binka(src)
            .and_then(|f| tf_assets::binka::decode(&f))
            .map(|p| (Arc::new(to_stereo(&p.samples, p.channels)), p.sample_rate))
            .map_err(|e| log::warn!("sound {}: {e:#}", src.name))
            .ok();
        log::debug!("decoded {} in {:?}", src.name, t0.elapsed());
        self.decoded.insert(index, result.clone());
        result
    }

    /// Decode every source a cue can use now, so the first play doesn't stall a frame.
    pub fn preload(&mut self, cues: &[Cue]) {
        let mut want = Vec::new();
        for c in cues {
            for prefix in c.sources().0 {
                want.extend(self.variants_of(prefix));
            }
        }
        self.decode_parallel(want);
    }

    /// Decode sources on every core (startup preloading).
    fn decode_parallel(&mut self, mut want: Vec<usize>) {
        want.sort_unstable();
        want.dedup();
        want.retain(|i| !self.decoded.contains_key(i));
        let Some(bank) = self.bank.clone() else { return };
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: Vec<(usize, Option<(Arc<Vec<f32>>, u32)>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    let (bank, want, next) = (&bank, &want, &next);
                    scope.spawn(move || {
                        let mut out = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&i) = want.get(k) else { break };
                            let src = &bank.sources[i];
                            let r = bank
                                .read_binka(src)
                                .and_then(|f| tf_assets::binka::decode(&f))
                                .map(|p| (Arc::new(to_stereo(&p.samples, p.channels)), p.sample_rate))
                                .map_err(|e| log::warn!("sound {}: {e:#}", src.name))
                                .ok();
                            out.push((i, r));
                        }
                        out
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        self.decoded.extend(results);
    }

    /// Decode a source off the main thread (music, long dialogue).
    pub fn decode_detached(&self, index: usize) -> Option<std::sync::mpsc::Receiver<Option<(Arc<Vec<f32>>, u32)>>> {
        let bank = self.bank.clone()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let src = &bank.sources[index];
            let pcm = bank.read_binka(src).and_then(|f| tf_assets::binka::decode(&f)).ok().map(|p| (Arc::new(to_stereo(&p.samples, p.channels)), p.sample_rate));
            let _ = tx.send(pcm);
        });
        Some(rx)
    }
}

/// Fold any channel layout down to stereo. 5.1 (FL FR FC LFE BL BR) mixes centre and
/// surrounds in at -3 dB.
pub fn to_stereo(s: &[f32], channels: usize) -> Vec<f32> {
    let frames = s.len() / channels.max(1);
    let mut out = Vec::with_capacity(frames * 2);
    for f in s.chunks_exact(channels.max(1)) {
        let (l, r) = match channels {
            1 => (f[0], f[0]),
            2 => (f[0], f[1]),
            6 => {
                let c = 0.707 * f[2] + 0.5 * f[3];
                (f[0] + c + 0.707 * f[4], f[1] + c + 0.707 * f[5])
            }
            n => {
                let (mut l, mut r) = (0.0, 0.0);
                for (i, v) in f.iter().enumerate().take(n) {
                    if i % 2 == 0 { l += v } else { r += v }
                }
                (l, r)
            }
        };
        out.push(l.clamp(-1.0, 1.0));
        out.push(r.clamp(-1.0, 1.0));
    }
    out
}

/// Master volume at startup: deliberately quiet; raise it with `=` / lower with `-`, or
/// start with `--volume 0.0..1.0`.
pub const DEFAULT_VOLUME: f32 = 0.15;

pub struct GameAudioPlugin;

/// `-` and `=` step the master volume down/up by 0.05 (0 to 1).
fn volume_keys(keys: Res<ButtonInput<KeyCode>>, mut global: ResMut<MasterVolume>) {
    let step = if keys.just_pressed(KeyCode::Equal) {
        0.05
    } else if keys.just_pressed(KeyCode::Minus) {
        -0.05
    } else {
        return;
    };
    let v = (global.volume.to_linear() + step).clamp(0.0, 1.0);
    global.volume = bevy::audio::Volume::Linear(v);
    log::info!("volume {:.0}%", v * 100.0);
}

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Sound>()
            .init_resource::<Mixer>()
            .init_resource::<crate::ui::Subtitles>()
            .init_resource::<crate::music::Music>()
            .add_systems(Update, (crate::music::update_music, crate::dialogue::announcer).run_if(crate::ui::loaded))
            .insert_resource(MasterVolume::new(bevy::audio::Volume::Linear(DEFAULT_VOLUME)))
            .add_systems(Update, (volume_keys, apply_master_volume).chain())
            .add_systems(PostUpdate, (crate::actor::actor_event_sounds, play_cues).chain());
    }
}

/// Voices playing, loops held, and ducking.
#[derive(Resource, Default)]
pub struct Mixer {
    /// (ends at, event name) of every one-shot playing.
    voices: Vec<(f32, String)>,
    /// Looping sounds by key: (entity, last refreshed, timeout, end event, position).
    loops: HashMap<String, (Entity, f32, Option<f32>, Option<String>, Option<Vec3>)>,
    /// Explosion ducking (0..1), decays.
    pub duck: f32,
    /// Dialogue is playing until this time.
    pub dialogue_until: f32,
}

impl Mixer {
    /// How much to scale music by right now.
    pub fn music_scale(&self, now: f32) -> f32 {
        let dialogue = if now < self.dialogue_until { 0.45 } else { 1.0 };
        (1.0 - 0.5 * self.duck) * dialogue
    }
}

/// Game space from Bevy space (inverse of `to_bevy`).
fn to_game(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y) / crate::player::UNIT
}

/// Everything needed to place a sound for the listener.
struct Listener {
    ear: Vec3,
    right: Vec3,
    ear_game: Vec3,
}

/// Per-ear gains and low-pass for a sound at `at` (game space), or None if it's out of range.
fn place(info: &EventInfo, at: Option<Vec3>, fallback: (f32, f32, f32), listener: &Option<Listener>, world: Option<&crate::player::Collision>) -> Option<([f32; 2], f32)> {
    let base = fallback.2 * info.volume * EVENT_TRIM;
    let (Some(at), Some(l), true) = (at, listener, info.spatial || fallback.0 < 1e8) else {
        return Some(([base.min(1.0), base.min(1.0)], 1.0));
    };
    let (near, far) = match (info.min_distance, info.max_distance) {
        (Some(n), Some(f)) if f > n => (n, f),
        (None, Some(f)) => (0.0, f),
        _ if fallback.0 < 1e8 => (fallback.0, fallback.1),
        _ => (400.0, 7000.0),
    };
    let to = to_bevy(at) - l.ear;
    let d = to.length() / crate::player::UNIT;
    let k = 1.0 - ((d - near) / (far - near).max(1.0)).clamp(0.0, 1.0);
    if k <= 0.0 {
        return None;
    }
    // Equal-power pan, centred at unity.
    let pan = to.normalize_or_zero().dot(l.right).clamp(-1.0, 1.0) * 0.8;
    let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let mut g = base * k * k;
    let mut lowpass = 1.0;
    // Occlusion: world geometry between the sound and the listener muffles it.
    if let Some(w) = world {
        let dir = l.ear_game - at;
        let len = dir.length();
        if len > 60.0 {
            let n = dir / len;
            let from = at + n * 30.0;
            if w.0.raycast(tf_sim::glam::Vec3::new(from.x, from.y, from.z), tf_sim::glam::Vec3::new(n.x, n.y, n.z), len - 60.0).is_some() {
                g *= OCCLUDED_GAIN;
                lowpass = OCCLUDED_CUTOFF;
            }
        }
    }
    let gains = [g * a.cos() * std::f32::consts::SQRT_2, g * a.sin() * std::f32::consts::SQRT_2].map(|v| v.min(1.0));
    Some((gains, lowpass))
}

/// Low-pass coefficient for a cutoff at a sample rate (1 = off).
fn lowpass_coef(cutoff: f32, rate: u32) -> f32 {
    if cutoff >= 1.0 && cutoff < rate as f32 * 0.5 {
        1.0 - (-2.0 * std::f32::consts::PI * cutoff / rate as f32).exp()
    } else {
        1.0
    }
}

/// What to play for one request.
struct Play<'a> {
    name: &'a str,
    at: Option<Vec3>,
    fallback: (f32, f32, f32),
    duration: Option<f32>,
    /// Loop it (returned entity kept by the caller).
    looped: bool,
    /// Return the entity of a one-shot too (a keyed sound that can be cut short).
    keep: bool,
}

#[allow(clippy::too_many_arguments)]
fn play_event(
    commands: &mut Commands,
    sfx: &mut Sfx,
    sounds: &mut Assets<Sound>,
    mixer: &mut Mixer,
    subtitle: &mut crate::ui::Subtitles,
    listener: &Option<Listener>,
    world: Option<&crate::player::Collision>,
    now: f32,
    p: Play,
) -> Option<Entity> {
    let info = sfx.info(p.name)?;
    let dialogue = p.name.to_ascii_lowercase().starts_with("diag_");
    // Voice limits: world sounds give way when the mix is full or the event is already busy.
    mixer.voices.retain(|v| v.0 > now);
    let busy = mixer.voices.iter().filter(|v| v.1 == p.name).count();
    if !dialogue && p.at.is_some() && (mixer.voices.len() >= MAX_VOICES || busy >= MAX_INSTANCES) {
        return None;
    }
    let (mut gains, cutoff) = place(&info, p.at, p.fallback, listener, world)?;
    let i = sfx.pick(&info.sources)?;
    let (pcm, rate) = sfx.pcm(i)?;
    let secs = pcm.len() as f32 / 2.0 / rate as f32;
    if dialogue {
        let source = sfx.bank.as_ref().map(|b| b.sources[i].name.clone()).unwrap_or_default();
        if let Some(text) = sfx.captions.find(&source).or_else(|| sfx.captions.find(p.name)) {
            log::info!("subtitle: {text}");
            subtitle.push("", &text, secs + 0.5);
        }
        mixer.dialogue_until = mixer.dialogue_until.max(now + secs);
    } else if now < mixer.dialogue_until {
        gains = gains.map(|g| g * 0.75);
    }
    let lower = p.name.to_ascii_lowercase();
    if lower.contains("explo") || lower.contains("titan_land") || lower.contains("hotdrop") {
        mixer.duck = mixer.duck.max(0.6);
    }
    let mut sound = Sound::new(pcm, rate, gains);
    sound.lowpass = lowpass_coef(cutoff, rate);
    let handle = sounds.add(sound);
    let looped = p.looped || (info.looped && p.duration.is_none());
    let mut settings = if looped { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN };
    if let Some(d) = p.duration {
        settings = settings.with_duration(Duration::from_secs_f32(d.max(0.01)));
    }
    if !looped {
        mixer.voices.push((now + p.duration.unwrap_or(secs).min(secs), p.name.to_string()));
    }
    let e = commands.spawn((AudioPlayer::<Sound>(handle), settings)).id();
    (looped || p.keep).then_some(e)
}

#[allow(clippy::too_many_arguments)]
fn play_cues(
    mut commands: Commands,
    time: Res<Time>,
    sfx: Option<ResMut<Sfx>>,
    mut sounds: ResMut<Assets<Sound>>,
    mut mixer: ResMut<Mixer>,
    mut subtitle: ResMut<crate::ui::Subtitles>,
    world: Option<Res<crate::player::Collision>>,
    mut cues: Query<(Entity, &mut SoundCue)>,
    events: Query<(Entity, &SoundEvent)>,
    loops: Query<(Entity, &SoundLoop)>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
) {
    let Some(mut sfx) = sfx else {
        for (e, _) in &cues {
            commands.entity(e).despawn();
        }
        for (e, _) in &events {
            commands.entity(e).despawn();
        }
        for (e, _) in &loops {
            commands.entity(e).despawn();
        }
        return;
    };
    let now = time.elapsed_secs();
    mixer.duck = (mixer.duck - time.delta_secs() * 0.8).max(0.0);
    let listener = camera.single().ok().map(|g| Listener { ear: g.translation(), right: g.right().as_vec3(), ear_game: to_game(g.translation()) });
    let world = world.as_deref();
    let sfx = &mut *sfx;
    let (mixer, subtitle) = (&mut *mixer, &mut *subtitle);
    let event_fallback = (1e9, 1e9, EVENT_GAIN);
    let world_fallback = (400.0, 7000.0, 0.55);

    // Animation and script sound events.
    for (e, ev) in &events {
        commands.entity(e).despawn();
        log::debug!("sound event {}", ev.name);
        let fallback = if ev.at.is_some() { world_fallback } else { event_fallback };
        let play = Play { name: &ev.name, at: ev.at, fallback, duration: None, looped: false, keep: false };
        if let Some(looper) = play_event(&mut commands, sfx, &mut sounds, mixer, subtitle, &listener, world, now, play) {
            // A looping event fired as a one-shot (an emitter): let it run a while, then stop.
            mixer.loops.insert(format!("event:{}:{}", ev.name, looper.index()), (looper, now, Some(6.0), None, ev.at));
        }
    }

    // Loop requests.
    for (e, l) in &loops {
        commands.entity(e).despawn();
        match &l.event {
            Some(name) => {
                if l.once {
                    log::debug!("keyed sound {} on {}", name, l.key);
                    if let Some((old, ..)) = mixer.loops.remove(&l.key) {
                        commands.entity(old).try_despawn();
                    }
                } else if let Some(entry) = mixer.loops.get_mut(&l.key) {
                    entry.1 = now;
                    continue;
                }
                let play = Play { name, at: l.at, fallback: if l.at.is_some() { world_fallback } else { event_fallback }, duration: None, looped: !l.once, keep: l.once };
                if let Some(ent) = play_event(&mut commands, sfx, &mut sounds, mixer, subtitle, &listener, world, now, play) {
                    mixer.loops.insert(l.key.clone(), (ent, now, l.timeout, l.end.clone(), l.at));
                }
            }
            None => stop_loop(&mut commands, sfx, &mut sounds, mixer, subtitle, &listener, world, now, &l.key, l.end.as_deref()),
        }
    }
    // Loops nobody refreshed in time stop by themselves.
    let stale: Vec<String> = mixer.loops.iter().filter(|(_, v)| v.2.is_some_and(|t| now - v.1 > t)).map(|(k, _)| k.clone()).collect();
    for k in stale {
        stop_loop(&mut commands, sfx, &mut sounds, mixer, subtitle, &listener, world, now, &k, None);
    }

    // Gameplay cues.
    for (e, mut c) in &mut cues {
        if c.delay > 0.0 {
            c.delay -= time.delta_secs();
            continue;
        }
        commands.entity(e).despawn();
        log::debug!("cue {:?}", c.cue);
        let fallback = c.cue.falloff();
        let mut ctx = Ctx { commands: &mut commands, sfx: &mut *sfx, sounds: &mut sounds, mixer: &mut *mixer, subtitle: &mut *subtitle, listener: &listener, world, now };
        if ctx.weapon_cue(c.cue, c.at, fallback) {
            continue;
        }
        let names = c.cue.events();
        if !names.is_empty() && names.iter().all(|n| ctx.sfx.has_event(n)) {
            for n in names {
                let play = Play { name: n, at: c.at, fallback, duration: c.duration, looped: false, keep: false };
                ctx.play_oneshot(play);
            }
            ctx.cue_side_effects(c.cue, c.at);
            continue;
        }
        ctx.legacy(c.cue, c.at, fallback, c.duration);
        ctx.cue_side_effects(c.cue, c.at);
    }
}

#[allow(clippy::too_many_arguments)]
fn stop_loop(
    commands: &mut Commands,
    sfx: &mut Sfx,
    sounds: &mut Assets<Sound>,
    mixer: &mut Mixer,
    subtitle: &mut crate::ui::Subtitles,
    listener: &Option<Listener>,
    world: Option<&crate::player::Collision>,
    now: f32,
    key: &str,
    end: Option<&str>,
) {
    let Some((ent, _, _, own_end, at)) = mixer.loops.remove(key) else { return };
    log::debug!("stop loop {key}");
    commands.entity(ent).try_despawn();
    for name in end.into_iter().chain(own_end.as_deref()) {
        let fallback = if at.is_some() { (400.0, 7000.0, 0.55) } else { (1e9, 1e9, EVENT_GAIN) };
        play_event(commands, sfx, sounds, mixer, subtitle, listener, world, now, Play { name, at, fallback, duration: None, looped: false, keep: false });
    }
}

/// Borrowed state for playing cues.
struct Ctx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    sfx: &'a mut Sfx,
    sounds: &'a mut Assets<Sound>,
    mixer: &'a mut Mixer,
    subtitle: &'a mut crate::ui::Subtitles,
    listener: &'a Option<Listener>,
    world: Option<&'a crate::player::Collision>,
    now: f32,
}

impl Ctx<'_, '_, '_> {
    fn play(&mut self, p: Play) -> Option<Entity> {
        play_event(self.commands, self.sfx, self.sounds, self.mixer, self.subtitle, self.listener, self.world, self.now, p)
    }

    /// Play once; an event whose record loops is let run a few seconds, then stopped.
    fn play_oneshot(&mut self, p: Play) {
        let (name, at) = (p.name.to_string(), p.at);
        if let Some(ent) = self.play(p) {
            self.mixer.loops.insert(format!("event:{name}:{}", ent.index()), (ent, self.now, Some(6.0), None, at));
        }
    }

    /// Weapon fire from the weapon's script events. False if the weapon has none (the
    /// hand-mapped sources play instead).
    fn weapon_cue(&mut self, cue: Cue, at: Option<Vec3>, fallback: (f32, f32, f32)) -> bool {
        let (id, part, npc) = match cue {
            Cue::Gun(i, part) => (crate::pilotweapon::ARSENAL[i as usize].id, part, false),
            Cue::TitanGun(i, part) => (crate::weapons::TITAN_ARSENAL[i as usize].id, part, false),
            Cue::EnemyGun(i) => (crate::weapons::TITAN_ARSENAL[i as usize].id, GunPart::Shot, true),
            _ => return false,
        };
        let Some(ws) = self.sfx.weapon(id).cloned() else { return false };
        let gain = fallback.2;
        if npc {
            if ws.shot_npc.is_empty() {
                return false;
            }
            for n in &ws.shot_npc {
                self.play(Play { name: n, at, fallback: (400.0, 7000.0, 0.55), duration: None, looped: false, keep: false });
            }
            return true;
        }
        let key = format!("gun:{id}");
        if ws.looping {
            let middle = ws.middle_1p.clone().unwrap_or_default();
            match part {
                GunPart::First | GunPart::Shot => {
                    if !self.mixer.loops.contains_key(&key) {
                        if part == GunPart::First {
                            for n in &ws.start_1p {
                                self.play(Play { name: n, at, fallback: (fallback.0, fallback.1, gain), duration: None, looped: false, keep: false });
                            }
                        }
                        let p = Play { name: &middle, at, fallback: (fallback.0, fallback.1, gain), duration: None, looped: true, keep: false };
                        if let Some(ent) = self.play(p) {
                            // The tail cue ends the loop; a weapon switch or stall without one
                            // still ends it after a while.
                            self.mixer.loops.insert(key, (ent, self.now, Some(2.0), ws.end_1p.first().cloned(), at));
                        }
                    } else if let Some(entry) = self.mixer.loops.get_mut(&key) {
                        entry.1 = self.now;
                    }
                }
                GunPart::Tail => stop_loop(self.commands, self.sfx, self.sounds, self.mixer, self.subtitle, self.listener, self.world, self.now, &key, None),
            }
            return true;
        }
        let names = match part {
            GunPart::First if !ws.start_1p.is_empty() => ws.start_1p.clone(),
            GunPart::First | GunPart::Shot => ws.shot_1p.clone(),
            GunPart::Tail => ws.end_1p.clone(),
        };
        for n in &names {
            self.play_oneshot(Play { name: n, at, fallback: (fallback.0, fallback.1, gain), duration: None, looped: false, keep: false });
        }
        true
    }

    /// Hand-mapped sources (cues the scripts don't name, or events the bank lacks).
    fn legacy(&mut self, cue: Cue, at: Option<Vec3>, fallback: (f32, f32, f32), duration: Option<f32>) {
        let (prefixes, all) = cue.sources();
        let groups: Vec<Vec<usize>> = if all {
            prefixes.iter().map(|p| self.sfx.variants_of(p)).collect()
        } else {
            vec![prefixes.iter().flat_map(|p| self.sfx.variants_of(p)).collect()]
        };
        let info = EventInfo { volume: 1.0, spatial: fallback.0 < 1e8, ..Default::default() };
        let Some((gains, cutoff)) = place(&info, at, fallback, self.listener, self.world) else { return };
        for g in groups {
            let Some(i) = self.sfx.pick(&g) else { continue };
            let Some((pcm, rate)) = self.sfx.pcm(i) else { continue };
            let mut sound = Sound::new(pcm, rate, gains);
            sound.lowpass = lowpass_coef(cutoff, rate);
            let handle = self.sounds.add(sound);
            let mut settings = PlaybackSettings::DESPAWN;
            if let Some(d) = duration {
                settings = settings.with_duration(Duration::from_secs_f32(d.max(0.01)));
            }
            self.commands.spawn((AudioPlayer::<Sound>(handle), settings));
        }
        if matches!(cue, Cue::MissileExplode | Cue::TitanDeath | Cue::TitanLand) {
            self.mixer.duck = self.mixer.duck.max(0.6);
        }
    }

    /// Loops some cues start or stop (the Vortex hum while it's up).
    fn cue_side_effects(&mut self, cue: Cue, at: Option<Vec3>) {
        match cue {
            Cue::VortexStart => {
                let p = Play { name: "vortex_shield_loop_1P", at, fallback: (1e9, 1e9, 0.6), duration: None, looped: true, keep: false };
                if !self.mixer.loops.contains_key("vortex") {
                    if let Some(ent) = self.play(p) {
                        self.mixer.loops.insert("vortex".into(), (ent, self.now, Some(12.0), None, at));
                    }
                }
            }
            Cue::VortexEnd | Cue::VortexThrow => stop_loop(self.commands, self.sfx, self.sounds, self.mixer, self.subtitle, self.listener, self.world, self.now, "vortex", None),
            _ => {}
        }
    }
}

/// Sound events the code plays by literal name (preloaded with `Sfx::preload_code_events`).
const CODE_EVENTS: &[&str] = &[
    "pilot_stimpack_loop_1P", "pilot_stimpack_deactivate_1P", "cloak_sustain_loop_1P", "cloak_interruptend_1P", "Pilot_PhaseShift_Loop_1P", "Pilot_PhaseShift_End_1P",
    "Jumpjet_Jump_Body_1P", "Jumpjet_Jet_Body_1P", "Jumpjet_Wallrun_Body_1P", "Player_Zipline_Attach", "Player_Zipline_Detach", "Player_Zipline_Loop", "pilot_grapple_ready", "pilot_grapple_retract_1p", "default_grapple_impact_1p_vs_3p",
    "Pilot_PulseBlade_Sonar_Pulse_1P", "Pilot_Mvmt_Melee_Hit_1P", "Pilot_Rodeo_Titan_Attach", "weapon_gravitystar_preexplo", "weapon_r1_satchel_armedbeep", "weapon_r1_satchel_attach",
    "holopilot_end_3P", "Hardcover_Shield_Start_3P", "Hardcover_Shield_End_3P", "player_eject_windrush", "Titan_Eject_PilotLaunch_3P", "Titan_Eject_Servos_3P", "titan_eject_xbutton",
    "vortex_1p_shield_throw", "vortex_3p_shield_end_1", "vortex_3p_shield_start_1_long", "flamewall_flame_start", "incendiary_trap_burn", "incendiary_trap_explode", "incendiary_trap_gas",
    "incendiary_trap_land", "Titan_Offhand_ElectricSmoke_Deploy_3P", "titan_core_flight_liftoff_1p", "Titan_Core_Laser_FireBeam_1P", "Titan_Tone_SonarLock_Impact_1P",
    "Titan_Tone_SonarLock_Impact_Pulse_1P", "UI_TitanBattery_Pilot_PickUp", "UI_TitanBattery_Titan_PickUp", "Wpn_TetherTrap_Land", "Wpn_TetherTrap_PopOpen_3p",
    "weapon_predator_rangeswitch_tolong_1p", "weapon_predator_rangeswitch_toshort_1p", "Atlas_3p_Sync_Melee",
];
