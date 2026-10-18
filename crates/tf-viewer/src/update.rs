//! Auto-update: every few minutes the game asks GitHub whether `main` has moved on. If it has
//! (and this checkout is on `main` with nothing of its own on top), it fast-forwards, rebuilds
//! the release binary in the background and restarts on it with the same settings, hosting
//! or joining the same session again. Outside a match the restart is immediate; in a match it
//! waits until the player is back in the menus.
//!
//! Off for scripted test runs (`--script`, `--shots`) and with `TF_NO_UPDATE=1`. A checkout on
//! another branch (a development tree) only reports that it is not following `main`.

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Seconds between checks (the first comes a few seconds after start).
const CHECK_EVERY: f32 = 300.0;
const FIRST_CHECK: f32 = 5.0;

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Idle,
    Checking,
    /// Pulled this commit, building it.
    Building(String),
    /// Built: restart when not in a match.
    Ready(String),
    Failed(String),
}

#[derive(Resource)]
pub struct Updater {
    phase: Arc<Mutex<Phase>>,
    /// The commit whose pull or build failed (not retried until `main` moves again).
    failed: Arc<Mutex<Option<String>>>,
    next: f32,
    repo: PathBuf,
    restarting: bool,
}

impl Default for Updater {
    fn default() -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        Self {
            phase: Arc::new(Mutex::new(Phase::Idle)),
            failed: Arc::new(Mutex::new(None)),
            next: FIRST_CHECK,
            repo: repo.canonicalize().unwrap_or(repo),
            restarting: false,
        }
    }
}

/// The on-screen update notice.
#[derive(Component)]
pub struct UpdateText;

