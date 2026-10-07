//! Controller support from the game's own controller data (read from the VPKs at runtime):
//! - `cfg/gamepad_button_layout_<layout>.cfg`: which button runs which command
//!   (`bind "A_BUTTON" "+ability 3"`, ...).
//! - `cfg/aimassist/looksensitivity.txt` (and `_zoomed`, `_titan`, `_titan_zoomed`): per look
//!   sensitivity setting, the yaw/pitch speeds at full deflection, and the extra "acceleration"
//!   speed that ramps in (over `accel_time`, after `accel_time_delay`) while the stick is held at
//!   the edge, shaped by `cfg/aimassist/accelcurve.txt`.
//! - `cfg/aimassist/aimcurve_look_N.txt`: the response curves (piecewise linear points, then a
//!   squared / cubed / linear transform).
//!
//! The left stick moves (`joy_movement_stick 0`), the right stick looks. Stick dead zones are
//! engine-side and not in the files; this uses a 0.15 radial inner dead zone.

use crate::gamedata::GameData;
use bevy::prelude::*;

/// What a controller button does, from the layout cfg's command names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadCommand {
    /// `+ability 3`: jump (BT dashes).
    Jump,
    /// `+toggle_duck`.
    ToggleDuck,
    /// `+useandreload`: tap to reload, hold to use (embark/disembark).
    UseAndReload,
    /// `+ability 7`: switch between the primary and the sidearm.
    SwitchWeapon,
    /// `+ability 6`: the anti-Titan weapon.
    AntiTitan,
    Zoom,
    Attack,
    /// `+offhand1`: the Pilot's tactical / BT's defensive.
    Offhand1,
    /// `+offhand0`: the Pilot's ordnance / BT's ordnance.
    Offhand0,
    /// `+ability 1`: call in a Titan.
    Titanfall,
    Sprint,
    Melee,
    Pause,
    /// Anything else the layout binds (scoreboard, script commands): no action here.
    Other,
}

impl PadCommand {
    fn from_cfg(s: &str) -> PadCommand {
        match s.trim() {
            "+ability 3" | "+jump" => PadCommand::Jump,
            "+toggle_duck" | "+duck" => PadCommand::ToggleDuck,
            "+useandreload" => PadCommand::UseAndReload,
            "+ability 7" | "+weaponcycle" => PadCommand::SwitchWeapon,
            "+ability 6" => PadCommand::AntiTitan,
            "+zoom" | "+toggle_zoom" => PadCommand::Zoom,
            "+attack" => PadCommand::Attack,
            "+offhand1" => PadCommand::Offhand1,
            "+offhand0" => PadCommand::Offhand0,
            "+ability 1" => PadCommand::Titanfall,
            "+speed" => PadCommand::Sprint,
            "+melee" => PadCommand::Melee,
            "ingamemenu_activate" => PadCommand::Pause,
            _ => PadCommand::Other,
        }
    }
}

/// One row of a `looksensitivity*.txt` table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookRow {
    pub yaw_speed: f32,
    pub pitch_speed: f32,
    pub accel_yaw: f32,
    pub accel_pitch: f32,
    pub accel_delay: f32,
    pub accel_time: f32,
}

/// A response curve: piecewise-linear points then a power transform.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub points: Vec<(f32, f32)>,
    pub power: i32,
}

impl Curve {
    pub fn linear() -> Self {
        Curve { points: vec![(1.0, 1.0)], power: 1 }
    }

    /// Parse `LINEAR x y` lines and a `TRANSFORM: squared|cubed|linear` line.
    pub fn parse(text: &str) -> Self {
        let mut points = Vec::new();
        let mut power = 1;
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.first().copied() {
                Some("LINEAR") if w.len() >= 3 => {
                    if let (Ok(x), Ok(y)) = (w[1].parse(), w[2].parse()) {
                        points.push((x, y));
                    }
                }
                Some("TRANSFORM:") => {
                    power = match w.get(1).copied() {
                        Some("squared") => 2,
                        Some("cubed") => 3,
                        _ => 1,
                    }
                }
                _ => {}
            }
        }
        if points.is_empty() {
            points.push((1.0, 1.0));
        }
        Curve { points, power }
    }

    /// Map an input magnitude 0..1: the transform shapes the input, the points map it (from
    /// (0, 0) through each point).
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0).powi(self.power);
        let mut prev = (0.0f32, 0.0f32);
        for &(px, py) in &self.points {
            if x <= px {
                let k = if px > prev.0 { (x - prev.0) / (px - prev.0) } else { 1.0 };
                return prev.1 + (py - prev.1) * k;
            }
            prev = (px, py);
        }
        prev.1
    }
}

