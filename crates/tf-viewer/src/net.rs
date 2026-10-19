//! Multiplayer: a host-authoritative TCP session.
//!
//! One player hosts (main menu: HOST MULTIPLAYER, a port and PvP or PvE), the others join by
//! address. Everyone simulates their own Pilot and Titan and sends their state 30 times a
//! second; the host relays it to the others, runs the AI enemies and waves (PvE) and sends their
//! state 20 times a second. Remote players are `Enemy` entities (a Pilot body and a Titan) driven
//! by those snapshots instead of AI, so every weapon, the weak points, the grapple and the rodeo
//! work on them unchanged. Damage is reported to the owner of what was hit (health is owned by
//! its player, enemies by the host) as the difference a frame's local hits made.
//!
//! Framing: a u32 little-endian length then the message, first byte the tag. Nothing here is
//! secured; it is for playing with friends over a LAN or a tunnel.

use crate::game::{Game, GameState};
use crate::pilotctl::{Control, PilotHealth, PlayerPilot};
use crate::player::{PlayerInput, PlayerTitan};
use crate::targets::{Enemy, EnemyKits};
use bevy::prelude::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub const DEFAULT_PORT: u16 = 37015;
/// Own-state send rate and the host's enemy snapshot rate.
const PLAYER_RATE: f32 = 30.0;
const ENEMY_RATE: f32 = 20.0;
/// Seconds a dead player waits before respawning at the spawn.
pub const RESPAWN_SECS: f32 = 4.0;
/// Pilot hull for hit tests (player_target in targets.rs).
pub const PILOT_RADIUS: f32 = 30.0;
pub const PILOT_HEIGHT: f32 = 72.0;
/// Spawn slots next to the Titan start, one per player.
const SLOT_SPACING: f32 = 350.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Pve,
    Pvp,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Pve => "PVE",
            Mode::Pvp => "PVP",
        }
    }
}

/// How a remote Pilot body is moving (pilotbody::BodyMove, as sent).
pub const MOVE_IDLE: u8 = 0;
pub const MOVE_RUN: u8 = 1;
pub const MOVE_CROUCH_RUN: u8 = 2;
pub const MOVE_SLIDE: u8 = 3;
pub const MOVE_WALLRUN_LEFT: u8 = 4;
pub const MOVE_WALLRUN_RIGHT: u8 = 5;
pub const MOVE_WALL_HANG: u8 = 6;
pub const MOVE_AIR: u8 = 7;

/// A player's own bodies, as they tell everyone else.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerSnap {
    /// On foot (else in the Titan).
    pub on_foot: bool,
    pub pilot_pos: Vec3,
    pub pilot_vel: Vec3,
    pub pilot_yaw: f32,
    pub pilot_pitch: f32,
    pub body_move: u8,
    pub pilot_health: f32,
    pub titan_alive: bool,
    pub titan_pos: Vec3,
    pub titan_vel: Vec3,
    pub titan_yaw: f32,
    pub titan_pitch: f32,
    pub titan_health: f32,
    pub titan_max_health: f32,
    pub titan_shield: f32,
    pub titan_max_shield: f32,
    pub titan_doomed: bool,
    /// The Titan's scripted sequence (index into TITAN_ANIMS, 255 none) and seconds into it.
    pub titan_anim: u8,
    pub titan_anim_t: f32,
    /// Rodeo: phase (0 none, 1 entrance, 2 idle, 3 rip, 4 grenade), the Titan ridden (a
    /// player id, or POOL_TARGET | pool index), entrance side/height, seconds into the phase.
    pub rodeo: u8,
    pub rodeo_target: u32,
    pub rodeo_side: u8,
    pub rodeo_height: u8,
    pub rodeo_t: f32,
    /// Shots fired so far (wrapping): the Titan's main gun and the Pilot's gun, whose
    /// pilotweapon::ARSENAL index is `pilot_gun`. Others draw each new shot.
    pub titan_shots: u16,
    pub pilot_shots: u16,
    pub pilot_gun: u8,
    pub titan_sprint: bool,
    /// What the player has up (F_*).
    pub flags: u16,
}

/// `PlayerSnap::flags`: the Titan's Vortex, Thermal Shield, Gun Shield or Phase Dash, the
/// Pilot's Cloak or Phase Shift.
pub const F_VORTEX: u16 = 1;
pub const F_HEAT: u16 = 2;
pub const F_GUN_SHIELD: u16 = 4;
pub const F_TITAN_PHASE: u16 = 8;
pub const F_CLOAK: u16 = 16;
pub const F_PHASE: u16 = 32;
/// Fitted with Bulwark (pas_legion_gunshield): the Gun Shield has 5000 health.
pub const F_BULWARK: u16 = 64;

/// `Msg::Event` kinds: a Titan's Arc Wave, Firewall or Flame Core (from its feet, along its
/// facing), a Gravity Star popping (at `pos`).
pub const EV_ARC_WAVE: u8 = 0;
pub const EV_FIREWALL: u8 = 1;
pub const EV_FLAME_CORE: u8 = 2;
pub const EV_GRAVITY_STAR: u8 = 3;
/// A Titan execution: `pos` the victim's spot (the shared reference), `dir` (victim player
/// id, BT_EXECUTIONS index, facing).
pub const EV_EXECUTION: u8 = 4;
/// A Titan's nuclear core going off (Nuclear Ejection), at `pos`: its sound and blast here
/// (the damage comes from the owner's hits).
pub const EV_NUKE: u8 = 5;

/// `Msg::Wall` kinds.
pub const WALL_PARTICLE: u8 = 0;
pub const WALL_AWALL: u8 = 1;

/// `rodeo_target` bit for a pooled AI Titan.
pub const POOL_TARGET: u32 = 0x8000_0000;

/// The Titan sequences a player's Titan can be in (PlayerTitan.override_anim), as sent.
pub const TITAN_ANIMS: &[&str] = &[
    "at_dismount_stand",
    "at_dismount_crouch",
    "at_MP_disembark_back2idle",
    "at_mount_stand_front",
    "at_mount_stand_behind",
    "at_mount_stand_left",
    "at_mount_stand_right",
    "at_mount_stand_airgrab",
    "at_mount_above_left",
    "at_mount_above_right",
    "at_mount_kneel_front",
    "at_mount_kneel_behind",
    "at_mount_kneel_left",
    "at_mount_kneel_right",
    "at_mount_kneel_above",
    "at_MP_eject_stand_start",
    "at_hotdrop_drop_2knee_turbo",
    "at_hotdrop_quickstand",
    "at_rodeo_ride_R_hijack_battery",
    "at_mount_kneel_airgrab",
    "bt_synced_titan_execute_flip_takedown_A",
    "bt_synced_titan_execute_kickshoot_A",
    "bt_synced_titan_execute_pilot_rip_A",
];
const RODEO_PANEL_ANIM: &str = "at_rodeo_ride_R_hijack_battery";

/// One pooled AI enemy, as the host tells the clients.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnemySnap {
    pub idx: u16,
    pub active: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub health: f32,
    pub shield: f32,
    pub doomed: bool,
    pub dead_for: f32,
    /// Titanfall drop / quickstand time; NaN when not in one.
    pub drop: f32,
    pub stand: f32,
    /// Where it is aiming.
    pub threat: Vec3,
    pub battery: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Client -> host, first thing after connecting.
    Hello {
        name: String,
    },
    /// Host -> client: its id, the mode and everyone already here. `started`: a match is on,
    /// join it now.
    Welcome {
        id: u32,
        mode: Mode,
        started: bool,
        players: Vec<(u32, String)>,
        /// The host's map: a client on another one restarts on it and joins again.
        map: String,
    },
    Joined {
        id: u32,
        name: String,
    },
    Left {
        id: u32,
    },
    /// Host -> clients: the match starts (or restarts).
    Start {
        mode: Mode,
    },
    /// Host -> clients: back to the lobby.
    Stop,
    /// A player's bodies (relayed by the host with the sender's id).
    Player {
        id: u32,
        snap: PlayerSnap,
    },
    /// Host -> clients: the AI enemies.
    Enemies {
        wave: u32,
        kills: u32,
        /// The wave is on (else between waves).
        playing: bool,
        snaps: Vec<EnemySnap>,
    },
    /// Host -> clients: an AI enemy fired a round (drawn there, harmless).
    Shot {
        pos: Vec3,
        vel: Vec3,
        gravity: f32,
        kit: u32,
    },
    /// Client -> host: this frame's local hits on a pooled enemy.
    HitEnemy {
        idx: u16,
        dmg: f32,
    },
    /// Anyone -> owner (via the host): damage to a player's Pilot or Titan, and where from.
    HitPlayer {
        id: u32,
        titan: bool,
        dmg: f32,
        from: Vec3,
    },
    /// A player put up a Particle Wall or an A-Wall (`kind`, relayed by the host): where,
    /// facing, life and health.
    Wall {
        id: u32,
        kind: u8,
        origin: Vec3,
        normal: Vec3,
        life: f32,
        hp: f32,
    },
    /// A player's one-off ability (EV_*, relayed by the host): drawn by the others, and an
    /// opponent's Gravity Star pulls them in.
    Event {
        id: u32,
        kind: u8,
        pos: Vec3,
        dir: Vec3,
    },
}

// --- codec ---------------------------------------------------------------------------------

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn b(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn v3(&mut self, v: Vec3) {
        self.f(v.x);
        self.f(v.y);
        self.f(v.z);
    }
    fn s(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(255);
        self.u8(n as u8);
        self.0.extend_from_slice(&b[..n]);
    }
}

struct R<'a>(&'a [u8], usize);

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn b(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn v3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f()?, self.f()?, self.f()?))
    }
    fn s(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).to_string())
    }
}

