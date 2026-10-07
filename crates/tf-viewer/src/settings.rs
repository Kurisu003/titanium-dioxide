//! Player settings, persisted like the game's own config: a Source-style cfg file of convars
//! and `bind "KEY" "+command"` lines in the user's config directory
//! (`$XDG_CONFIG_HOME/titanium-dioxide/settings.cfg`, else `~/.config/...`), never in the repo.
//!
//! Ranges follow the game's settings menus (`resource/ui/menus/controls.menu`, `video.menu`):
//! `mouse_sensitivity` and `mouse_sensitivity_zoomed` 0-20 in steps of 0.2, `cl_fovScale`
//! 1.0-1.55 in steps of 0.0275 on the game's 70 degree (horizontal, 4:3) base FOV,
//! `m_invert_pitch`, and the controller's `gamepad_aim_speed` / `gamepad_aim_speed_ads`
//! (rows 0-7 of `cfg/aimassist/looksensitivity*.txt`), `gamepad_look_curve` (the five
//! `aimcurve_look_N.txt` curves), `joy_inverty`, `gamepad_aim_assist`. The defaults themselves
//! live in the engine, not the scripts; the ones here are Source's usual values.

use bevy::prelude::*;
use std::path::PathBuf;

/// One rebindable command, named like the game's console commands where there is one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Command {
    Forward,
    Back,
    Left,
    Right,
    Sprint,
    Jump,
    Crouch,
    Fire,
    Ads,
    Reload,
    Melee,
    Use,
    /// Q: BT's ordnance, the Pilot's tactical.
    Ability,
    /// E: BT's defensive ability.
    Utility,
    /// G: the Pilot's ordnance.
    Throw,
    Core,
    Titanfall,
    View,
    Slot1,
    Slot2,
    Slot3,
}

impl Command {
    pub const ALL: [Command; 21] = [
        Command::Forward,
        Command::Back,
        Command::Left,
        Command::Right,
        Command::Sprint,
        Command::Jump,
        Command::Crouch,
        Command::Fire,
        Command::Ads,
        Command::Reload,
        Command::Melee,
        Command::Use,
        Command::Ability,
        Command::Utility,
        Command::Throw,
        Command::Core,
        Command::Titanfall,
        Command::View,
        Command::Slot1,
        Command::Slot2,
        Command::Slot3,
    ];

