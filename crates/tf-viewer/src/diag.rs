//! TF_DIAG=1: every 30 seconds, log the entity count, the average frame time and the
//! components most entities carry, to find what piles up over a long session.

use bevy::prelude::*;

const EVERY: f32 = 30.0;

#[derive(Default)]
pub struct Diag {
    next: f32,
    frames: u32,
    secs: f32,
}

pub fn diag(world: &mut World, mut d: Local<Diag>) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("TF_DIAG").is_some()) {
        return;
    }
    let dt = world.resource::<Time<Real>>().delta_secs();
    d.frames += 1;
    d.secs += dt;
    d.next -= dt;
    if d.next > 0.0 {
        return;
    }
    d.next = EVERY;
    let ms = d.secs * 1000.0 / d.frames.max(1) as f32;
    d.frames = 0;
    d.secs = 0.0;
    let total: usize = world.archetypes().iter().map(|a| a.len() as usize).sum();
    macro_rules! count {
        ($($t:ty),*) => {
            vec![$((
                stringify!($t).rsplit("::").next().unwrap_or(""),
                world.query_filtered::<(), With<$t>>().iter(world).count(),
            )),*]
        };
    }
    let top: Vec<(&str, usize)> = count!(
        Mesh3d,
        Children,
        crate::pfx::PfxTrail,
        crate::pfx::PfxOneShot,
        crate::pfx::FxModelInstance,
        crate::particles::Glow,
        crate::weapons::Fx,
        crate::pilotweapon::PilotBolt,
        crate::targets::Bolt,
        crate::targets::Enemy,
        crate::titankit::Wave,
        crate::titankit::Thermite,
        crate::titankit::ParticleWall,
        AudioPlayer,
        Node,
        PointLight
    );
    // Every component by how many entities carry it (named in a build with bevy/debug).
    let mut per: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for a in world.archetypes().iter() {
        let n = a.len() as usize;
        if n == 0 {
            continue;
        }
        for c in a.components() {
            if let Some(info) = world.components().get_info(*c) {
                *per.entry(format!("{}", info.name())).or_default() += n;
            }
        }
    }
    let mut all: Vec<(String, usize)> = per.into_iter().collect();
    all.sort_by(|a, b| b.1.cmp(&a.1));
    all.truncate(40);
    log::info!("diag: components {all:?}");
    let net = world.resource::<crate::net::Net>();
    log::info!(
        "diag: {} entities, {ms:.1} ms/frame, ghost rounds {}, top {:?}",
        total,
        net.shots.len(),
        top
    );
}