fn mode_u8(m: Mode) -> u8 {
    match m {
        Mode::Pve => 0,
        Mode::Pvp => 1,
    }
}

fn u8_mode(v: u8) -> Mode {
    if v == 1 {
        Mode::Pvp
    } else {
        Mode::Pve
    }
}

impl PlayerSnap {
    fn write(&self, w: &mut W) {
        w.b(self.on_foot);
        w.v3(self.pilot_pos);
        w.v3(self.pilot_vel);
        w.f(self.pilot_yaw);
        w.f(self.pilot_pitch);
        w.u8(self.body_move);
        w.f(self.pilot_health);
        w.b(self.titan_alive);
        w.v3(self.titan_pos);
        w.v3(self.titan_vel);
        w.f(self.titan_yaw);
        w.f(self.titan_pitch);
        w.f(self.titan_health);
        w.f(self.titan_max_health);
        w.f(self.titan_shield);
        w.f(self.titan_max_shield);
        w.b(self.titan_doomed);
        w.u8(self.titan_anim);
        w.f(self.titan_anim_t);
        w.u8(self.rodeo);
        w.u32(self.rodeo_target);
        w.u8(self.rodeo_side);
        w.u8(self.rodeo_height);
        w.f(self.rodeo_t);
        w.u16(self.titan_shots);
        w.u16(self.pilot_shots);
        w.u8(self.pilot_gun);
        w.b(self.titan_sprint);
        w.u16(self.flags);
    }
    fn read(r: &mut R) -> Option<Self> {
        Some(Self {
            on_foot: r.b()?,
            pilot_pos: r.v3()?,
            pilot_vel: r.v3()?,
            pilot_yaw: r.f()?,
            pilot_pitch: r.f()?,
            body_move: r.u8()?,
            pilot_health: r.f()?,
            titan_alive: r.b()?,
            titan_pos: r.v3()?,
            titan_vel: r.v3()?,
            titan_yaw: r.f()?,
            titan_pitch: r.f()?,
            titan_health: r.f()?,
            titan_max_health: r.f()?,
            titan_shield: r.f()?,
            titan_max_shield: r.f()?,
            titan_doomed: r.b()?,
            titan_anim: r.u8()?,
            titan_anim_t: r.f()?,
            rodeo: r.u8()?,
            rodeo_target: r.u32()?,
            rodeo_side: r.u8()?,
            rodeo_height: r.u8()?,
            rodeo_t: r.f()?,
            titan_shots: r.u16()?,
            pilot_shots: r.u16()?,
            pilot_gun: r.u8()?,
            titan_sprint: r.b()?,
            flags: r.u16()?,
        })
    }
}

impl EnemySnap {
    fn write(&self, w: &mut W) {
        w.u16(self.idx);
        w.b(self.active);
        w.v3(self.pos);
        w.v3(self.vel);
        w.f(self.yaw);
        w.f(self.health);
        w.f(self.shield);
        w.b(self.doomed);
        w.f(self.dead_for);
        w.f(self.drop);
        w.f(self.stand);
        w.v3(self.threat);
        w.b(self.battery);
    }
    fn read(r: &mut R) -> Option<Self> {
        Some(Self {
            idx: r.u16()?,
            active: r.b()?,
            pos: r.v3()?,
            vel: r.v3()?,
            yaw: r.f()?,
            health: r.f()?,
            shield: r.f()?,
            doomed: r.b()?,
            dead_for: r.f()?,
            drop: r.f()?,
            stand: r.f()?,
            threat: r.v3()?,
            battery: r.b()?,
        })
    }
}

impl Msg {
    fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::with_capacity(64));
        match self {
            Msg::Hello { name } => {
                w.u8(1);
                w.s(name);
            }
            Msg::Welcome {
                id,
                mode,
                started,
                players,
                map,
            } => {
                w.u8(2);
                w.s(map);
                w.u32(*id);
                w.u8(mode_u8(*mode));
                w.b(*started);
                w.u16(players.len() as u16);
                for (id, n) in players {
                    w.u32(*id);
                    w.s(n);
                }
            }
            Msg::Joined { id, name } => {
                w.u8(3);
                w.u32(*id);
                w.s(name);
            }
            Msg::Left { id } => {
                w.u8(4);
                w.u32(*id);
            }
            Msg::Start { mode } => {
                w.u8(5);
                w.u8(mode_u8(*mode));
            }
            Msg::Stop => w.u8(6),
            Msg::Player { id, snap } => {
                w.u8(7);
                w.u32(*id);
                snap.write(&mut w);
            }
            Msg::Enemies {
                wave,
                kills,
                playing,
                snaps,
            } => {
                w.u8(8);
                w.u32(*wave);
                w.u32(*kills);
                w.b(*playing);
                w.u16(snaps.len() as u16);
                for s in snaps {
                    s.write(&mut w);
                }
            }
            Msg::Shot {
                pos,
                vel,
                gravity,
                kit,
            } => {
                w.u8(9);
                w.v3(*pos);
                w.v3(*vel);
                w.f(*gravity);
                w.u32(*kit);
            }
            Msg::HitEnemy { idx, dmg } => {
                w.u8(10);
                w.u16(*idx);
                w.f(*dmg);
            }
            Msg::HitPlayer {
                id,
                titan,
                dmg,
                from,
            } => {
                w.u8(11);
                w.u32(*id);
                w.b(*titan);
                w.f(*dmg);
                w.v3(*from);
            }
            Msg::Wall {
                id,
                kind,
                origin,
                normal,
                life,
                hp,
            } => {
                w.u8(12);
                w.u32(*id);
                w.u8(*kind);
                w.v3(*origin);
                w.v3(*normal);
                w.f(*life);
                w.f(*hp);
            }
            Msg::Event { id, kind, pos, dir } => {
                w.u8(13);
                w.u32(*id);
                w.u8(*kind);
                w.v3(*pos);
                w.v3(*dir);
            }
        }
        w.0
    }

    fn decode(b: &[u8]) -> Option<Msg> {
        let mut r = R(b, 0);
        Some(match r.u8()? {
            1 => Msg::Hello { name: r.s()? },
            2 => {
                let map = r.s()?;
                let id = r.u32()?;
                let mode = u8_mode(r.u8()?);
                let started = r.b()?;
                let n = r.u16()? as usize;
                let mut players = Vec::with_capacity(n);
                for _ in 0..n {
                    players.push((r.u32()?, r.s()?));
                }
                Msg::Welcome {
                    id,
                    mode,
                    started,
                    players,
                    map,
                }
            }
            3 => Msg::Joined {
                id: r.u32()?,
                name: r.s()?,
            },
            4 => Msg::Left { id: r.u32()? },
            5 => Msg::Start {
                mode: u8_mode(r.u8()?),
            },
            6 => Msg::Stop,
            7 => Msg::Player {
                id: r.u32()?,
                snap: PlayerSnap::read(&mut r)?,
            },
            8 => {
                let wave = r.u32()?;
                let kills = r.u32()?;
                let playing = r.b()?;
                let n = r.u16()? as usize;
                let mut snaps = Vec::with_capacity(n);
                for _ in 0..n {
                    snaps.push(EnemySnap::read(&mut r)?);
                }
                Msg::Enemies {
                    wave,
                    kills,
                    playing,
                    snaps,
                }
            }
            9 => Msg::Shot {
                pos: r.v3()?,
                vel: r.v3()?,
                gravity: r.f()?,
                kit: r.u32()?,
            },
            10 => Msg::HitEnemy {
                idx: r.u16()?,
                dmg: r.f()?,
            },
            11 => Msg::HitPlayer {
                id: r.u32()?,
                titan: r.b()?,
                dmg: r.f()?,
                from: r.v3()?,
            },
            12 => Msg::Wall {
                id: r.u32()?,
                kind: r.u8()?,
                origin: r.v3()?,
                normal: r.v3()?,
                life: r.f()?,
                hp: r.f()?,
            },
            13 => Msg::Event {
                id: r.u32()?,
                kind: r.u8()?,
                pos: r.v3()?,
                dir: r.v3()?,
            },
            _ => return None,
        })
    }
}

// --- connections ---------------------------------------------------------------------------

/// One TCP connection: a reader and a writer thread, talked to over channels.
pub struct Conn {
    tx: Sender<Vec<u8>>,
    rx: Mutex<Receiver<Vec<u8>>>,
    alive: Arc<AtomicBool>,
    pub addr: String,
}