    /// The cfg name (the game's console command where one exists).
    pub fn cfg(self) -> &'static str {
        match self {
            Command::Forward => "+forward",
            Command::Back => "+back",
            Command::Left => "+moveleft",
            Command::Right => "+moveright",
            Command::Sprint => "+speed",
            Command::Jump => "+jump",
            Command::Crouch => "+duck",
            Command::Fire => "+attack",
            Command::Ads => "+zoom",
            Command::Reload => "+reload",
            Command::Melee => "+melee",
            Command::Use => "+use",
            Command::Ability => "+offhand1",
            Command::Utility => "+offhand2",
            Command::Throw => "+offhand0",
            Command::Core => "+ability 1",
            Command::Titanfall => "+scriptcommand1",
            Command::View => "toggle_view",
            Command::Slot1 => "weaponselect_primary0",
            Command::Slot2 => "weaponselect_primary1",
            Command::Slot3 => "weaponselect_primary2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::Forward => "MOVE FORWARD",
            Command::Back => "MOVE BACK",
            Command::Left => "MOVE LEFT",
            Command::Right => "MOVE RIGHT",
            Command::Sprint => "SPRINT",
            Command::Jump => "JUMP / DASH",
            Command::Crouch => "CROUCH / SLIDE",
            Command::Fire => "FIRE",
            Command::Ads => "AIM DOWN SIGHTS",
            Command::Reload => "RELOAD",
            Command::Melee => "MELEE",
            Command::Use => "USE / EMBARK",
            Command::Ability => "TACTICAL / TITAN DEFENSIVE",
            Command::Utility => "TITAN UTILITY",
            Command::Throw => "ORDNANCE / TITAN ORDNANCE",
            Command::Core => "TITAN CORE",
            Command::Titanfall => "CALL TITAN",
            Command::View => "COCKPIT / THIRD PERSON",
            Command::Slot1 => "PRIMARY WEAPON",
            Command::Slot2 => "SIDEARM",
            Command::Slot3 => "ANTI-TITAN WEAPON",
        }
    }

    fn from_cfg(s: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.cfg().eq_ignore_ascii_case(s))
    }

    /// This build's default binding.
    pub fn default_bind(self) -> Bind {
        use KeyCode as K;
        match self {
            Command::Forward => Bind::Key(K::KeyW),
            Command::Back => Bind::Key(K::KeyS),
            Command::Left => Bind::Key(K::KeyA),
            Command::Right => Bind::Key(K::KeyD),
            Command::Sprint => Bind::Key(K::ShiftLeft),
            Command::Jump => Bind::Key(K::Space),
            Command::Crouch => Bind::Key(K::ControlLeft),
            Command::Fire => Bind::Mouse(MouseButton::Left),
            Command::Ads => Bind::Mouse(MouseButton::Right),
            Command::Reload => Bind::Key(K::KeyR),
            Command::Melee => Bind::Key(K::KeyF),
            Command::Use => Bind::Key(K::KeyX),
            Command::Ability => Bind::Key(K::KeyQ),
            Command::Utility => Bind::Key(K::KeyE),
            Command::Throw => Bind::Key(K::KeyG),
            Command::Core => Bind::Key(K::KeyV),
            Command::Titanfall => Bind::Key(K::KeyT),
            Command::View => Bind::Key(K::KeyC),
            Command::Slot1 => Bind::Key(K::Digit1),
            Command::Slot2 => Bind::Key(K::Digit2),
            Command::Slot3 => Bind::Key(K::Digit3),
        }
    }
}

/// A key or mouse button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bind {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Bind {
    pub fn pressed(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Bind::Key(k) => keys.pressed(k),
            Bind::Mouse(m) => mouse.pressed(m),
        }
    }
    pub fn just_pressed(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Bind::Key(k) => keys.just_pressed(k),
            Bind::Mouse(m) => mouse.just_pressed(m),
        }
    }

    /// Source key name ("W", "SPACE", "MOUSE1").
    pub fn name(self) -> String {
        match self {
            Bind::Mouse(MouseButton::Left) => "MOUSE1".into(),
            Bind::Mouse(MouseButton::Right) => "MOUSE2".into(),
            Bind::Mouse(MouseButton::Middle) => "MOUSE3".into(),
            Bind::Mouse(MouseButton::Back) => "MOUSE4".into(),
            Bind::Mouse(MouseButton::Forward) => "MOUSE5".into(),
            Bind::Mouse(MouseButton::Other(n)) => format!("MOUSE{}", n + 6),
            Bind::Key(k) => KEY_NAMES.iter().find(|(_, kk)| *kk == k).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("{k:?}").to_uppercase()),
        }
    }

    pub fn parse(s: &str) -> Option<Bind> {
        let u = s.to_ascii_uppercase();
        let mouse = match u.as_str() {
            "MOUSE1" => Some(MouseButton::Left),
            "MOUSE2" => Some(MouseButton::Right),
            "MOUSE3" => Some(MouseButton::Middle),
            "MOUSE4" => Some(MouseButton::Back),
            "MOUSE5" => Some(MouseButton::Forward),
            _ => None,
        };
        if let Some(m) = mouse {
            return Some(Bind::Mouse(m));
        }
        KEY_NAMES.iter().find(|(n, _)| *n == u).map(|(_, k)| Bind::Key(*k))
    }
}

