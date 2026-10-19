//! The game loop: title screen, waves of enemy Titans, score, game over and restart.

use crate::abilities::{Ordnance, TitanCore};
use crate::combat::{Melee, TitanHealth, Vortex};
use crate::pilotctl::{Control, PilotHealth, PlayerPilot, Titanfall};
use crate::player::{PlayerInput, PlayerTitan};
use crate::targets::{Enemy, EnemyKits};
use crate::weapons::Weapon;
use bevy::prelude::*;
use tf_sim::glam::Vec3 as SVec3;

const INTERMISSION_SECS: f32 = 5.0;
const MAX_ENEMIES_PER_WAVE: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameState {
    Title,
    Playing,
    /// Short break before the next wave.
    Intermission,
    GameOver,
}

#[derive(Resource)]
pub struct Game {
    pub state: GameState,
    pub wave: u32,
    pub kills: u32,
    pub timer: f32,
    pub best_wave: u32,
    /// How many enemies the current wave started with (the wave panel's bar).
    pub wave_size: u32,
    /// Frontier Defense cash from kills (the counter bottom left).
    pub cash: u32,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            state: GameState::Title,
            wave: 0,
            kills: 0,
            timer: 0.0,
            best_wave: 0,
            wave_size: 0,
            cash: 0,
        }
    }
}

impl Game {
    pub fn in_play(&self) -> bool {
        matches!(self.state, GameState::Playing | GameState::Intermission)
    }
}

/// Where BT starts, and where enemy Titans can appear (game units).
#[derive(Resource, Default)]
pub struct Arena {
    pub player_spawn: Vec3,
    pub player_yaw: f32,
    pub enemy_spots: Vec<Vec3>,
}

/// Outside of play, only the start key does anything.
pub fn gate_input(
    game: Res<Game>,
    mut input: ResMut<PlayerInput>,
    pilots: Query<&PilotHealth, With<PlayerPilot>>,
    menu: Res<crate::ui::Menu>,
) {
    // In a match only a dead player (online, waiting to respawn) is held still: no moving,
    // shooting or abilities until the respawn. Paused online (the clock keeps running) the
    // player stands still behind the menu too, and a Titan being executed has no control.
    let executed = crate::executions::BEING_EXECUTED.load(std::sync::atomic::Ordering::Relaxed)
        || crate::executions::EXECUTING.load(std::sync::atomic::Ordering::Relaxed);
    if game.in_play() && !menu.paused && !executed && !pilots.iter().any(|h| h.dead()) {
        return;
    }
    let (start, pause, menu_nav) = (input.start, input.pause, input.menu_nav);
    let (yaw, pitch) = (input.yaw, input.pitch);
    *input = PlayerInput {
        start,
        pause,
        yaw,
        pitch,
        menu_nav,
        ..default()
    };
}

#[allow(clippy::too_many_arguments)]
/// No enemy waves: TF_NO_ENEMIES=1, or the flat test map unless TF_NO_ENEMIES=0 (main.rs).
#[derive(Resource)]
pub struct NoEnemies(pub bool);