impl Conn {
    fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        stream.set_nonblocking(false)?;
        let addr = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let alive = Arc::new(AtomicBool::new(true));
        let (tx_out, rx_out) = channel::<Vec<u8>>();
        let (tx_in, rx_in) = channel::<Vec<u8>>();
        let mut reader = stream.try_clone()?;
        let mut writer = stream;
        let a = alive.clone();
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            loop {
                if reader.read_exact(&mut len).is_err() {
                    break;
                }
                let n = u32::from_le_bytes(len) as usize;
                if n > 1 << 22 {
                    break;
                }
                let mut buf = vec![0u8; n];
                if reader.read_exact(&mut buf).is_err() || tx_in.send(buf).is_err() {
                    break;
                }
            }
            a.store(false, Ordering::Relaxed);
        });
        let a = alive.clone();
        std::thread::spawn(move || {
            // Everything queued goes out in one write. When the link falls behind (a slow
            // upload, a Wi-Fi stall) a backlog builds; of the states in it only the newest of
            // each (a player's snapshot, the enemies) is still worth sending, so the delay
            // can't keep growing.
            let mut buf = Vec::new();
            while let Ok(first) = rx_out.recv() {
                let mut frames = vec![first];
                frames.extend(rx_out.try_iter());
                let key = |f: &[u8]| match f.first() {
                    Some(7) if f.len() >= 5 => {
                        Some((7u8, u32::from_le_bytes([f[1], f[2], f[3], f[4]])))
                    }
                    Some(8) => Some((8, 0)),
                    _ => None,
                };
                buf.clear();
                for (i, f) in frames.iter().enumerate() {
                    let superseded =
                        key(f).is_some_and(|k| frames[i + 1..].iter().any(|g| key(g) == Some(k)));
                    if superseded {
                        continue;
                    }
                    buf.extend_from_slice(&(f.len() as u32).to_le_bytes());
                    buf.extend_from_slice(f);
                }
                if writer.write_all(&buf).is_err() {
                    break;
                }
            }
            a.store(false, Ordering::Relaxed);
            let _ = writer.shutdown(std::net::Shutdown::Both);
        });
        Ok(Self {
            tx: tx_out,
            rx: Mutex::new(rx_in),
            alive,
            addr,
        })
    }
    fn send(&self, m: &Msg) {
        let _ = self.tx.send(m.encode());
    }
    fn recv(&self) -> Vec<Msg> {
        let mut out = Vec::new();
        if let Ok(rx) = self.rx.lock() {
            while let Ok(b) = rx.try_recv() {
                match Msg::decode(&b) {
                    Some(m) => out.push(m),
                    None => {
                        // Another version of the game (an update on one side only): say so
                        // now and then, not every frame.
                        static BAD: AtomicU32 = AtomicU32::new(0);
                        if BAD.fetch_add(1, Ordering::Relaxed) % 300 == 0 {
                            log::warn!(
                                "net: unreadable message from {} (code {:?}; is the other player on another version?)",
                                self.addr,
                                b.first()
                            );
                        }
                    }
                }
            }
        }
        out
    }
    fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

/// A joined client, as the host sees it.
pub struct Peer {
    pub id: u32,
    pub conn: Conn,
    /// Set by its Hello.
    pub name: Option<String>,
}

pub struct Host {
    listener: TcpListener,
    pub peers: Vec<Peer>,
    next_id: u32,
}

pub enum Role {
    Off,
    Host(Host),
    Connecting(Mutex<Receiver<std::io::Result<TcpStream>>>),
    Client(Conn),
}

#[derive(Clone, Debug)]
pub struct Player {
    pub id: u32,
    pub name: String,
}

/// A remote player's bodies here: a Pilot and a Titan `Enemy` entity (spawned by
/// `spawn_remotes`) and the last state they sent.
#[derive(Default)]
pub struct Remote {
    pub pilot: Option<Entity>,
    pub titan: Option<Entity>,
    pub snap: Option<PlayerSnap>,
    /// Seconds since the last snapshot.
    pub age: f32,
    /// The shot counters of the last snapshot drawn (Titan, Pilot).
    pub shots: Option<(u16, u16)>,
}

#[derive(Resource)]
pub struct Net {
    pub role: Role,
    pub mode: Mode,
    /// Menu text fields.
    pub port: String,
    pub address: String,
    pub name: String,
    pub my_id: u32,
    /// Everyone in the session, the host first.
    pub players: Vec<Player>,
    /// Lobby status line (errors, connecting, ...).
    pub status: String,
    /// The host has started the match.
    pub started: bool,
    pub remotes: HashMap<u32, Remote>,
    /// Health (plus shield) of the enemies after the last frame, to find this frame's local hits.
    synced: HashMap<Entity, f32>,
    player_tick: f32,
    enemy_tick: f32,
    /// Seconds until the dead local player respawns.
    pub respawn: Option<f32>,
    /// Messages other systems queued for sending (AI damage to remote players, enemy shots).
    pub queue: Vec<Msg>,
    /// Ghost rounds to draw (client).
    pub shots: Vec<(Vec3, Vec3, f32, usize)>,
    was_in_play: bool,
    /// Join in progress: the Welcome said a match is on.
    join_started: bool,
    /// Our Titan is playing the rodeo panel clip for a remote rider.
    panel_open: bool,
    /// The map this game loaded (`--map`).
    pub map: String,
    /// The host plays another map: restart on it and join this address again (ui.rs).
    pub relaunch: Option<(String, String)>,
    /// Other players' one-off abilities received this frame (player, EV_*, position,
    /// direction), for `remote_states`.
    pub events_in: Vec<(u32, u8, Vec3, Vec3)>,
}

/// Enemy shots the host fired this frame (targets.rs pushes, `update` sends).
pub static SHOTS: Mutex<Vec<(Vec3, Vec3, f32, usize)>> = Mutex::new(Vec::new());
pub static HOSTING: AtomicBool = AtomicBool::new(false);
/// Walls the local player put up this frame (titankit.rs, pilotability.rs): origin, normal,
/// life, hp and kind (WALL_*).
pub static WALLS: Mutex<Vec<(Vec3, Vec3, f32, f32, u8)>> = Mutex::new(Vec::new());
/// The local player's one-off abilities this frame (EV_*, position, direction).
pub static EVENTS: Mutex<Vec<(u8, Vec3, Vec3)>> = Mutex::new(Vec::new());
/// What the local player has up (F_*), gathered by `local_flags`.
pub static LOCAL_FLAGS: AtomicU32 = AtomicU32::new(0);

/// Note one of the local player's one-off abilities for the others (EV_*).
pub fn event(kind: u8, pos: Vec3, dir: Vec3) {
    if let Ok(mut e) = EVENTS.lock() {
        e.push((kind, pos, dir));
    }
}
/// Rounds the local Pilot has fired (pilotweapon.rs), for `PlayerSnap::pilot_shots`.
pub static PILOT_SHOTS: AtomicU32 = AtomicU32::new(0);
/// The local Pilot's gun (pilotweapon::ARSENAL index), for `PlayerSnap::pilot_gun`.
pub static PILOT_GUN: AtomicU32 = AtomicU32::new(0);

/// A remote Titan's extra clips: the side walks and sprint of the player's blend and the fire
/// gesture (targets.rs animate_remote_titan).
const REMOTE_TITAN_SEQUENCES: &[&str] = &[
    "bt_synced_titan_execute_flip_takedown_A",
    "bt_synced_titan_execute_kickshoot_A",
    "bt_synced_titan_execute_pilot_rip_A",
    "Walk_aim_all_left",
    "Walk_aim_all_right",
    "Sprint_mp_forward",
    "mp_stand_shootGeneric_add",
];

/// A remote Pilot's extra clips: the fire gestures and the third-person embarks and
/// disembarks, synced with the Titan's (`pilot_mount_clip`).
const REMOTE_PILOT_SEQUENCES: &[&str] = &[
    "gesture_mp_shootStand",
    "gesture_mp_shootCrouch",
    "pt_dismount_atlas_stand",
    "pt_dismount_atlas_crouch",
    "pt_mount_atlas_stand_front",
    "pt_mount_atlas_stand_behind",
    "pt_mount_atlas_stand_left",
    "pt_mount_atlas_stand_right",
    "pt_mount_atlas_stand_airgrab",
    "pt_mount_atlas_above_left",
    "pt_mount_atlas_above_right",
    "pt_mount_atlas_kneel_front",
    "pt_mount_atlas_kneel_behind",
    "pt_mount_atlas_kneel_left",
    "pt_mount_atlas_kneel_right",
    "pt_mount_atlas_kneel_airgrab",
    "pt_mount_atlas_kneel_above_left",
    "pt_mount_atlas_kneel_above_right",
];

/// The Pilot's third-person clip that goes with a Titan's embark or disembark sequence (the
/// game plays the pair together on the Titan's hijack attachment). `left`: the Pilot came from
/// the Titan's left (the one kneeling sequence from above serves both sides).
pub fn pilot_mount_clip(titan_anim: &str, left: bool) -> Option<String> {
    let rest = titan_anim.strip_prefix("at_")?;
    if let Some(how) = rest.strip_prefix("dismount_") {
        return Some(format!("pt_dismount_atlas_{how}"));
    }
    let how = rest.strip_prefix("mount_")?;
    if how == "kneel_above" {
        let side = if left { "left" } else { "right" };
        return Some(format!("pt_mount_atlas_kneel_above_{side}"));
    }
    Some(format!("pt_mount_atlas_{how}"))
}

/// Where a body at that yaw and pitch (positive down) looks.
fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    )
}

impl Default for Net {
    fn default() -> Self {
        let name = std::env::var("TF_NAME")
            .ok()
            .filter(|n| !n.trim().is_empty())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "PILOT".into())
            .to_uppercase();
        Self {
            role: Role::Off,
            mode: Mode::Pve,
            port: DEFAULT_PORT.to_string(),
            address: "127.0.0.1".into(),
            name,
            my_id: 0,
            players: Vec::new(),
            status: String::new(),
            started: false,
            remotes: HashMap::new(),
            synced: HashMap::new(),
            player_tick: 0.0,
            enemy_tick: 0.0,
            respawn: None,
            queue: Vec::new(),
            shots: Vec::new(),
            was_in_play: false,
            join_started: false,
            panel_open: false,
            map: String::new(),
            relaunch: None,
            events_in: Vec::new(),
        }
    }
}