/// Source key names for the keys this build can bind.
pub const KEY_NAMES: &[(&str, KeyCode)] = {
    use KeyCode as K;
    &[
        ("A", K::KeyA), ("B", K::KeyB), ("C", K::KeyC), ("D", K::KeyD), ("E", K::KeyE), ("F", K::KeyF), ("G", K::KeyG),
        ("H", K::KeyH), ("I", K::KeyI), ("J", K::KeyJ), ("K", K::KeyK), ("L", K::KeyL), ("M", K::KeyM), ("N", K::KeyN),
        ("O", K::KeyO), ("P", K::KeyP), ("Q", K::KeyQ), ("R", K::KeyR), ("S", K::KeyS), ("T", K::KeyT), ("U", K::KeyU),
        ("V", K::KeyV), ("W", K::KeyW), ("X", K::KeyX), ("Y", K::KeyY), ("Z", K::KeyZ),
        ("0", K::Digit0), ("1", K::Digit1), ("2", K::Digit2), ("3", K::Digit3), ("4", K::Digit4), ("5", K::Digit5),
        ("6", K::Digit6), ("7", K::Digit7), ("8", K::Digit8), ("9", K::Digit9),
        ("SPACE", K::Space), ("SHIFT", K::ShiftLeft), ("RSHIFT", K::ShiftRight), ("CTRL", K::ControlLeft),
        ("RCTRL", K::ControlRight), ("ALT", K::AltLeft), ("RALT", K::AltRight), ("TAB", K::Tab), ("CAPSLOCK", K::CapsLock),
        ("ENTER", K::Enter), ("BACKSPACE", K::Backspace), ("`", K::Backquote), ("-", K::Minus), ("=", K::Equal),
        ("[", K::BracketLeft), ("]", K::BracketRight), ("\\", K::Backslash), ("SEMICOLON", K::Semicolon), ("'", K::Quote),
        (",", K::Comma), (".", K::Period), ("/", K::Slash),
        ("UPARROW", K::ArrowUp), ("DOWNARROW", K::ArrowDown), ("LEFTARROW", K::ArrowLeft), ("RIGHTARROW", K::ArrowRight),
        ("INS", K::Insert), ("DEL", K::Delete), ("HOME", K::Home), ("END", K::End), ("PGUP", K::PageUp), ("PGDN", K::PageDown),
        ("F1", K::F1), ("F2", K::F2), ("F3", K::F3), ("F4", K::F4), ("F5", K::F5), ("F6", K::F6), ("F7", K::F7),
        ("F8", K::F8), ("F9", K::F9), ("F10", K::F10), ("F11", K::F11), ("F12", K::F12),
        ("KP_0", K::Numpad0), ("KP_1", K::Numpad1), ("KP_2", K::Numpad2), ("KP_3", K::Numpad3), ("KP_4", K::Numpad4),
        ("KP_5", K::Numpad5), ("KP_6", K::Numpad6), ("KP_7", K::Numpad7), ("KP_8", K::Numpad8), ("KP_9", K::Numpad9),
    ]
};

/// The game's base FOV (`cl_fov` 70, horizontal at 4:3) that `cl_fovScale` multiplies.
pub const BASE_FOV_DEG: f32 = 70.0;
/// Source's `m_yaw` / `m_pitch`: degrees per mouse count per unit of sensitivity.
pub const M_YAW: f32 = 0.022;

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    pub mouse_sensitivity: f32,
    pub mouse_sensitivity_zoomed: f32,
    pub fov_scale: f32,
    pub invert_y: bool,
    /// Master volume (0-1).
    pub volume: f32,
    pub gamepad_look: usize,
    pub gamepad_look_ads: usize,
    pub gamepad_curve: usize,
    pub gamepad_invert_y: bool,
    pub aim_assist: bool,
    /// The game's `gamepad_button_layout_*.cfg` name ("default", "bumper_jumper", ...).
    pub gamepad_layout: String,
    pub binds: Vec<(Command, Bind)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 5.0,
            mouse_sensitivity_zoomed: 5.0,
            fov_scale: 1.0,
            invert_y: false,
            volume: crate::audio::DEFAULT_VOLUME,
            gamepad_look: 2,
            gamepad_look_ads: 2,
            gamepad_curve: 0,
            gamepad_invert_y: false,
            aim_assist: true,
            gamepad_layout: "default".into(),
            binds: Command::ALL.iter().map(|c| (*c, c.default_bind())).collect(),
        }
    }
}