/// The game's controller data.
#[derive(Resource, Default)]
pub struct PadConfig {
    pub buttons: Vec<(GamepadButton, PadCommand)>,
    pub look: Vec<LookRow>,
    pub look_zoomed: Vec<LookRow>,
    pub look_titan: Vec<LookRow>,
    pub look_titan_zoomed: Vec<LookRow>,
    pub curves: Vec<Curve>,
    pub accel: Option<Curve>,
    pub layout: String,
}

fn button(name: &str) -> Option<GamepadButton> {
    Some(match name {
        "A_BUTTON" => GamepadButton::South,
        "B_BUTTON" => GamepadButton::East,
        "X_BUTTON" => GamepadButton::West,
        "Y_BUTTON" => GamepadButton::North,
        "L_TRIGGER" => GamepadButton::LeftTrigger2,
        "R_TRIGGER" => GamepadButton::RightTrigger2,
        "L_SHOULDER" => GamepadButton::LeftTrigger,
        "R_SHOULDER" => GamepadButton::RightTrigger,
        "UP" => GamepadButton::DPadUp,
        "DOWN" => GamepadButton::DPadDown,
        "LEFT" => GamepadButton::DPadLeft,
        "RIGHT" => GamepadButton::DPadRight,
        "STICK1" => GamepadButton::LeftThumb,
        "STICK2" => GamepadButton::RightThumb,
        "BACK" => GamepadButton::Select,
        "START" => GamepadButton::Start,
        _ => return None,
    })
}

/// `bind "A_BUTTON" "+ability 3"` lines of a layout cfg.
pub fn parse_layout(text: &str) -> Vec<(GamepadButton, PadCommand)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let q: Vec<&str> = line.split('"').collect();
        if line.starts_with("bind") && q.len() >= 4 {
            if let Some(b) = button(q[1]) {
                out.push((b, PadCommand::from_cfg(q[3])));
            }
        }
    }
    out
}

/// The rows of a `looksensitivity*.txt` table.
pub fn parse_look(text: &str) -> Vec<LookRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let w: Vec<&str> = line.split_whitespace().collect();
        // number yaw pitch "curve" accel_yaw accel_pitch delay time cutoff
        if w.len() >= 8 && w[0].parse::<u32>().is_ok() {
            let f = |i: usize| w.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            rows.push(LookRow { yaw_speed: f(1), pitch_speed: f(2), accel_yaw: f(4), accel_pitch: f(5), accel_delay: f(6), accel_time: f(7) });
        }
    }
    rows
}

/// Read the controller data once the game files are open (and again if the layout setting
/// changes).
pub fn load_pad_config(gd: Option<Res<GameData>>, settings: Res<crate::settings::Settings>, mut cfg: ResMut<PadConfig>) {
    let Some(gd) = gd else { return };
    if !cfg.look.is_empty() && cfg.layout == settings.gamepad_layout {
        return;
    }
    let read = |p: &str| gd.read_file(p).ok().map(|b| String::from_utf8_lossy(&b).to_string());
    cfg.layout = settings.gamepad_layout.clone();
    cfg.buttons = read(&format!("cfg/gamepad_button_layout_{}.cfg", cfg.layout)).map(|t| parse_layout(&t)).unwrap_or_default();
    cfg.look = read("cfg/aimassist/looksensitivity.txt").map(|t| parse_look(&t)).unwrap_or_default();
    cfg.look_zoomed = read("cfg/aimassist/looksensitivity_zoomed.txt").map(|t| parse_look(&t)).unwrap_or_default();
    cfg.look_titan = read("cfg/aimassist/looksensitivity_titan.txt").map(|t| parse_look(&t)).unwrap_or_default();
    cfg.look_titan_zoomed = read("cfg/aimassist/looksensitivity_titan_zoomed.txt").map(|t| parse_look(&t)).unwrap_or_default();
    cfg.curves = (0..5).map(|i| read(&format!("cfg/aimassist/aimcurve_look_{i}.txt")).map(|t| Curve::parse(&t)).unwrap_or_else(Curve::linear)).collect();
    cfg.accel = read("cfg/aimassist/accelcurve.txt").map(|t| Curve::parse(&t));
    if cfg.look.is_empty() {
        // Keep the controller usable without the files.
        cfg.look = vec![LookRow { yaw_speed: 160.0, pitch_speed: 120.0, accel_yaw: 220.0, accel_pitch: 0.0, accel_delay: 0.0, accel_time: 0.33 }; 8];
    }
    log::info!("controller: layout {} ({} buttons), {} look settings, {} curves", cfg.layout, cfg.buttons.len(), cfg.look.len(), cfg.curves.len());
}