impl Net {
    pub fn active(&self) -> bool {
        !matches!(self.role, Role::Off)
    }
    /// The host we joined (a client).
    pub fn host_address(&self) -> Option<String> {
        match &self.role {
            Role::Client(c) => Some(c.addr.clone()),
            Role::Connecting(_) => Some(self.address.clone()),
            _ => None,
        }
    }
    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host(_))
    }
    /// Joined (or joining) someone else's session: enemies are mirrored, not simulated.
    pub fn is_client(&self) -> bool {
        matches!(self.role, Role::Client(_) | Role::Connecting(_))
    }
    pub fn pvp(&self) -> bool {
        self.active() && self.mode == Mode::Pvp
    }
    /// A session where this machine runs no AI waves: a client, or a PvP host.
    pub fn no_waves(&self) -> bool {
        self.is_client() || self.pvp()
    }
    pub fn port_num(&self) -> u16 {
        self.port.parse().unwrap_or(DEFAULT_PORT)
    }

    /// Open the lobby: listen on the port.
    pub fn host(&mut self) {
        self.leave();
        let port = self.port_num();
        match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => {
                if let Err(e) = l.set_nonblocking(true) {
                    self.status = format!("LISTEN FAILED: {e}");
                    return;
                }
                self.role = Role::Host(Host {
                    listener: l,
                    peers: Vec::new(),
                    next_id: 1,
                });
                self.my_id = 0;
                self.players = vec![Player {
                    id: 0,
                    name: self.name.clone(),
                }];
                self.status = format!("HOSTING ON PORT {port} ({})", self.mode.name());
                HOSTING.store(true, Ordering::Relaxed);
                log::info!("net: hosting on port {port} ({:?})", self.mode);
            }
            Err(e) => self.status = format!("PORT {port}: {}", e.to_string().to_uppercase()),
        }
    }

    /// Connect to a host (in the background; `update` finishes the join).
    pub fn join(&mut self) {
        self.leave();
        let mut addr = self.address.trim().to_string();
        if addr.is_empty() {
            addr = "127.0.0.1".into();
        }
        if !addr.contains(':') {
            addr = format!("{addr}:{}", self.port_num());
        }
        let (tx, rx) = channel();
        let a = addr.clone();
        std::thread::spawn(move || {
            use std::net::ToSocketAddrs;
            let r = a
                .to_socket_addrs()
                .and_then(|mut it| {
                    it.next().ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "no such host")
                    })
                })
                .and_then(|sa| TcpStream::connect_timeout(&sa, std::time::Duration::from_secs(8)));
            let _ = tx.send(r);
        });
        self.role = Role::Connecting(Mutex::new(rx));
        self.status = format!("CONNECTING TO {addr}...");
        log::info!("net: connecting to {addr}");
    }

    /// Close the session (both roles).
    pub fn leave(&mut self) {
        if self.active() {
            log::info!("net: session closed");
        }
        self.role = Role::Off;
        self.players.clear();
        self.started = false;
        self.join_started = false;
        self.respawn = None;
        self.queue.clear();
        self.shots.clear();
        self.synced.clear();
        HOSTING.store(false, Ordering::Relaxed);
        // The remote entities are despawned by `spawn_remotes`.
        for r in self.remotes.values_mut() {
            r.snap = None;
        }
        self.status.clear();
    }

    fn broadcast(&self, m: &Msg, except: Option<u32>) {
        if let Role::Host(h) = &self.role {
            for p in &h.peers {
                if p.name.is_some() && Some(p.id) != except {
                    p.conn.send(m);
                }
            }
        }
    }

    fn send_to(&self, id: u32, m: &Msg) {
        match &self.role {
            Role::Host(h) => {
                if let Some(p) = h.peers.iter().find(|p| p.id == id) {
                    p.conn.send(m);
                }
            }
            Role::Client(c) => c.send(m),
            _ => {}
        }
    }

    /// Send to the host (a client), or handle it here (the host).
    fn to_host(&mut self, m: Msg) {
        match &self.role {
            Role::Client(c) => c.send(&m),
            Role::Host(_) => self.queue.push(m),
            _ => {}
        }
    }

    /// Where player `slot` spawns: the Titan start, then alternately left and right of it.
    pub fn spawn_offset(slot: u32, yaw: f32) -> Vec3 {
        if slot == 0 {
            return Vec3::ZERO;
        }
        let k = slot.div_ceil(2) as f32 * if slot % 2 == 1 { 1.0 } else { -1.0 };
        Vec3::new(yaw.sin(), -yaw.cos(), 0.0) * SLOT_SPACING * k
    }

    /// The remote players' bodies to shoot at: (player, feet, radius, height, is_pilot).
    pub fn targets(&self) -> Vec<(u32, Vec3, f32, f32, bool)> {
        let mut out = Vec::new();
        for (id, r) in &self.remotes {
            let Some(s) = r.snap else { continue };
            if s.on_foot && s.pilot_health > 0.0 {
                out.push((*id, s.pilot_pos, PILOT_RADIUS, PILOT_HEIGHT, true));
            }
            if s.titan_alive {
                out.push((*id, s.titan_pos, 70.0, 235.0, false));
            }
        }
        out
    }

    /// Queue damage done here to a remote player (AI melee, heat, rounds).
    pub fn hit_player(&mut self, id: u32, titan: bool, dmg: f32, from: Vec3) {
        if dmg > 0.0 {
            self.queue.push(Msg::HitPlayer {
                id,
                titan,
                dmg,
                from,
            });
        }
    }
}

/// `--host PORT [--pvp]` opens the lobby and `--join ADDR[:PORT]` connects as soon as the
/// world is up (scripted tests).
pub fn auto_session(
    time: Res<Time>,
    mut tries: Local<(u32, f32)>,
    args: Res<crate::Args>,
    mut net: ResMut<Net>,
    mut menu: ResMut<crate::ui::Menu>,
    game: Res<Game>,
) {
    if net.map.is_empty() {
        net.map = args.map.clone();
    }
    if let Some(port) = args.host {
        if tries.0 == 0 {
            tries.0 = 1;
            net.port = port.to_string();
            net.mode = if args.pvp { Mode::Pvp } else { Mode::Pve };
            net.host();
            if game.state == GameState::Title {
                menu.open_lobby();
            }
        }
    } else if let Some(addr) = &args.join {
        if tries.0 == 0 && game.state == GameState::Title {
            menu.open_lobby();
        }
        // The host may still be loading: try again every couple of seconds for a minute.
        tries.1 -= time.delta_secs();
        if net.active() || tries.0 >= 30 || tries.1 > 0.0 {
            return;
        }
        tries.0 += 1;
        tries.1 = 2.0;
        net.address = addr.clone();
        net.mode = if args.pvp { Mode::Pvp } else { Mode::Pve };
        net.join();
    }
}