/// The game's look-curve names (the comments heading `aimcurve_look_N.txt`).
pub const LOOK_CURVES: [&str; 5] = ["CLASSIC", "STEADY", "FINE AIM", "HIGH VELOCITY", "LINEAR"];
/// The game's controller layouts that ship as `gamepad_button_layout_*.cfg`.
pub const PAD_LAYOUTS: [&str; 6] = ["default", "bumper_jumper", "bumper_jumper_alt", "pogo_stick", "button_kicker", "circle"];

impl Settings {
    pub fn bind(&self, c: Command) -> Bind {
        self.binds.iter().find(|(cc, _)| *cc == c).map(|(_, b)| *b).unwrap_or(c.default_bind())
    }

    /// Bind `c` to `b`; whatever else used `b` gets `c`'s old binding (a swap, so no command
    /// is left unbound).
    pub fn rebind(&mut self, c: Command, b: Bind) {
        let old = self.bind(c);
        for (cc, bb) in &mut self.binds {
            if *bb == b && *cc != c {
                *bb = old;
            }
        }
        match self.binds.iter_mut().find(|(cc, _)| *cc == c) {
            Some(e) => e.1 = b,
            None => self.binds.push((c, b)),
        }
    }

    /// Vertical FOV in radians for a window of `aspect` (width / height): the game's FOV is
    /// horizontal at 4:3, so vertical FOV is the same for every window shape.
    pub fn vertical_fov(&self) -> f32 {
        let h = (BASE_FOV_DEG * self.fov_scale).to_radians();
        2.0 * ((h * 0.5).tan() * 0.75).atan()
    }

    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("titanium-dioxide").join("settings.cfg"))
    }

    pub fn to_cfg(&self) -> String {
        let mut s = String::from("// titanium-dioxide settings (Titanfall 2 convar names)\n");
        s += &format!("mouse_sensitivity \"{}\"\n", self.mouse_sensitivity);
        s += &format!("mouse_sensitivity_zoomed \"{}\"\n", self.mouse_sensitivity_zoomed);
        s += &format!("cl_fovScale \"{}\"\n", self.fov_scale);
        s += &format!("m_invert_pitch \"{}\"\n", self.invert_y as u8);
        s += &format!("volume \"{}\"\n", self.volume);
        s += &format!("gamepad_aim_speed \"{}\"\n", self.gamepad_look);
        s += &format!("gamepad_aim_speed_ads \"{}\"\n", self.gamepad_look_ads);
        s += &format!("gamepad_look_curve \"{}\"\n", self.gamepad_curve);
        s += &format!("joy_inverty \"{}\"\n", self.gamepad_invert_y as u8);
        s += &format!("gamepad_aim_assist \"{}\"\n", self.aim_assist as u8);
        s += &format!("gamepad_button_layout \"{}\"\n", self.gamepad_layout);
        for (c, b) in &self.binds {
            s += &format!("bind \"{}\" \"{}\"\n", b.name(), c.cfg());
        }
        s
    }

    /// Read a cfg written by `to_cfg`; anything missing or unreadable keeps its default.
    pub fn from_cfg(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            let toks: Vec<String> = tokens(line);
            if toks.is_empty() {
                continue;
            }
            let f = |i: usize| toks.get(i).and_then(|v| v.parse::<f32>().ok());
            match toks[0].to_ascii_lowercase().as_str() {
                "mouse_sensitivity" => s.mouse_sensitivity = f(1).unwrap_or(s.mouse_sensitivity).clamp(0.0, 20.0),
                "mouse_sensitivity_zoomed" => s.mouse_sensitivity_zoomed = f(1).unwrap_or(s.mouse_sensitivity_zoomed).clamp(0.0, 20.0),
                "cl_fovscale" => s.fov_scale = f(1).unwrap_or(s.fov_scale).clamp(1.0, 1.55),
                "m_invert_pitch" => s.invert_y = f(1).unwrap_or(0.0) != 0.0,
                "volume" => s.volume = f(1).unwrap_or(s.volume).clamp(0.0, 1.0),
                "gamepad_aim_speed" => s.gamepad_look = (f(1).unwrap_or(2.0) as usize).min(7),
                "gamepad_aim_speed_ads" => s.gamepad_look_ads = (f(1).unwrap_or(2.0) as usize).min(7),
                "gamepad_look_curve" => s.gamepad_curve = (f(1).unwrap_or(0.0) as usize).min(4),
                "joy_inverty" => s.gamepad_invert_y = f(1).unwrap_or(0.0) != 0.0,
                "gamepad_aim_assist" => s.aim_assist = f(1).unwrap_or(1.0) != 0.0,
                "gamepad_button_layout" => {
                    if let Some(l) = toks.get(1).filter(|l| PAD_LAYOUTS.contains(&l.as_str())) {
                        s.gamepad_layout = l.clone();
                    }
                }
                "bind" => {
                    if let (Some(b), Some(c)) = (toks.get(1).and_then(|k| Bind::parse(k)), toks.get(2).and_then(|c| Command::from_cfg(c))) {
                        s.rebind(c, b);
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Self::from_cfg(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&p, self.to_cfg()) {
            Ok(()) => log::info!("settings saved to {}", p.display()),
            Err(e) => log::warn!("settings {}: {e}", p.display()),
        }
    }
}

/// Split a cfg line into words, keeping quoted strings together.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                if quoted {
                    out.push(std::mem::take(&mut cur));
                }
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Save whenever the settings change (after the first frame).
pub fn save_settings(settings: Res<Settings>, mut first: Local<bool>) {
    if !*first {
        *first = true;
        return;
    }
    if settings.is_changed() {
        settings.save();
    }
}

/// Apply the FOV setting to the main camera. The cameras compute their FOV each frame against
/// a 60 degree vertical base (with ADS zoom as a ratio of it); this rescales that to the
/// configured FOV while keeping the zoom ratio.
pub fn apply_fov(settings: Res<Settings>, mut cams: Query<&mut Projection, With<crate::player::MainCamera>>, mut last: Local<Option<f32>>) {
    let Ok(mut p) = cams.single_mut() else { return };
    let Projection::Perspective(pp) = p.as_mut() else { return };
    // Already rescaled this value (the camera systems didn't touch it since)?
    if last.is_some_and(|l| (l - pp.fov).abs() < 1e-6) && !settings.is_changed() {
        return;
    }
    let base = 60f32.to_radians();
    let raw = if last.is_some_and(|l| (l - pp.fov).abs() < 1e-6) { pp.fov * base / settings.vertical_fov() } else { pp.fov };
    let ratio = (raw * 0.5).tan() / (base * 0.5).tan();
    pp.fov = 2.0 * ((settings.vertical_fov() * 0.5).tan() * ratio).atan();
    *last = Some(pp.fov);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_round_trip() {
        let mut s = Settings::default();
        s.mouse_sensitivity = 3.4;
        s.fov_scale = 1.275;
        s.invert_y = true;
        s.gamepad_look = 5;
        s.rebind(Command::Jump, Bind::Key(KeyCode::KeyJ));
        s.rebind(Command::Fire, Bind::Mouse(MouseButton::Back));
        let back = Settings::from_cfg(&s.to_cfg());
        assert_eq!(back, s);
    }

    #[test]
    fn rebind_swaps_conflicts() {
        let mut s = Settings::default();
        // W is Forward's; giving it to Jump moves Space (Jump's old key) to Forward.
        s.rebind(Command::Jump, Bind::Key(KeyCode::KeyW));
        assert_eq!(s.bind(Command::Jump), Bind::Key(KeyCode::KeyW));
        assert_eq!(s.bind(Command::Forward), Bind::Key(KeyCode::Space));
    }

    #[test]
    fn default_fov_is_the_games() {
        // 70 degrees horizontal at 4:3 is about 55.4 degrees vertical.
        let v = Settings::default().vertical_fov().to_degrees();
        assert!((v - 55.41).abs() < 0.05, "{v}");
    }
}