pub fn director(
    time: Res<Time>,
    no_enemies: Res<NoEnemies>,
    mut game: ResMut<Game>,
    mut input: ResMut<PlayerInput>,
    mut control: ResMut<Control>,
    arena: Res<Arena>,
    kits: Res<EnemyKits>,
    world: Res<crate::player::Collision>,
    mut titans: Query<(
        &mut PlayerTitan,
        &mut TitanHealth,
        &mut Titanfall,
        &mut Weapon,
        &mut Ordnance,
        &mut TitanCore,
        &mut Vortex,
        &mut Melee,
    )>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    mut enemies: Query<&mut Enemy>,
    (mut pilot_status, ability_defs): (
        ResMut<crate::pilotability::PilotStatus>,
        Option<Res<crate::pilotability::PilotAbilityDefs>>,
    ),
    (mut feed, loadouts): (
        ResMut<crate::feed::Feed>,
        Query<&crate::pilotweapon::PilotLoadout>,
    ),
    mut net: ResMut<crate::net::Net>,
) {
    let dt = time.delta_secs();
    // Multiplayer: a client runs no waves (the host's enemies are mirrored), a PvP host none
    // either; a dead player respawns instead of ending the game.
    let no_waves = no_enemies.0 || net.no_waves();
    let online = net.active();
    let spawn_slot = net.my_id;
    // The gun in hand, for the kill feed rows.
    let weapon_name: String = match *control {
        Control::Pilot => loadouts
            .single()
            .ok()
            .and_then(|l| l.active())
            .map(|w| w.def.name.clone())
            .unwrap_or_default(),
        _ => titans
            .iter()
            .next()
            .map(|t| t.3.def.name.clone())
            .unwrap_or_default(),
    };
    let kits = Some(&*kits);
    let mut start = std::mem::take(&mut input.start);
    // Online, a dead Pilot comes back at the spawn after a few seconds.
    let mut respawn = false;
    if online && game.in_play() {
        if pilots.iter().any(|(_, h)| h.dead()) {
            let t = net.respawn.get_or_insert(crate::net::RESPAWN_SECS);
            *t -= dt;
            if *t <= 0.0 {
                net.respawn = None;
                respawn = true;
                log::info!("respawning");
            }
        } else {
            net.respawn = None;
        }
    }
    if respawn {
        start = true;
    }
    match game.state {
        GameState::Title | GameState::GameOver | GameState::Playing | GameState::Intermission
            if start =>
        {
            let fresh = !game.in_play();
            if fresh || respawn {
                // Fresh run (or a respawn): BT at the spawn, full health and kit; a fresh run
                // also clears the enemies. Online, each player has their own spawn slot.
                let spawn = arena.player_spawn
                    + crate::net::Net::spawn_offset(spawn_slot, arena.player_yaw);
                for (mut t, mut h, mut tf, mut w, mut o, mut c, mut v, mut m) in &mut titans {
                    t.state = tf_sim::titan::TitanState::new(
                        SVec3::from(spawn.to_array()),
                        arena.player_yaw,
                    );
                    t.override_anim = None;
                    h.reset();
                    *tf = Titanfall::default();
                    w.ammo = w.def.clip;
                    *o = Ordnance::default();
                    *c = TitanCore::default();
                    // The kit's shield (Ion's, Scorch's Thermal Shield) stays: apply_kit only
                    // sets it when the loadout changes.
                    let mode = v.mode;
                    *v = Vortex::default();
                    v.mode = mode;
                    *m = Melee::default();
                }
                for (_, mut ph) in &mut pilots {
                    *ph = PilotHealth::default();
                }
                if let Some(d) = ability_defs.as_deref() {
                    pilot_status.refill(d);
                }
                input.yaw = arena.player_yaw;
                input.pitch = 0.0;
                *control = Control::Titan;
                if !fresh {
                    return;
                }
                for mut e in &mut enemies {
                    if e.pool.is_some() {
                        e.active = false;
                        e.v.health = 0.0;
                    }
                }
                // TF_WAVE=N starts at wave N (debugging bigger fights).
                game.wave = std::env::var("TF_WAVE")
                    .ok()
                    .and_then(|w| w.parse::<u32>().ok())
                    .unwrap_or(1)
                    .saturating_sub(1);
                game.kills = 0;
                game.cash = 0;
                feed.kills.clear();
                feed.popups.clear();
                game.timer = 2.0;
                game.state = GameState::Intermission;
                log::info!("new game at {spawn:.0?} (slot {spawn_slot})");
            }
        }
        GameState::Title | GameState::GameOver => {}
        GameState::Playing | GameState::Intermission => {
            if std::mem::take(&mut input.kill_nearest) {
                let from = titans
                    .iter()
                    .next()
                    .map(|t| t.0.state.pos)
                    .map(|p| Vec3::from(p.to_array()))
                    .unwrap_or_default();
                let mut best: Option<(f32, Mut<Enemy>)> = None;
                for e in &mut enemies {
                    if e.alive() {
                        let d = e.pos.distance(from);
                        if best.as_ref().is_none_or(|b| d < b.0) {
                            best = Some((d, e));
                        }
                    }
                }
                if let Some((_, mut e)) = best {
                    e.kill();
                }
            }
            for mut e in &mut enemies {
                if e.just_died {
                    e.just_died = false;
                    game.kills += 1;
                    // Another player's body: net.rs writes their kill row (with their name).
                    if e.remote.is_some() {
                        continue;
                    }
                    // The kill feed row and the Frontier Defense cash popup at the kill (no
                    // cash in PvP: it is Frontier Defense's).
                    let worth = if e.infantry { 45 } else { 100 };
                    if !net.pvp() {
                        game.cash += worth;
                    }
                    let victim = kits
                        .and_then(|k| k.0.get(e.kit))
                        .map(|k| titlecase(&k.name))
                        .unwrap_or_else(|| "Titan".into());
                    feed.kills.push(crate::feed::Kill {
                        victim,
                        weapon: weapon_name.clone(),
                        at: time.elapsed_secs(),
                    });
                    if !net.pvp() {
                        feed.popups.push(crate::feed::Popup {
                            pos: e.pos + Vec3::Z * e.height * 0.6,
                            worth,
                            at: time.elapsed_secs(),
                        });
                    }
                }
            }
            if pilots.iter().any(|(_, h)| h.dead()) && !online {
                game.best_wave = game.best_wave.max(game.wave);
                game.state = GameState::GameOver;
                log::info!("game over at wave {} with {} kills", game.wave, game.kills);
                return;
            }
            if net.is_client() {
                // The host runs the waves; its snapshots carry the wave and kill counts.
                return;
            }
            if game.state == GameState::Playing {
                if !enemies.iter().any(|e| e.active && e.pool.is_some()) {
                    game.state = GameState::Intermission;
                    game.timer = INTERMISSION_SECS;
                    log::info!("wave {} cleared", game.wave);
                }
            } else {
                game.timer -= dt;
                // An empty map for testing movement, weapons and animations (`NoEnemies`), or
                // a PvP match.
                if no_waves {
                    game.timer = game.timer.max(1.0);
                }
                if game.timer <= 0.0 {
                    game.wave += 1;
                    spawn_wave(
                        &mut game,
                        &arena,
                        kits.unwrap(),
                        &world,
                        &titans,
                        &pilots,
                        *control,
                        &mut enemies,
                    );
                    game.state = GameState::Playing;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_wave(
    game: &mut Game,
    arena: &Arena,
    kits: &EnemyKits,
    world: &crate::player::Collision,
    titans: &Query<(
        &mut PlayerTitan,
        &mut TitanHealth,
        &mut Titanfall,
        &mut Weapon,
        &mut Ordnance,
        &mut TitanCore,
        &mut Vortex,
        &mut Melee,
    )>,
    pilots: &Query<(&PlayerPilot, &mut PilotHealth)>,
    control: Control,
    enemies: &mut Query<&mut Enemy>,
) {
    let player = match control {
        Control::Pilot => pilots
            .iter()
            .next()
            .map(|(p, _)| Vec3::from(p.state.pos.to_array())),
        _ => titans
            .iter()
            .next()
            .map(|t| Vec3::from(t.0.state.pos.to_array())),
    }
    .unwrap_or(arena.player_spawn);
    // TF_STATIC_TITAN: one enemy Titan and no grunts, standing still about 1200 out (a test
    // target: it turns and shoots but never walks, and refills its health; targets.rs).
    let static_titan = std::env::var_os("TF_STATIC_TITAN").is_some();
    // One more Titan every other wave: 1, 2, 2, 3, 3, 4...
    let count = if static_titan {
        1
    } else {
        (1 + game.wave as usize / 2).clamp(1, MAX_ENEMIES_PER_WAVE)
    };
    game.wave_size = count as u32;
    let range = if static_titan { 1200.0 } else { 2400.0 };
    // Prefer spots that can see the player, at a fair distance.
    let sees = |p: &Vec3| {
        let from = *p + Vec3::Z * 200.0;
        let to = player + Vec3::Z * 120.0;
        let d = to - from;
        world
            .0
            .raycast(
                SVec3::from(from.to_array()),
                SVec3::from(d.normalize().to_array()),
                d.length() - 50.0,
            )
            .is_none()
    };
    let mut spots: Vec<Vec3> = arena.enemy_spots.clone();
    spots
        .sort_by_key(|p| (!sees(p) as i64) * 100_000 + ((p.distance(player) - range).abs()) as i64);
    // Mix the classes: walk the pool starting at a class that rotates with the wave, so each
    // wave brings a different line-up and no class repeats until all have appeared.
    let (titans, grunts): (Vec<_>, Vec<_>) = enemies
        .iter_mut()
        .filter(|e| e.pool.is_some())
        .partition(|e| !e.infantry);
    let classes = kits
        .0
        .iter()
        .filter(|k| !k.infantry && !k.remote)
        .count()
        .max(1);
    let first = (game.wave as usize).saturating_sub(1) % classes;
    let mut seen = vec![0usize; classes];
    let mut keyed: Vec<_> = titans
        .into_iter()
        .map(|e| {
            let c = e.kit.min(classes - 1);
            seen[c] += 1;
            ((seen[c] - 1) * classes + (c + classes - first) % classes, e)
        })
        .collect();
    keyed.sort_by_key(|(k, _)| *k);
    let mut names = Vec::new();
    for ((_, mut e), spot) in keyed.into_iter().zip(spots.iter().cycle()).take(count) {
        let to = player - *spot;
        let kit = &kits.0[e.kit];
        e.activate(*spot, to.y.atan2(to.x), kit);
        names.push(kit.name.clone());
    }
    // A squad of grunts with every wave, two more each wave, spread around the far spots.
    let squad = if static_titan {
        0
    } else {
        (2 + game.wave as usize * 2).min(grunts.len())
    };
    for (i, mut e) in grunts
        .into_iter()
        .take(if spots.is_empty() { 0 } else { squad })
        .enumerate()
    {
        // Fire teams of three around the best spots, clear of the Titans standing on them.
        let mut spot = spots[(i / 3) % spots.len()];
        // TF_GRUNTS_NEAR: put the squad in front of the player (debugging their animation).
        if std::env::var_os("TF_GRUNTS_NEAR").is_some() {
            spot = player + (spots[0] - player).normalize_or_zero() * 900.0;
        }
        let a = i as f32 * 2.1;
        let p = spot + Vec3::new(a.cos(), a.sin(), 0.0) * 260.0;
        let to = player - p;
        let kit = &kits.0[e.kit];
        e.activate(p, to.y.atan2(to.x), kit);
    }
    names.push(format!("{squad} grunts"));
    log::info!("wave {}: {}", game.wave, names.join(", "));
}

/// "ION" -> "Ion".
fn titlecase(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_ascii_lowercase(),
        None => String::new(),
    }
}