/// Lobby, joins, state exchange and damage reports. Runs before the game director.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    mut net: ResMut<Net>,
    mut game: ResMut<Game>,
    mut input: ResMut<PlayerInput>,
    control: Res<Control>,
    kits: Res<EnemyKits>,
    mut pilots: Query<(&PlayerPilot, &mut PilotHealth)>,
    mut titans: Query<(&mut PlayerTitan, &mut crate::combat::TitanHealth)>,
    mut enemies: Query<(Entity, &mut Enemy)>,
    mut damage_from: ResMut<crate::hud::DamageFrom>,
    (mut feed, rodeo): (ResMut<crate::feed::Feed>, Res<crate::rodeo::Rodeo>),
    weapons: Query<&crate::weapons::Weapon, With<PlayerTitan>>,
) {
    let dt = time.delta_secs();
    // The walls put up and abilities used since last frame (sent below when online).
    let walls: Vec<(Vec3, Vec3, f32, f32, u8)> = WALLS
        .lock()
        .map(|mut w| std::mem::take(&mut *w))
        .unwrap_or_default();
    let events: Vec<(u8, Vec3, Vec3)> = EVENTS
        .lock()
        .map(|mut e| std::mem::take(&mut *e))
        .unwrap_or_default();
    if !net.active() {
        net.was_in_play = game.in_play();
        return;
    }
    for r in net.remotes.values_mut() {
        r.age += dt;
    }

    // --- connections: accept, finish joining, drop the dead ---
    let mut inbox: Vec<(u32, Msg)> = Vec::new();
    let mut gone: Vec<u32> = Vec::new();
    let mut lost_host = false;
    let mut connected: Option<std::io::Result<TcpStream>> = None;
    match &mut net.role {
        Role::Host(h) => {
            loop {
                match h.listener.accept() {
                    Ok((s, a)) => match Conn::new(s) {
                        Ok(c) => {
                            let id = h.next_id;
                            h.next_id += 1;
                            log::info!("net: connection from {a} as player {id}");
                            h.peers.push(Peer {
                                id,
                                conn: c,
                                name: None,
                            });
                        }
                        Err(e) => log::warn!("net: accept: {e}"),
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        log::warn!("net: accept: {e}");
                        break;
                    }
                }
            }
            for p in &h.peers {
                for m in p.conn.recv() {
                    inbox.push((p.id, m));
                }
                if !p.conn.alive() {
                    gone.push(p.id);
                }
            }
            h.peers.retain(|p| !gone.contains(&p.id));
        }
        Role::Connecting(rx) => {
            connected = rx.lock().ok().and_then(|rx| rx.try_recv().ok());
        }
        Role::Client(c) => {
            for m in c.recv() {
                inbox.push((0, m));
            }
            lost_host = !c.alive();
        }
        Role::Off => {}
    }
    match connected {
        Some(Ok(s)) => match Conn::new(s) {
            Ok(c) => {
                c.send(&Msg::Hello {
                    name: net.name.clone(),
                });
                net.role = Role::Client(c);
                net.status = "CONNECTED, WAITING FOR THE HOST...".into();
            }
            Err(e) => {
                net.role = Role::Off;
                net.status = format!("CONNECT FAILED: {e}").to_uppercase();
            }
        },
        Some(Err(e)) => {
            net.role = Role::Off;
            net.status = format!("CONNECT FAILED: {e}").to_uppercase();
            log::warn!("net: connect failed: {e}");
        }
        None => {}
    }
    if lost_host {
        log::warn!("net: lost the host");
        net.leave();
        net.status = "LOST THE CONNECTION TO THE HOST".into();
        if game.in_play() {
            game.state = GameState::Title;
            for (_, mut e) in &mut enemies {
                e.active = false;
            }
        }
        return;
    }
    for id in gone {
        let name = net
            .players
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone());
        net.players.retain(|p| p.id != id);
        if let Some(r) = net.remotes.get_mut(&id) {
            r.snap = None;
        }
        if let Some(n) = name {
            log::info!("net: {n} ({id}) left");
            net.status = format!("{n} LEFT");
            net.broadcast(&Msg::Left { id }, None);
        }
    }

    // --- messages ---
    let is_host = net.is_host();
    let local_pilot_pos = pilots
        .single()
        .ok()
        .map(|(p, _)| Vec3::from(p.state.pos.to_array()))
        .unwrap_or_default();
    let queued: Vec<Msg> = std::mem::take(&mut net.queue);
    for m in queued {
        inbox.push((net.my_id, m));
    }
    let mut enemy_snaps: Option<(u32, u32, bool, Vec<EnemySnap>)> = None;
    for (from, m) in inbox {
        match m {
            Msg::Hello { name } if is_host => {
                let name = if name.trim().is_empty() {
                    format!("PILOT {from}")
                } else {
                    name
                };
                let players: Vec<(u32, String)> =
                    net.players.iter().map(|p| (p.id, p.name.clone())).collect();
                let welcome = Msg::Welcome {
                    id: from,
                    mode: net.mode,
                    started: game.in_play(),
                    players,
                    map: net.map.clone(),
                };
                net.send_to(from, &welcome);
                if let Role::Host(h) = &mut net.role {
                    if let Some(p) = h.peers.iter_mut().find(|p| p.id == from) {
                        p.name = Some(name.clone());
                    }
                }
                net.broadcast(
                    &Msg::Joined {
                        id: from,
                        name: name.clone(),
                    },
                    Some(from),
                );
                net.players.push(Player {
                    id: from,
                    name: name.clone(),
                });
                net.remotes.entry(from).or_insert(Remote::default());
                net.status = format!("{name} JOINED");
                log::info!("net: {name} joined as player {from}");
            }
            Msg::Welcome {
                id,
                mode,
                started,
                players,
                map,
            } => {
                // Everyone plays the host's map: on another one, restart on it and join again.
                if !map.is_empty() && map != net.map {
                    let addr = match &net.role {
                        Role::Client(c) => c.addr.clone(),
                        _ => net.address.clone(),
                    };
                    log::info!("net: the host plays {map}, restarting on it to join {addr}");
                    net.status = format!("LOADING {}...", map.to_uppercase());
                    net.relaunch = Some((map, addr));
                    continue;
                }
                net.my_id = id;
                net.mode = mode;
                net.players = players
                    .iter()
                    .map(|(id, n)| Player {
                        id: *id,
                        name: n.clone(),
                    })
                    .collect();
                let my_name = net.name.clone();
                net.players.push(Player { id, name: my_name });
                for (pid, _) in &players {
                    net.remotes.entry(*pid).or_insert(Remote::default());
                }
                net.status = format!("IN THE LOBBY ({})", mode.name());
                log::info!("net: joined as player {id}, {} others", players.len());
                if started {
                    net.join_started = true;
                }
            }
            Msg::Joined { id, name } => {
                net.players.push(Player {
                    id,
                    name: name.clone(),
                });
                net.remotes.entry(id).or_insert(Remote::default());
                net.status = format!("{name} JOINED");
            }
            Msg::Left { id } => {
                let name = net
                    .players
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                net.players.retain(|p| p.id != id);
                if let Some(r) = net.remotes.get_mut(&id) {
                    r.snap = None;
                }
                net.status = format!("{name} LEFT");
            }
            Msg::Start { mode } if !is_host => {
                net.mode = mode;
                net.started = true;
                // Like PLAY: the director starts a fresh run from the title state.
                game.state = GameState::GameOver;
                input.start = true;
                log::info!("net: the host started the match ({:?})", mode);
            }
            Msg::Stop if !is_host => {
                net.started = false;
                game.state = GameState::Title;
                for (_, mut e) in &mut enemies {
                    e.active = false;
                }
            }
            Msg::Player { id, snap } => {
                let id = if is_host { from } else { id };
                if is_host {
                    net.broadcast(&Msg::Player { id, snap }, Some(id));
                }
                if let Some(r) = net.remotes.get_mut(&id) {
                    if r.snap.is_none() {
                        log::info!(
                            "net: first state of player {id}: pilot {:.0?} titan {:.0?}",
                            snap.pilot_pos,
                            snap.titan_pos
                        );
                    }
                    r.snap = Some(snap);
                    r.age = 0.0;
                }
            }
            Msg::Enemies {
                wave,
                kills,
                playing,
                snaps,
            } if !is_host => {
                enemy_snaps = Some((wave, kills, playing, snaps));
            }
            Msg::Shot {
                pos,
                vel,
                gravity,
                kit,
            } if !is_host => net.shots.push((pos, vel, gravity, kit as usize)),
            Msg::HitEnemy { idx, dmg } if is_host => {
                for (_, mut e) in &mut enemies {
                    if e.pool == Some(idx as usize) && e.alive() {
                        e.damage(dmg, false);
                        break;
                    }
                }
            }
            Msg::HitPlayer {
                id,
                titan,
                dmg,
                from: at,
            } => {
                // Phased out (Phase Shift, Phase Dash), nothing touches us.
                let flags = LOCAL_FLAGS.load(Ordering::Relaxed) as u16;
                let phased = flags & if titan { F_TITAN_PHASE } else { F_PHASE } != 0;
                if id == net.my_id && phased {
                    log::info!("net: phased, ignoring {dmg:.0} from player {from}");
                } else if id == net.my_id {
                    // AI damage comes from the host, PvP damage from whoever hit us.
                    if titan {
                        for (_, mut h) in &mut titans {
                            // Another Titan landing on ours (damagedef_titan_fall) bypasses the
                            // shield and the doomed state.
                            if dmg >= crate::pilotctl::TITANFALL_DAMAGE_HEAVY * 0.5 {
                                h.v.shield = 0.0;
                                if h.v.doomed.is_none() {
                                    h.v.doomed = Some(1.0);
                                }
                            }
                            h.damage_from(dmg, false, at);
                        }
                    } else {
                        for (_, mut h) in &mut pilots {
                            h.damage(dmg);
                        }
                    }
                    damage_from.push(at);
                    log::info!(
                        "net: took {dmg:.0} to the {} from player {from}",
                        if titan { "Titan" } else { "Pilot" }
                    );
                } else if is_host {
                    net.send_to(
                        id,
                        &Msg::HitPlayer {
                            id,
                            titan,
                            dmg,
                            from: at,
                        },
                    );
                }
            }
            Msg::Wall {
                id,
                kind,
                origin,
                normal,
                life,
                hp,
            } => {
                let id = if is_host { from } else { id };
                if is_host {
                    net.broadcast(
                        &Msg::Wall {
                            id,
                            kind,
                            origin,
                            normal,
                            life,
                            hp,
                        },
                        Some(id),
                    );
                }
                if id != net.my_id {
                    // An opponent's wall stops our shots (PvP); a teammate's stops the AI's
                    // rounds like our own (PvE).
                    crate::titankit::ParticleWall::spawn(
                        &mut commands,
                        origin,
                        normal,
                        life,
                        hp,
                        net.pvp(),
                        kind == WALL_AWALL,
                    );
                    log::info!(
                        "net: player {id} put up {} at {origin:.0}",
                        if kind == WALL_AWALL {
                            "an A-Wall"
                        } else {
                            "a Particle Wall"
                        }
                    );
                }
            }
            Msg::Event { id, kind, pos, dir } => {
                let id = if is_host { from } else { id };
                if is_host {
                    net.broadcast(&Msg::Event { id, kind, pos, dir }, Some(id));
                }
                if id != net.my_id {
                    log::info!("net: player {id} event {kind} at {pos:.0}");
                    net.events_in.push((id, kind, pos, dir));
                }
            }
            _ => {}
        }
    }

    // A joined match that is already on: start playing now.
    if net.join_started && !net.started && matches!(net.role, Role::Client(_)) {
        net.join_started = false;
        net.started = true;
        game.state = GameState::GameOver;
        input.start = true;
        log::info!("net: joining the match in progress");
    }

    // --- the host tells everyone when the match starts (lobby START, a script start) ---
    let in_play = game.in_play();
    if is_host {
        if in_play && !net.was_in_play {
            net.started = true;
            net.broadcast(&Msg::Start { mode: net.mode }, None);
            log::info!("net: match started ({:?})", net.mode);
        } else if !in_play && net.was_in_play {
            net.started = false;
            net.broadcast(&Msg::Stop, None);
        }
    }
    net.was_in_play = in_play;
    if in_play {
        for (kind, pos, dir) in events {
            let m = Msg::Event {
                id: net.my_id,
                kind,
                pos,
                dir,
            };
            if is_host {
                net.broadcast(&m, None);
            } else {
                net.to_host(m);
            }
        }
        for (origin, normal, life, hp, kind) in walls {
            let m = Msg::Wall {
                id: net.my_id,
                kind,
                origin,
                normal,
                life,
                hp,
            };
            if is_host {
                net.broadcast(&m, None);
            } else {
                net.to_host(m);
            }
        }
    }

    // --- this frame's local hits on things owned elsewhere ---
    let pvp = net.pvp();
    let mut reports: Vec<Msg> = Vec::new();
    let is_client = net.is_client();
    for (id, e) in &enemies {
        let owned_elsewhere = e.remote.is_some() || (is_client && e.pool.is_some());
        if !owned_elsewhere {
            continue;
        }
        let cur = e.v.health + e.v.shield;
        if let Some(last) = net.synced.get(&id) {
            let dmg = last - cur;
            if dmg > 0.5 && e.active {
                if let Some(idx) = e.pool {
                    reports.push(Msg::HitEnemy {
                        idx: idx as u16,
                        dmg,
                    });
                } else if let Some(pid) = e.remote {
                    if pvp {
                        reports.push(Msg::HitPlayer {
                            id: pid,
                            titan: !e.infantry,
                            dmg,
                            from: local_pilot_pos,
                        });
                    }
                }
            }
        }
    }
    for m in reports {
        net.to_host(m);
    }
    // --- apply the enemy snapshot (client) ---
    if let Some((wave, kills, playing, snaps)) = enemy_snaps {
        game.wave = wave;
        game.kills = kills;
        if game.in_play() {
            game.state = if playing {
                GameState::Playing
            } else {
                GameState::Intermission
            };
        }
        for (_, mut e) in &mut enemies {
            let Some(idx) = e.pool else { continue };
            if let Some(s) = snaps.iter().find(|s| s.idx as usize == idx) {
                let kit = &kits.0[e.kit.min(kits.0.len() - 1)];
                e.apply_net(s, kit);
            }
        }
    }

    // --- apply the remote players' snapshots to their bodies ---
    let mut kill_names: Vec<String> = Vec::new();
    let mut synced_now: Vec<(Entity, f32)> = Vec::new();
    // What a rider's `rodeo_target` means here: a player's Titan body or a pooled Titan.
    let resolve_target = |code: u32, net: &Net, enemies: &Query<(Entity, &mut Enemy)>| {
        if code & POOL_TARGET != 0 {
            let idx = (code & !POOL_TARGET) as usize;
            enemies
                .iter()
                .find(|(_, e)| e.pool == Some(idx))
                .map(|(id, _)| id)
        } else {
            net.remotes.get(&code).and_then(|r| r.titan)
        }
    };
    let my_id = net.my_id;
    let mut ridden_by_remote: Option<f32> = None;
    // New shots since the last snapshot drawn (none on the first one), at most a few a frame.
    let fresh = |now: u16, last: Option<u16>| last.map_or(0, |l| now.wrapping_sub(l).min(6));
    let mut shot_marks: Vec<(u32, (u16, u16))> = Vec::new();
    let mut ghosts: Vec<(Vec3, Vec3, f32, usize)> = Vec::new();
    for (pid, r) in net.remotes.iter() {
        let Some(s) = r.snap else { continue };
        shot_marks.push((*pid, (s.titan_shots, s.pilot_shots)));
        let name = net
            .players
            .iter()
            .find(|p| p.id == *pid)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        if let Some(te) = r.titan {
            if let Ok((_, mut e)) = enemies.get_mut(te) {
                let kit = &kits.0[e.kit.min(kits.0.len() - 1)];
                let was_alive = e.alive();
                if s.titan_alive && !e.active {
                    e.activate(s.titan_pos, s.titan_yaw, kit);
                    e.cancel_arrival();
                }
                if s.titan_alive {
                    e.set_remote_pose(s.titan_pos, s.titan_vel, s.titan_yaw, s.titan_pitch);
                    e.sprint = s.titan_sprint;
                    // Its main gun's shots: the fire gesture, flash, sound and a harmless round
                    // (the owner's own rounds do the damage there).
                    let n = fresh(s.titan_shots, r.shots.map(|x| x.0));
                    if n > 0 {
                        log::debug!("net: player {pid}'s Titan fired {n}");
                        e.fire_t = 0.0;
                        let dir = look_dir(s.titan_yaw, s.titan_pitch);
                        let fwd = Vec3::new(s.titan_yaw.cos(), s.titan_yaw.sin(), 0.0);
                        let muzzle = e.pos + Vec3::Z * 190.0 + fwd * 110.0;
                        for _ in 0..n {
                            crate::audio::cue(
                                &mut commands,
                                crate::audio::Cue::EnemyGun(kit.arsenal as u8),
                                Some(muzzle),
                            );
                            crate::particles::emit(
                                &mut commands,
                                crate::particles::Effect::Muzzle {
                                    at: crate::player::to_bevy(muzzle),
                                    dir: crate::player::to_bevy(dir).normalize_or(Vec3::NEG_Z),
                                    scale: 3.0,
                                    energy: false,
                                },
                            );
                            ghosts.push((muzzle, dir * kit.projectile, kit.gravity, e.kit));
                        }
                    }
                    e.override_anim = TITAN_ANIMS
                        .get(s.titan_anim as usize)
                        .map(|n| (n.to_string(), s.titan_anim_t));
                    let v = &mut e.v;
                    v.max_health = s.titan_max_health.max(1.0);
                    v.max_shield = s.titan_max_shield;
                    v.health = s.titan_health;
                    v.shield = s.titan_shield;
                    v.doomed = if s.titan_doomed {
                        Some(v.doomed.unwrap_or(1.0))
                    } else {
                        None
                    };
                    e.dead_for = 0.0;
                } else if e.active && was_alive {
                    e.v.health = 0.0;
                    e.dead_for = 0.0;
                    kill_names.push(format!("{name}'S TITAN"));
                }
                synced_now.push((te, e.v.health + e.v.shield));
            }
        }
        if let Some(pe) = r.pilot {
            // The rodeo, resolved to the Titan body it rides here (before borrowing the body).
            let ride = (s.rodeo != 0)
                .then(|| resolve_target(s.rodeo_target, &net, &enemies))
                .flatten()
                .map(|target| {
                    let chassis = if s.rodeo_target & POOL_TARGET != 0 {
                        enemies
                            .get(target)
                            .ok()
                            .and_then(|(_, t)| kits.0.get(t.kit))
                            .map(|k| chassis_name(&k.model))
                            .unwrap_or("atlas")
                    } else {
                        chassis_name(crate::PLAYER_TITAN_MODEL)
                    };
                    crate::targets::RemoteRide {
                        target,
                        chassis,
                        phase: s.rodeo,
                        side: s.rodeo_side,
                        height: s.rodeo_height,
                        t: s.rodeo_t,
                        clip: None,
                    }
                });
            // Climbing in or out: the Pilot body plays its half of the Titan's sequence on
            // the Titan's hijack point (else it would pop out next to the Titan at the end).
            let left_of_titan = (s.pilot_pos - s.titan_pos)
                .truncate()
                .dot(Vec2::new(-s.titan_yaw.sin(), s.titan_yaw.cos()))
                > 0.0;
            let mount = (!s.on_foot && s.titan_alive && s.pilot_health > 0.0)
                .then(|| TITAN_ANIMS.get(s.titan_anim as usize))
                .flatten()
                .and_then(|a| pilot_mount_clip(a, left_of_titan))
                .zip(r.titan)
                .map(|(clip, target)| crate::targets::RemoteRide {
                    target,
                    chassis: "atlas",
                    phase: 1,
                    side: 0,
                    height: 0,
                    t: s.titan_anim_t,
                    clip: Some(clip),
                });
            let ride = ride.or(mount);
            let mounting = ride.as_ref().is_some_and(|r| r.clip.is_some());
            if let Ok((_, mut e)) = enemies.get_mut(pe) {
                let kit = &kits.0[e.kit.min(kits.0.len() - 1)];
                let show = (s.on_foot || mounting) && s.pilot_health > 0.0;
                let was_alive = e.alive();
                if show && !e.active {
                    e.activate(s.pilot_pos, s.pilot_yaw, kit);
                    e.cancel_arrival();
                }
                if show {
                    e.set_remote_pose(s.pilot_pos, s.pilot_vel, s.pilot_yaw, s.pilot_pitch);
                    e.body_move = s.body_move;
                    e.ride = ride.clone();
                    let n = fresh(s.pilot_shots, r.shots.map(|x| x.1));
                    if n > 0 && s.on_foot && ride.is_none() {
                        log::debug!("net: player {pid}'s Pilot fired {n}");
                        e.fire_t = 0.0;
                        let dir = look_dir(s.pilot_yaw, s.pilot_pitch);
                        let right = Vec3::new(s.pilot_yaw.sin(), -s.pilot_yaw.cos(), 0.0);
                        let muzzle = e.pos + Vec3::Z * 52.0 + dir * 30.0 + right * 8.0;
                        for _ in 0..n {
                            crate::audio::cue(
                                &mut commands,
                                crate::audio::Cue::RemoteGun(s.pilot_gun),
                                Some(muzzle),
                            );
                            crate::particles::emit(
                                &mut commands,
                                crate::particles::Effect::Muzzle {
                                    at: crate::player::to_bevy(muzzle),
                                    dir: crate::player::to_bevy(dir).normalize_or(Vec3::NEG_Z),
                                    scale: 1.0,
                                    energy: false,
                                },
                            );
                            ghosts.push((muzzle, dir * 12000.0, 0.0, e.kit));
                        }
                    }
                    if s.rodeo == 3 && s.rodeo_target == my_id {
                        ridden_by_remote = Some(s.rodeo_t);
                    }
                    e.v.health = s.pilot_health;
                    e.v.max_health = 100.0;
                    e.dead_for = 0.0;
                } else if e.active && was_alive {
                    e.v.health = 0.0;
                    e.dead_for = 0.0;
                    if s.on_foot {
                        kill_names.push(name.clone());
                    } else {
                        // Climbed into the Titan: gone, not dead.
                        e.active = false;
                    }
                }
                synced_now.push((pe, e.v.health + e.v.shield));
            }
        }
    }
    for (e, h) in synced_now {
        net.synced.insert(e, h);
    }
    for (pid, marks) in shot_marks {
        if let Some(r) = net.remotes.get_mut(&pid) {
            r.shots = Some(marks);
        }
    }
    net.shots.extend(ghosts);
    // A remote rider ripping our battery: our Titan opens its panel for them (its copy on the
    // rider's side plays the same clip, rodeo.rs).
    if let Ok((mut titan, _)) = titans.single_mut() {
        match ridden_by_remote {
            Some(t) => {
                titan.override_anim = Some((RODEO_PANEL_ANIM.into(), t));
                net.panel_open = true;
            }
            None if net.panel_open => {
                if titan
                    .override_anim
                    .as_ref()
                    .is_some_and(|(n, _)| n == RODEO_PANEL_ANIM)
                {
                    titan.override_anim = None;
                }
                net.panel_open = false;
            }
            None => {}
        }
    }
    for n in kill_names {
        feed.kills.push(crate::feed::Kill {
            victim: n,
            weapon: String::new(),
            at: time.elapsed_secs(),
        });
    }
    // Everything else owned elsewhere: remember the health after this frame.
    for (id, e) in &enemies {
        if e.remote.is_some() || (is_client && e.pool.is_some()) {
            net.synced.insert(id, e.v.health + e.v.shield);
        }
    }

    // --- send our own bodies ---
    net.player_tick += dt;
    if net.player_tick >= 1.0 / PLAYER_RATE && in_play {
        net.player_tick = 0.0;
        if let (Ok((pilot, ph)), Ok((titan, th))) = (pilots.single(), titans.single()) {
            let ps = &pilot.state;
            let ts = &titan.state;
            let (titan_anim, titan_anim_t) = titan
                .override_anim
                .as_ref()
                .and_then(|(n, t)| {
                    TITAN_ANIMS
                        .iter()
                        .position(|a| a == n)
                        .map(|i| (i as u8, *t))
                })
                .unwrap_or((255, 0.0));
            let (rodeo_phase, rodeo_target, rodeo_side, rodeo_height, rodeo_t) = rodeo
                .ride
                .as_ref()
                .and_then(|r| {
                    let (target, phase, side, height, t) = r.net_state();
                    let (_, e) = enemies.get(target).ok()?;
                    let code = e
                        .remote
                        .or_else(|| e.pool.map(|i| POOL_TARGET | i as u32))?;
                    Some((phase, code, side, height, t))
                })
                .unwrap_or((0, 0, 0, 0, 0.0));
            let snap = PlayerSnap {
                on_foot: *control == Control::Pilot,
                pilot_pos: Vec3::from(ps.pos.to_array()),
                pilot_vel: Vec3::from(ps.vel.to_array()),
                pilot_yaw: ps.yaw,
                pilot_pitch: ps.pitch,
                body_move: body_move_code(ps),
                pilot_health: ph.health,
                titan_alive: th.dead_for.is_none(),
                titan_pos: Vec3::from(ts.pos.to_array()),
                titan_vel: Vec3::from(ts.vel.to_array()),
                titan_yaw: ts.yaw,
                titan_pitch: ts.pitch,
                titan_health: th.v.health,
                titan_max_health: th.v.max_health,
                titan_shield: th.v.shield,
                titan_max_shield: th.v.max_shield,
                titan_doomed: th.v.doomed.is_some(),
                titan_anim,
                titan_anim_t,
                rodeo: rodeo_phase,
                rodeo_target,
                rodeo_side,
                rodeo_height,
                rodeo_t,
                titan_shots: weapons.single().map_or(0, |w| w.shots as u16),
                pilot_shots: PILOT_SHOTS.load(Ordering::Relaxed) as u16,
                pilot_gun: PILOT_GUN.load(Ordering::Relaxed) as u8,
                titan_sprint: ts.sprinting,
                flags: LOCAL_FLAGS.load(Ordering::Relaxed) as u16,
            };
            let m = Msg::Player {
                id: net.my_id,
                snap,
            };
            if is_host {
                net.broadcast(&m, None);
            } else if let Role::Client(c) = &net.role {
                c.send(&m);
            }
        }
    }

    // --- the host sends the enemies and their shots ---
    if is_host {
        net.enemy_tick += dt;
        if net.enemy_tick >= 1.0 / ENEMY_RATE && in_play {
            net.enemy_tick = 0.0;
            let snaps: Vec<EnemySnap> = enemies
                .iter()
                .filter_map(|(_, e)| e.pool.map(|i| e.net_state(i as u16)))
                .collect();
            net.broadcast(
                &Msg::Enemies {
                    wave: game.wave,
                    kills: game.kills,
                    playing: game.state == GameState::Playing,
                    snaps,
                },
                None,
            );
        }
        let shots: Vec<_> = SHOTS
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        for (pos, vel, gravity, kit) in shots {
            net.broadcast(
                &Msg::Shot {
                    pos,
                    vel,
                    gravity,
                    kit: kit as u32,
                },
                None,
            );
        }
    } else if let Ok(mut s) = SHOTS.lock() {
        s.clear();
    }
}