/// `git -C repo args...`: its trimmed output when it succeeded.
fn git(repo: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Cargo on the PATH, else rustup's default place (a game started from a desktop entry may
/// not have ~/.cargo/bin on its PATH).
fn cargo() -> PathBuf {
    let on_path = Command::new("cargo")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if on_path {
        return "cargo".into();
    }
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")))
        .map(|c| c.join("bin/cargo"))
        .unwrap_or_else(|| "cargo".into())
}

/// One check (a background thread): fetch, fast-forward, build.
fn check(repo: PathBuf, phase: Arc<Mutex<Phase>>, failed: Arc<Mutex<Option<String>>>) {
    let set = |p: Phase| {
        if let Ok(mut s) = phase.lock() {
            *s = p;
        }
    };
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if branch != "main" {
        log::info!("update: this checkout is on '{branch}', only main updates itself");
        set(Phase::Idle);
        return;
    }
    if git(&repo, &["fetch", "--quiet", "origin", "main"]).is_none() {
        log::warn!("update: could not reach GitHub (git fetch failed)");
        set(Phase::Idle);
        return;
    }
    let (Some(head), Some(remote)) = (
        git(&repo, &["rev-parse", "HEAD"]),
        git(&repo, &["rev-parse", "FETCH_HEAD"]),
    ) else {
        set(Phase::Idle);
        return;
    };
    if head == remote || failed.lock().ok().and_then(|f| f.clone()) == Some(remote.clone()) {
        set(Phase::Idle);
        return;
    }
    if git(
        &repo,
        &["merge-base", "--is-ancestor", "HEAD", "FETCH_HEAD"],
    )
    .is_none()
    {
        log::warn!("update: local main has commits that are not on GitHub, not updating");
        set(Phase::Idle);
        return;
    }
    let short: String = remote.chars().take(7).collect();
    log::info!("update: new version {short} on GitHub, pulling and building");
    set(Phase::Building(short.clone()));
    let fail = |why: &str| {
        log::warn!("update: {why}");
        if let Ok(mut f) = failed.lock() {
            *f = Some(remote.clone());
        }
        set(Phase::Failed(why.to_string()));
    };
    if git(&repo, &["merge", "--ff-only", "--quiet", "FETCH_HEAD"]).is_none() {
        fail("git pull failed (local changes in the way?)");
        return;
    }
    let log_path = repo.join("target/update-build.log");
    let _ = std::fs::create_dir_all(repo.join("target"));
    let out = std::fs::File::create(&log_path).ok();
    let mut cmd = Command::new(cargo());
    cmd.current_dir(&repo)
        .args(["build", "--release", "-p", "tf-viewer"])
        .stdin(Stdio::null());
    if let Some(f) = out {
        if let Ok(f2) = f.try_clone() {
            cmd.stdout(f).stderr(f2);
        }
    }
    match cmd.status() {
        Ok(s) if s.success() => {
            log::info!("update: built {short}");
            set(Phase::Ready(short));
        }
        _ => fail("build failed, see target/update-build.log"),
    }
}

/// Start the rebuilt game with this one's settings and the same session, then quit.
fn restart(repo: &Path, net: &crate::net::Net) -> bool {
    let exe = repo
        .join("target/release")
        .join(format!("tf-viewer{}", std::env::consts::EXE_SUFFIX));
    // The command line minus the session flags, which follow the live session instead.
    let mut args: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" | "--join" => {
                it.next();
            }
            "--pvp" => {}
            _ => args.push(a),
        }
    }
    if net.is_host() {
        args.push("--host".into());
        args.push(net.port_num().to_string());
        if net.pvp() {
            args.push("--pvp".into());
        }
    } else if let Some(addr) = net.host_address() {
        args.push("--join".into());
        args.push(addr);
    }
    let r = Command::new(&exe)
        .args(&args)
        .env("TF_NAME", &net.name)
        .spawn();
    log::info!(
        "update: restarting {} {args:?}: {:?}",
        exe.display(),
        r.as_ref().map(|c| c.id())
    );
    r.is_ok()
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time<Real>>,
    args: Res<crate::Args>,
    mut up: ResMut<Updater>,
    game: Res<crate::game::Game>,
    net: Res<crate::net::Net>,
    ui: Option<Res<crate::ui::UiAssets>>,
    mut texts: Query<&mut Text, With<UpdateText>>,
    mut exit: MessageWriter<AppExit>,
) {
    if args.script.is_some() || !args.shots.is_empty() || std::env::var_os("TF_NO_UPDATE").is_some()
    {
        return;
    }
    let phase = up.phase.lock().map(|p| p.clone()).unwrap_or(Phase::Idle);
    up.next -= time.delta_secs();
    if up.next <= 0.0 && matches!(phase, Phase::Idle | Phase::Failed(_)) {
        up.next = CHECK_EVERY;
        if let Ok(mut p) = up.phase.lock() {
            *p = Phase::Checking;
        }
        let (repo, ph, failed) = (up.repo.clone(), up.phase.clone(), up.failed.clone());
        std::thread::spawn(move || check(repo, ph, failed));
    }
    let in_match = game.in_play();
    if let Phase::Ready(_) = phase {
        if !in_match && !up.restarting {
            up.restarting = true;
            if restart(&up.repo, &net) {
                exit.write(AppExit::Success);
            }
        }
    }
    // The notice, bottom right.
    let msg = match &phase {
        Phase::Building(v) => format!("NEW VERSION {v}: UPDATING..."),
        Phase::Ready(v) if in_match => {
            format!("VERSION {v} READY: RESTARTS WHEN YOU LEAVE THE MATCH")
        }
        Phase::Ready(v) => format!("RESTARTING ON VERSION {v}..."),
        Phase::Failed(why) => format!("UPDATE FAILED: {}", why.to_uppercase()),
        _ => String::new(),
    };
    if texts.is_empty() {
        if let Some(ui) = ui {
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Vh(1.5),
                    bottom: Val::Vh(1.5),
                    ..default()
                },
                Text::new(msg),
                TextFont {
                    font: ui.bold_font.clone(),
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.85, 0.4)),
                crate::ui::VhText(1.6),
                GlobalZIndex(100),
                UpdateText,
            ));
        }
        return;
    }
    for mut t in &mut texts {
        if t.0 != msg {
            t.0 = msg.clone();
        }
    }
}
