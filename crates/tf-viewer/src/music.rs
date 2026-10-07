//! Music: Frontier Defense's own music events, picked from the wave game's state as the FD
//! mode plays them (`music_mp_fd_*`): the intro for the difficulty, a stinger when a wave
//! starts (`introwave`) under the mid-wave bed (`midwave`, `finalwave` every fifth wave), the
//! wave-cleared stinger into the between-waves bed, and the defeat. The bed's level follows the
//! fighting (recent hits on the player and enemy Titans up), and the mixer ducks it under
//! explosions and dialogue. Tracks decode on a worker thread so switching never stalls a frame.

use crate::audio::{Sfx, Sound};
use bevy::audio::AudioSinkPlayback;
use bevy::prelude::*;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

const MUSIC_GAIN: f32 = 0.5;

/// What should be playing: a one-shot stinger, then a looping bed.
#[derive(Clone, PartialEq, Debug)]
struct Cue {
    stinger: Option<&'static str>,
    bed: Option<String>,
}

type Pcm = Option<(Arc<Vec<f32>>, u32)>;

#[derive(Resource, Default)]
pub struct Music {
    want: Option<Cue>,
    /// Decoding: (is the bed, name, receiver).
    pending: Vec<(bool, String, Mutex<Receiver<Pcm>>)>,
    stinger: Option<Entity>,
    bed: Option<Entity>,
    /// Combat intensity 0..1 (smoothed).
    intensity: f32,
    seen_hits: usize,
}

fn cue_for(game: &crate::game::Game, difficulty: crate::vitals::Difficulty) -> Cue {
    use crate::game::GameState::*;
    match game.state {
        Title => Cue { stinger: None, bed: Some("music_lobby_menumusic02".into()) },
        Intermission if game.wave == 0 => Cue {
            stinger: Some(match difficulty {
                crate::vitals::Difficulty::Easy => "music_mp_fd_intro_easy",
                crate::vitals::Difficulty::Normal => "music_mp_fd_intro_medium",
                _ => "music_mp_fd_intro_hard",
            }),
            bed: Some("music_mp_fd_betweenwaves".into()),
        },
        Intermission => Cue { stinger: Some("music_mp_fd_wavecleared"), bed: Some("music_mp_fd_betweenwaves".into()) },
        Playing if game.wave % 5 == 0 => Cue { stinger: Some("music_mp_fd_introwave"), bed: Some("music_mp_fd_finalwave".into()) },
        Playing => Cue { stinger: Some("music_mp_fd_introwave"), bed: Some("music_mp_fd_midwave".into()) },
        GameOver => Cue { stinger: Some("music_mp_fd_defeat"), bed: None },
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_music(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<crate::game::Game>,
    difficulty: Res<crate::vitals::Difficulty>,
    sfx: Option<ResMut<Sfx>>,
    mixer: Res<crate::audio::Mixer>,
    damage: Option<Res<crate::hud::DamageFrom>>,
    enemies: Query<&crate::targets::Enemy>,
    mut music: ResMut<Music>,
    mut sounds: ResMut<Assets<Sound>>,
    mut sinks: Query<&mut AudioSink>,
) {
    let Some(mut sfx) = sfx else { return };
    let want = cue_for(&game, *difficulty);
    if music.want.as_ref() != Some(&want) {
        let old = music.want.replace(want.clone());
        music.pending.clear();
        // A new stinger interrupts the old one; the bed only restarts if it changes.
        if let Some(e) = music.stinger.take() {
            commands.entity(e).despawn();
        }
        let bed_changed = old.and_then(|o| o.bed) != want.bed;
        if bed_changed {
            if let Some(e) = music.bed.take() {
                commands.entity(e).despawn();
            }
        }
        let mut start = |is_bed: bool, name: &str, music: &mut Music| {
            let Some(info) = sfx.info(name) else { return };
            let Some(i) = sfx.pick(&info.sources) else { return };
            if let Some(rx) = sfx.decode_detached(i) {
                music.pending.push((is_bed, name.to_string(), Mutex::new(rx)));
            }
        };
        if let Some(s) = want.stinger {
            start(false, s, &mut music);
        }
        if bed_changed {
            if let Some(b) = &want.bed {
                start(true, b, &mut music);
            }
        }
    }

    // Finished decodes start playing.
    let mut ready = Vec::new();
    music.pending.retain(|(is_bed, name, rx)| match rx.lock().unwrap().try_recv() {
        Ok(pcm) => {
            ready.push((*is_bed, name.clone(), pcm));
            false
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for (is_bed, name, pcm) in ready {
        let Some((pcm, rate)) = pcm else {
            log::warn!("music {name} unavailable");
            continue;
        };
        log::info!("music: {name}");
        let handle = sounds.add(Sound::new(pcm, rate, [MUSIC_GAIN, MUSIC_GAIN]));
        let settings = if is_bed { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN };
        let e = commands.spawn((AudioPlayer::<Sound>(handle), settings)).id();
        if is_bed {
            music.bed = Some(e);
        } else {
            music.stinger = Some(e);
        }
    }

    // Combat intensity: recent hits on the player and living enemy Titans.
    let dt = time.delta_secs();
    let hits = damage.as_ref().map(|d| d.0.len()).unwrap_or(0);
    let new_hits = hits.saturating_sub(music.seen_hits);
    music.seen_hits = hits;
    let titans = enemies.iter().filter(|e| e.alive() && !e.infantry).count() as f32;
    let target = match game.state {
        crate::game::GameState::Playing => (0.35 + 0.15 * titans).min(1.0),
        _ => 0.35,
    };
    music.intensity += new_hits as f32 * 0.15;
    music.intensity += (target - music.intensity) * (1.0 - (-dt * 0.5).exp());
    music.intensity = music.intensity.clamp(0.0, 1.0);
    // Under a stinger the bed sits back; the mixer ducks for explosions and dialogue.
    let duck = mixer.music_scale(time.elapsed_secs());
    let stinger_on = music.stinger.is_some_and(|e| sinks.get(e).is_ok());
    let bed_level = (0.6 + 0.4 * music.intensity) * if stinger_on { 0.5 } else { 1.0 } * duck;
    if let Some(e) = music.bed {
        if let Ok(mut s) = sinks.get_mut(e) {
            s.set_volume(bevy::audio::Volume::Linear(bed_level));
        }
    }
    if let Some(e) = music.stinger {
        if let Ok(mut s) = sinks.get_mut(e) {
            s.set_volume(bevy::audio::Volume::Linear(duck));
        }
    }
}