/// The Pilot body's locomotion state code for `PlayerSnap`.
fn body_move_code(s: &tf_sim::pilot::PilotState) -> u8 {
    use tf_sim::pilot::PilotMove;
    let v = Vec2::new(s.vel.x, s.vel.y);
    let left = Vec2::new(-s.yaw.sin(), s.yaw.cos());
    match s.mode {
        PilotMove::Slide => MOVE_SLIDE,
        PilotMove::WallRun { normal, .. } => {
            if Vec2::new(normal.x, normal.y).dot(left) < 0.0 {
                MOVE_WALLRUN_LEFT
            } else {
                MOVE_WALLRUN_RIGHT
            }
        }
        PilotMove::WallHang { .. } => MOVE_WALL_HANG,
        PilotMove::Air | PilotMove::Mantle { .. } => MOVE_AIR,
        PilotMove::Ground if v.length() < 20.0 => MOVE_IDLE,
        PilotMove::Ground if s.crouched => MOVE_CROUCH_RUN,
        PilotMove::Ground => MOVE_RUN,
    }
}

pub fn body_move(code: u8) -> crate::pilotbody::BodyMove {
    use crate::pilotbody::BodyMove;
    match code {
        MOVE_RUN => BodyMove::Run,
        MOVE_CROUCH_RUN => BodyMove::CrouchRun,
        MOVE_SLIDE => BodyMove::Slide,
        MOVE_WALLRUN_LEFT => BodyMove::WallRun { wall_left: true },
        MOVE_WALLRUN_RIGHT => BodyMove::WallRun { wall_left: false },
        MOVE_WALL_HANG => BodyMove::WallHang,
        MOVE_AIR => BodyMove::Air,
        _ => BodyMove::Idle,
    }
}