/// Per-frame controller state carried between frames.
#[derive(Resource, Default)]
pub struct PadState {
    /// Seconds the look stick has been held at the edge.
    pub edge_time: f32,
    /// Seconds the use/reload button has been held.
    pub use_held: f32,
    pub use_fired: bool,
    pub crouch_toggled: bool,
    /// Whether the controller was the last thing used (aim assist only applies then).
    pub active: bool,
}

/// Radial dead zone and rescale.
pub fn deadzone(v: Vec2, inner: f32) -> Vec2 {
    let m = v.length();
    if m <= inner {
        return Vec2::ZERO;
    }
    v / m * ((m - inner) / (1.0 - inner)).min(1.0)
}

/// Look speed in degrees per second for a deflected right stick: the curve shapes the
/// magnitude, the table row gives the full-deflection speeds, and holding the stick at the
/// edge ramps in the acceleration speed.
pub fn look_rate(stick: Vec2, row: &LookRow, curve: &Curve, accel: Option<&Curve>, edge_time: f32) -> Vec2 {
    let m = stick.length();
    if m <= 0.0 {
        return Vec2::ZERO;
    }
    let dir = stick / m;
    let k = curve.eval(m);
    let mut rate = Vec2::new(dir.x * row.yaw_speed, dir.y * row.pitch_speed) * k;
    if m >= 0.99 && row.accel_time > 0.0 && edge_time > row.accel_delay {
        let t = ((edge_time - row.accel_delay) / row.accel_time).clamp(0.0, 1.0);
        let a = accel.map(|c| c.eval(t)).unwrap_or(t);
        rate += Vec2::new(dir.x * row.accel_yaw, dir.y * row.accel_pitch) * a;
    }
    rate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_tables_parse() {
        let layout = "bind \"A_BUTTON\"\t\t\"+ability 3\"\nbind \"R_TRIGGER\"\t\"+attack\"\n// x\nbind \"START\" \"ingamemenu_activate\"";
        let b = parse_layout(layout);
        assert_eq!(b, vec![(GamepadButton::South, PadCommand::Jump), (GamepadButton::RightTrigger2, PadCommand::Attack), (GamepadButton::Start, PadCommand::Pause)]);
        let look = "//\tnumber\tyaw\n\t2\t\t160\t\t\t120\t\t\t\t\"cfg/aimassist/accelcurve.txt\"\t220\t\t\t\t\t\t0\t\t\t\t\t\t0.0\t\t\t\t\t0.33\t\t\t\t50";
        let rows = parse_look(look);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].yaw_speed, 160.0);
        assert_eq!(rows[0].accel_yaw, 220.0);
        assert!((rows[0].accel_time - 0.33).abs() < 1e-6);
    }

    #[test]
    fn curves() {
        let squared = Curve::parse("// Classic\nLINEAR\t1.0\t\t1.0\nTRANSFORM: squared");
        assert!((squared.eval(0.5) - 0.25).abs() < 1e-6);
        let accel = Curve::parse("LINEAR 0.3 0.15\nLINEAR 0.6 0.4\nLINEAR 1.0 1.0\nTRANSFORM: linear");
        assert!((accel.eval(0.3) - 0.15).abs() < 1e-6);
        assert!((accel.eval(0.8) - 0.7).abs() < 1e-6);
        assert_eq!(accel.eval(1.0), 1.0);
    }

    #[test]
    fn look_rate_accelerates_at_the_edge() {
        let row = LookRow { yaw_speed: 160.0, pitch_speed: 120.0, accel_yaw: 220.0, accel_pitch: 0.0, accel_delay: 0.0, accel_time: 0.33 };
        let lin = Curve::linear();
        let base = look_rate(Vec2::X, &row, &lin, None, 0.0);
        assert!((base.x - 160.0).abs() < 1e-3);
        let full = look_rate(Vec2::X, &row, &lin, None, 1.0);
        assert!((full.x - 380.0).abs() < 1e-3);
        let half = look_rate(Vec2::X * 0.5, &row, &lin, None, 1.0);
        assert!((half.x - 80.0).abs() < 1e-3);
        assert_eq!(deadzone(Vec2::new(0.1, 0.0), 0.15), Vec2::ZERO);
    }
}
