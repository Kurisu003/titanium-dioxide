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
}

impl Default for Game {
    fn default() -> Self {
        Self { state: GameState::Title, wave: 0, kills: 0, timer: 0.0, best_wave: 0 }
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
pub fn gate_input(game: Res<Game>, mut input: ResMut<PlayerInput>) {
    if game.in_play() {
        return;
    }
    let (start, pause, menu_nav) = (input.start, input.pause, input.menu_nav);
    let (yaw, pitch) = (input.yaw, input.pitch);
    *input = PlayerInput { start, pause, yaw, pitch, menu_nav, ..default() };
}

#[allow(clippy::too_many_arguments)]
pub fn director(
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut input: ResMut<PlayerInput>,
    mut control: ResMut<Control>,
    arena: Res<Arena>,
    kits: Res<EnemyKits>,
    world: Res<crate::player::Collision>,
    mut titans: Query<(&mut PlayerTitan, &mut TitanHealth, &mut Titanfall, &mut Weapon, &mut Ordnance, &mut TitanCore, &mut Vortex, &mut Melee)>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    mut enemies: Query<&mut Enemy>,
    (mut pilot_status, ability_defs): (ResMut<crate::pilotability::PilotStatus>, Option<Res<crate::pilotability::PilotAbilityDefs>>),
) {
    let dt = time.delta_secs();
    let start = std::mem::take(&mut input.start);
    match game.state {
        GameState::Title | GameState::GameOver => {
            if start {
                // Fresh run: BT at the spawn, full health and kit, no enemies.
                for (mut t, mut h, mut tf, mut w, mut o, mut c, mut v, mut m) in &mut titans {
                    t.state = tf_sim::titan::TitanState::new(SVec3::from(arena.player_spawn.to_array()), arena.player_yaw);
                    t.override_anim = None;
                    h.reset();
                    *tf = Titanfall::default();
                    w.ammo = w.def.clip;
                    *o = Ordnance::default();
                    *c = TitanCore::default();
                    *v = Vortex::default();
                    *m = Melee::default();
                }
                for (_, mut ph) in &mut pilots {
                    *ph = PilotHealth::default();
                }
                if let Some(d) = ability_defs.as_deref() {
                    pilot_status.refill(d);
                }
                for mut e in &mut enemies {
                    e.active = false;
                    e.v.health = 0.0;
                }
                input.yaw = arena.player_yaw;
                input.pitch = 0.0;
                *control = Control::Titan;
                // TF_WAVE=N starts at wave N (debugging bigger fights).
                game.wave = std::env::var("TF_WAVE").ok().and_then(|w| w.parse::<u32>().ok()).unwrap_or(1).saturating_sub(1);
                game.kills = 0;
                game.timer = 2.0;
                game.state = GameState::Intermission;
                log::info!("new game");
            }
        }
        GameState::Playing | GameState::Intermission => {
            for mut e in &mut enemies {
                if e.just_died {
                    e.just_died = false;
                    game.kills += 1;
                }
            }
            if pilots.iter().any(|(_, h)| h.dead()) {
                game.best_wave = game.best_wave.max(game.wave);
                game.state = GameState::GameOver;
                log::info!("game over at wave {} with {} kills", game.wave, game.kills);
                return;
            }
            if game.state == GameState::Playing {
                if !enemies.iter().any(|e| e.active) {
                    game.state = GameState::Intermission;
                    game.timer = INTERMISSION_SECS;
                    log::info!("wave {} cleared", game.wave);
                }
            } else {
                game.timer -= dt;
                // TF_NO_ENEMIES=1: an empty map for testing movement, weapons and animations.
                if std::env::var_os("TF_NO_ENEMIES").is_some() {
                    game.timer = game.timer.max(1.0);
                }
                if game.timer <= 0.0 {
                    game.wave += 1;
                    spawn_wave(&mut game, &arena, &kits, &world, &titans, &pilots, *control, &mut enemies);
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
    titans: &Query<(&mut PlayerTitan, &mut TitanHealth, &mut Titanfall, &mut Weapon, &mut Ordnance, &mut TitanCore, &mut Vortex, &mut Melee)>,
    pilots: &Query<(&PlayerPilot, &mut PilotHealth)>,
    control: Control,
    enemies: &mut Query<&mut Enemy>,
) {
    let player = match control {
        Control::Pilot => pilots.iter().next().map(|(p, _)| Vec3::from(p.state.pos.to_array())),
        _ => titans.iter().next().map(|t| Vec3::from(t.0.state.pos.to_array())),
    }
    .unwrap_or(arena.player_spawn);
    // One more Titan every other wave: 1, 2, 2, 3, 3, 4...
    let count = (1 + game.wave as usize / 2).clamp(1, MAX_ENEMIES_PER_WAVE);
    // Prefer spots that can see the player, at a fair distance.
    let sees = |p: &Vec3| {
        let from = *p + Vec3::Z * 200.0;
        let to = player + Vec3::Z * 120.0;
        let d = to - from;
        world.0.raycast(SVec3::from(from.to_array()), SVec3::from(d.normalize().to_array()), d.length() - 50.0).is_none()
    };
    let mut spots: Vec<Vec3> = arena.enemy_spots.clone();
    spots.sort_by_key(|p| (!sees(p) as i64) * 100_000 + ((p.distance(player) - 2400.0).abs()) as i64);
    // Mix the classes: walk the pool starting at a class that rotates with the wave, so each
    // wave brings a different line-up and no class repeats until all have appeared.
    let (titans, grunts): (Vec<_>, Vec<_>) = enemies.iter_mut().partition(|e| !e.infantry);
    let classes = kits.0.iter().filter(|k| !k.infantry).count().max(1);
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
    let squad = (2 + game.wave as usize * 2).min(grunts.len());
    for (i, mut e) in grunts.into_iter().take(if spots.is_empty() { 0 } else { squad }).enumerate() {
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