/// Spawn the bodies of players who joined (a Pilot and a Titan `Enemy` each) and remove those
/// of players who left.
#[allow(clippy::too_many_arguments)]
pub fn spawn_remotes(
    mut commands: Commands,
    mut net: ResMut<Net>,
    gd: Res<crate::gamedata::GameData>,
    root: Option<Res<crate::pilotweapon::WorldRoot>>,
    kits: Res<EnemyKits>,
    arena: Option<Res<crate::game::Arena>>,
    mut cache: ResMut<crate::convert::Cache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let Some(root) = root else { return };
    let present: Vec<u32> = net.players.iter().map(|p| p.id).collect();
    let my_id = net.my_id;
    let active = net.active();
    // Gone: despawn.
    let gone: Vec<u32> = net
        .remotes
        .iter()
        .filter(|(id, _)| !active || !present.contains(id) || **id == my_id)
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        if let Some(r) = net.remotes.remove(&id) {
            for e in [r.pilot, r.titan].into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
    }
    if !active {
        return;
    }
    let Some(titan_kit) = kits.0.iter().position(|k| k.remote && !k.infantry) else {
        return;
    };
    let Some(pilot_kit) = kits.0.iter().position(|k| k.remote && k.infantry) else {
        return;
    };
    let at = arena.map(|a| a.player_spawn).unwrap_or_default();
    let ids: Vec<u32> = net
        .remotes
        .iter()
        .filter(|(_, r)| r.pilot.is_none() || r.titan.is_none())
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        let mut spawn = |kit_index: usize, extra: &[&str]| -> Option<Entity> {
            let kit = &kits.0[kit_index];
            let mut seqs = crate::enemy_sequences(kit);
            seqs.extend_from_slice(extra);
            let anchor = commands
                .spawn((Transform::from_translation(at), Visibility::Hidden))
                .id();
            commands.entity(root.0).add_child(anchor);
            let spec = crate::actor::ActorSpec {
                path: &kit.model,
                sequences: &seqs,
                grids: &crate::targets::aim_grids(&kit.model),
                body: &[],
            };
            match crate::actor::spawn_actor(
                &mut commands,
                anchor,
                &gd,
                &spec,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut bindposes,
            ) {
                Ok(a) => {
                    if !kit.infantry {
                        if let Some(hand) = a.joint("ja_c_propGun") {
                            let gun = crate::actor::ActorSpec {
                                path: &kit.weapon.playermodel,
                                sequences: &[],
                                grids: &[],
                                body: &[],
                            };
                            if let Err(e) = crate::actor::spawn_actor(
                                &mut commands,
                                hand,
                                &gd,
                                &gun,
                                &mut cache,
                                &mut meshes,
                                &mut images,
                                &mut materials,
                                &mut bindposes,
                            ) {
                                log::warn!("remote gun: {e:#}");
                            }
                        }
                    }
                    let mut e =
                        Enemy::new(at, a.entity, 0x51ED_2704 * (id as u64 + 7), kit_index, kit);
                    e.remote = Some(id);
                    // Its owner pops their own smoke; no anti-rodeo charge here.
                    e.smoke_charges = 0;
                    commands.entity(anchor).insert(e);
                    Some(anchor)
                }
                Err(e) => {
                    log::warn!("remote player body: {e:#}");
                    commands.entity(anchor).despawn();
                    None
                }
            }
        };
        let mut titan_seqs: Vec<&str> = TITAN_ANIMS.to_vec();
        titan_seqs.extend_from_slice(REMOTE_TITAN_SEQUENCES);
        let titan = spawn(titan_kit, &titan_seqs);
        let mut pilot_seqs: Vec<&str> = crate::pilotbody::BODY_SEQUENCES.to_vec();
        pilot_seqs.extend_from_slice(crate::targets::PILOT_RODEO_SEQUENCES);
        pilot_seqs.extend_from_slice(REMOTE_PILOT_SEQUENCES);
        let pilot = spawn(pilot_kit, &pilot_seqs);
        if let Some(r) = net.remotes.get_mut(&id) {
            r.titan = titan;
            r.pilot = pilot;
            log::info!("net: spawned the bodies of player {id}");
        }
    }
}

/// The chassis name of a Titan model for the rodeo clips (rodeo.rs Chassis).
pub fn chassis_name(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    if m.contains("titan_heavy") {
        "ogre"
    } else if m.contains("titan_light") {
        "stryder"
    } else {
        "atlas"
    }
}

/// The kits remote players' bodies use: the players' chassis (Tone) with its gun, and the
/// Pilot body model.
pub fn remote_kits(read: &mut dyn FnMut(&str) -> Option<String>) -> Vec<crate::targets::EnemyKit> {
    let tone = crate::targets::ENEMY_CLASSES
        .iter()
        .position(|c| c.3 == "TONE")
        .unwrap_or(0);
    let mut titan = crate::targets::EnemyKit::class(tone, read);
    titan.model = crate::PLAYER_TITAN_MODEL.into();
    titan.ability = None;
    titan.defense = None;
    titan.remote = true;
    let mut pilot = crate::targets::EnemyKit::infantry(read);
    pilot.model = crate::pilotbody::PILOT_BODY_MODEL.into();
    pilot.name = "PILOT".into();
    pilot.radius = PILOT_RADIUS;
    pilot.height = PILOT_HEIGHT;
    pilot.vitals.health = 100.0;
    pilot.vitals.max_health = 100.0;
    pilot.vitals.shield = 0.0;
    pilot.vitals.max_shield = 0.0;
    pilot.vitals.doomed_health = 0.0;
    pilot.remote = true;
    vec![titan, pilot]
}

/// What the local player has up, for the snapshot's flags (F_*).
pub fn local_flags(
    control: Res<Control>,
    status: Res<crate::pilotability::PilotStatus>,
    mods: Res<crate::titankit::TitanMoveMods>,
    titans: Query<(&crate::combat::Vortex, &crate::titankit::KitState), With<PlayerTitan>>,
) {
    let mut f = 0u16;
    if let Ok((v, k)) = titans.single() {
        if v.active {
            f |= if v.mode == crate::combat::VortexMode::Heat {
                F_HEAT
            } else {
                F_VORTEX
            };
        }
        if k.gun_shield > 0.0 && k.gun_shield_hp > 0.0 {
            f |= F_GUN_SHIELD;
        }
        if crate::kits::has("pas_legion_gunshield") {
            f |= F_BULWARK;
        }
        if mods.phased || k.phase > 0.0 {
            f |= F_TITAN_PHASE;
        }
    }
    if *control == Control::Pilot {
        if status.cloaked() {
            f |= F_CLOAK;
        }
        if status.phased() {
            f |= F_PHASE;
        }
    }
    LOCAL_FLAGS.store(f as u32, Ordering::Relaxed);
}

/// A remote Gravity Star pulling the local Pilot: where, and seconds since it popped.
#[derive(Default)]
pub struct RemoteStar(Vec3, f32);

/// The others' states on their bodies here: an opponent's Vortex, Thermal Shield or Gun Shield
/// stops our rounds (targets::Enemy::set_remote_defense), a phased player isn't drawn, a
/// cloaked Pilot is drawn cloaked (cloak.rs). Their one-off abilities play here: Arc Waves,
/// Firewalls and Flame Cores as harmless copies (their damage comes from the owner's hits), an
/// opponent's Gravity Star pulling the local Pilot in (the intense pull after the pop,
/// GRAV_PULL_ACCEL up to GRAV_PULL_SPEED within GRAV_PULL_RADIUS).
#[allow(clippy::too_many_arguments)]
pub fn remote_states(
    mut commands: Commands,
    time: Res<Time>,
    mut net: ResMut<Net>,
    control: Res<Control>,
    mut enemies: Query<&mut Enemy>,
    mut pilots: Query<&mut PlayerPilot>,
    titans: Query<&PlayerTitan>,
    mut stars: Local<Vec<RemoteStar>>,
    mut executed: ResMut<crate::executions::BeingExecuted>,
) {
    use crate::pilotability::{
        GRAV_PULL_ACCEL, GRAV_PULL_DELAY, GRAV_PULL_RADIUS, GRAV_PULL_SPEED,
    };
    use crate::targets::DefenseKind;
    let dt = time.delta_secs();
    if !net.active() {
        stars.clear();
        net.events_in.clear();
        return;
    }
    let pvp = net.pvp();
    let on_foot = *control == Control::Pilot;
    let me = if on_foot {
        pilots
            .single()
            .ok()
            .map(|p| Vec3::from(p.state.pos.to_array()))
    } else {
        titans
            .single()
            .ok()
            .map(|t| Vec3::from(t.state.pos.to_array()))
    }
    .unwrap_or(Vec3::ZERO);
    for r in net.remotes.values() {
        let Some(s) = r.snap else { continue };
        if let Some(mut e) = r.titan.and_then(|t| enemies.get_mut(t).ok()) {
            e.hidden = s.flags & F_TITAN_PHASE != 0;
            // A teammate's shields (PvE) don't stop our rounds.
            let kind = if !pvp || !s.titan_alive {
                None
            } else if s.flags & F_VORTEX != 0 {
                Some(DefenseKind::Vortex)
            } else if s.flags & F_HEAT != 0 {
                Some(DefenseKind::Heat)
            } else if s.flags & F_GUN_SHIELD != 0 {
                Some(DefenseKind::GunShield)
            } else {
                None
            };
            // PAS_LEGION_SHEILD_HEALTH / TITAN_GUN_SHIELD_HEALTH.
            e.shield_max = if s.flags & F_BULWARK != 0 {
                5000.0
            } else {
                2500.0
            };
            e.set_remote_defense(&mut commands, kind, me, s.titan_yaw);
        }
        if let Some(mut e) = r.pilot.and_then(|p| enemies.get_mut(p).ok()) {
            // Riding our Titan: from the cockpit the rider is out of sight and out of reach
            // on its back (only Electric Smoke or getting out deals with them); on foot we see
            // them up on its back, where we can shoot them off.
            let on_ours = s.on_foot && s.rodeo != 0 && s.rodeo_target == net.my_id;
            e.hidden = s.on_foot && (s.flags & F_PHASE != 0 || (on_ours && !on_foot));
            e.cloaked = s.on_foot && s.flags & F_CLOAK != 0;
            if on_ours && on_foot {
                if let Ok(t) = titans.single() {
                    let fwd = Vec3::new(t.state.yaw.cos(), t.state.yaw.sin(), 0.0);
                    let back = Vec3::from(t.state.pos.to_array()) - fwd * 60.0 + Vec3::Z * 160.0;
                    e.set_remote_pose(back, Vec3::ZERO, t.state.yaw, 0.0);
                }
            }
        }
    }
    for (_, kind, pos, dir) in std::mem::take(&mut net.events_in) {
        use crate::titankit::WaveKind;
        match kind {
            EV_ARC_WAVE | EV_FIREWALL | EV_FLAME_CORE => {
                let wave = match kind {
                    EV_ARC_WAVE => WaveKind::Arc,
                    EV_FIREWALL => WaveKind::Firewall,
                    _ => WaveKind::Flame,
                };
                crate::titankit::spawn_remote_wave(&mut commands, wave, pos, dir, pvp);
            }
            EV_EXECUTION if dir.x as u32 == net.my_id && *control == Control::Titan => {
                log::info!("net: being executed (sequence {})", dir.y);
                executed.start(pos, dir.z, dir.y as usize);
            }
            EV_NUKE => {
                crate::audio::event_at(&mut commands, "titan_nuclear_death_explode", pos);
                crate::pfx::emit_named(
                    &mut commands,
                    "P_xo_exp_nuke_3P_alt",
                    crate::player::to_bevy(pos - Vec3::Z * 100.0),
                    Vec3::Y,
                );
            }
            EV_GRAVITY_STAR => {
                crate::audio::event_at(&mut commands, "weapon_gravitystar_preexplo", pos);
                stars.push(RemoteStar(pos, 0.0));
            }
            _ => {}
        }
    }
    stars.retain_mut(|star| {
        star.1 += dt;
        if star.1 >= GRAV_PULL_DELAY {
            crate::particles::emit(
                &mut commands,
                crate::particles::Effect::Explosion {
                    at: crate::player::to_bevy(star.0),
                    scale: 1.0,
                },
            );
            return false;
        }
        // An opponent's star pulls the local Pilot (a teammate's doesn't).
        if pvp && on_foot {
            if let Ok(mut p) = pilots.single_mut() {
                let at = Vec3::from(p.state.pos.to_array()) + Vec3::Z * 36.0;
                let to = star.0 - at;
                let dist = to.length();
                if dist < GRAV_PULL_RADIUS && dist > 20.0 {
                    let dir = to / dist;
                    let speed = (GRAV_PULL_ACCEL * star.1).min(GRAV_PULL_SPEED);
                    let vel = Vec3::from(p.state.vel.to_array());
                    let toward = vel.dot(dir);
                    if toward < speed {
                        let v = vel + dir * (speed - toward);
                        p.state.vel = tf_sim::glam::Vec3::new(v.x, v.y, v.z);
                    }
                }
            }
        }
        true
    });
}
