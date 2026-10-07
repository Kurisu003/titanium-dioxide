//! Particle effects built from the game's own SpriteCard textures (`materials/particle/...` VTFs
//! in the VPKs): muzzle flashes, tracers, impacts, explosions, smoke, dust and projectile glows.
//!
//! Textures keep their sprite sheets (flipbook sequences) and, for the alpha-only ones, the
//! colour ramp their material names (`$RAMPTEXTURE` with `$texColorFromAlpha`) is baked in.
//! Every live particle of a texture goes into one camera-facing mesh rebuilt each frame, so the
//! whole system costs one draw call per texture. Effects are asked for with [`emit`] (like
//! `audio::cue`), in Bevy space (metres, Y up).

use crate::player::MainCamera;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tf_assets::vtf::{self, Sequence};

/// Gravity in m/s² (750 units/s²).
const GRAVITY: f32 = 750.0 * crate::player::UNIT;
/// Hard cap on live particles; new ones are dropped past it.
const MAX_PARTICLES: usize = 6000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Tex {
    FlashCore,
    FlashCoreBlue,
    FlashCloud,
    FlashCloudBlue,
    Glow,
    Spark,
    Fleks,
    Dirt,
    Smoke,
    FireBall,
    Burst,
    DarkFire,
    Streak,
    Trail,
}

/// (texture, ramp, additive, max mip size)
const TEXTURES: &[(Tex, &str, Option<&str>, bool, u32)] = &[
    (Tex::FlashCore, "particle/muzzleflash/flash_core", Some("particle/ramps/flash_cloud_fire"), true, 512),
    (Tex::FlashCoreBlue, "particle/muzzleflash/flash_core", Some("particle/ramps/flash_cloud_fire_blue"), true, 512),
    (Tex::FlashCloud, "particle/muzzleflash/flash_cloud", Some("particle/ramps/flash_cloud_fire"), true, 256),
    (Tex::FlashCloudBlue, "particle/muzzleflash/flash_cloud", Some("particle/ramps/flash_cloud_fire_blue"), true, 256),
    (Tex::Glow, "particle/glows/glow_pointlight", None, true, 128),
    (Tex::Spark, "particle/sparks/sparks", None, true, 256),
    (Tex::Fleks, "particle/impact/fleks", None, false, 512),
    (Tex::Dirt, "particle/dirt/dirt_burst_full", None, false, 512),
    (Tex::Smoke, "particle/smoke/smoke_puff_01/smoke_puff_01", Some("particle/ramps/smoke_puff_01_ramp"), false, 512),
    (Tex::FireBall, "particle/explosions/exp_fire_ball", Some("particle/ramps/fire_ramp"), false, 1024),
    (Tex::Burst, "particle/explosions/exp_burst", None, true, 1024),
    (Tex::DarkFire, "particle/explosions/exp_fireball_dark", None, false, 1024),
    (Tex::Streak, "particle/glows/glare_spike", None, true, 256),
    (Tex::Trail, "particle/smoke/rocket_trail_soft_edge", None, false, 256),
];

/// What a round struck, as the impact tables' surface keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Level geometry (concrete; the tables' catch-all).
    World,
    /// metal_titan ("E"): a Titan's hull.
    Titan,
    /// flesh ("F"): a Pilot or grunt.
    Flesh,
    /// xo_shield ("X", else "shieldhit"): a Vortex, Particle Wall, A-Wall or Gun Shield.
    Shield,
}

impl Surface {
    /// The table keys to try, most specific first.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Surface::World => &["C", "M"],
            Surface::Titan => &["E", "M", "C"],
            Surface::Flesh => &["F", "C", "M"],
            Surface::Shield => &["X", "shieldhit", "C", "M"],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    // Each variant plays the game's own particle systems when they're loaded (pfx.rs maps them
    // to the real system names and impact tables); the hand-built look below is the fallback.
    /// Muzzle flash at `at` pointing along `dir`; `scale` 1 is a pilot gun, ~3 a Titan's.
    Muzzle { at: Vec3, dir: Vec3, scale: f32, energy: bool },
    /// A tracer streak flying from `from` to `to`; `width` in metres.
    Tracer { from: Vec3, to: Vec3, width: f32, color: Vec3 },
    /// A round hitting something: sparks, a flash, dust and debris (`energy` = blue, no debris).
    Impact { at: Vec3, normal: Vec3, scale: f32, energy: bool },
    /// A weapon's hit played from its impact table (`scripts/impacts/<table>.txt`) for what it
    /// struck; `victim` is a hit on the player, which plays the table's FX_victim entry (the
    /// game's first-person-safe version) when it has one.
    Hit { at: Vec3, normal: Vec3, table: &'static str, surface: Surface, victim: bool, scale: f32 },
    /// Fireball, burst, sparks, debris and lingering smoke; `scale` 1 ≈ a 2 m blast.
    Explosion { at: Vec3, scale: f32 },
    /// A Titan blowing up: a big explosion, secondary blasts and a smoke column.
    TitanDeath { at: Vec3 },
    /// A ring of dust thrown out along the ground (Titanfall landing).
    DustRing { at: Vec3, radius: f32 },
    /// One puff of rocket exhaust smoke.
    TrailPuff { at: Vec3, scale: f32 },
}

#[derive(Component)]
pub struct EffectRequest(Effect);

/// Ask for an effect (Bevy-space positions).
pub fn emit(commands: &mut Commands, e: Effect) {
    commands.spawn(EffectRequest(e));
}

/// A glowing projectile: drawn as a hot head with a streak back along its path. Put it on the
/// projectile's entity (its Transform is the projectile's Bevy-space position).
#[derive(Component, Clone)]
pub struct Glow {
    pub color: Vec3,
    pub size: f32,
    /// Leave a smoke trail (rockets, grenades).
    pub smoke: bool,
    last: Option<Vec3>,
}

impl Glow {
    pub fn new(color: Vec3, size: f32, smoke: bool) -> Self {
        Self { color, size, smoke, last: None }
    }
}

#[derive(Clone, Copy)]
struct Particle {
    tex: Tex,
    pos: Vec3,
    vel: Vec3,
    /// Direction of a stretched particle when it isn't moving.
    axis: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    /// Stretched length (start, end); 0 = camera-facing billboard.
    len: (f32, f32),
    /// Stretched by speed × this many seconds (added to `len`).
    stretch: f32,
    /// Cap the stretched length by the distance travelled so far.
    from_origin: bool,
    rot: f32,
    spin: f32,
    color: Vec3,
    alpha: (f32, f32),
    /// Fraction of life spent fading in.
    fade_in: f32,
    gravity: f32,
    drag: f32,
    seq: u16,
    /// Play the sequence over the particle's life (else its first frame).
    animate: bool,
}

impl Particle {
    fn new(tex: Tex, pos: Vec3, life: f32, size: f32, color: Vec3) -> Self {
        Self {
            tex,
            pos,
            vel: Vec3::ZERO,
            axis: Vec3::Y,
            age: 0.0,
            life,
            size: (size, size),
            len: (0.0, 0.0),
            stretch: 0.0,
            from_origin: false,
            rot: 0.0,
            spin: 0.0,
            color,
            alpha: (1.0, 0.0),
            fade_in: 0.0,
            gravity: 0.0,
            drag: 0.0,
            seq: 0,
            animate: false,
        }
    }
    fn vel(mut self, v: Vec3) -> Self {
        self.vel = v;
        if v.length_squared() > 1e-8 {
            self.axis = v.normalize();
        }
        self
    }
    fn grow(mut self, to: f32) -> Self {
        self.size.1 = to;
        self
    }
    fn alpha(mut self, a: f32, b: f32) -> Self {
        self.alpha = (a, b);
        self
    }
    fn stretched(mut self, axis: Vec3, len: f32, len_end: f32) -> Self {
        self.axis = axis.normalize_or(Vec3::Y);
        self.len = (len.max(1e-3), len_end.max(1e-3));
        self
    }
    fn delay(mut self, d: f32) -> Self {
        self.age = -d;
        self
    }
}

struct Layer {
    tex: Tex,
    additive: bool,
    sheet: Vec<Sequence>,
    mesh: Handle<Mesh>,
    entity: Entity,
}

#[derive(Resource)]
pub struct Particles {
    layers: Vec<Layer>,
    live: Vec<Particle>,
    rng: u64,
}

impl Particles {
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.rand()
    }
    fn unit(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.rand() * 2.0 - 1.0, self.rand() * 2.0 - 1.0, self.rand() * 2.0 - 1.0);
            let l = v.length_squared();
            if l > 1e-4 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }
    /// A random direction within `spread` (0 = along n, 1 = hemisphere) of `n`.
    fn around(&mut self, n: Vec3, spread: f32) -> Vec3 {
        let r = self.unit();
        let r = if r.dot(n) < 0.0 { -r } else { r };
        (n * (1.0 - spread) + r * spread).normalize_or(n)
    }
    fn seqs(&self, tex: Tex) -> u16 {
        self.layers.iter().find(|l| l.tex == tex).map(|l| l.sheet.len().max(1) as u16).unwrap_or(1)
    }
    /// A random single-frame sequence (1..n; sequence 0 is often the whole flipbook).
    fn pick_seq(&mut self, tex: Tex, skip_first: bool) -> u16 {
        let n = self.seqs(tex);
        let lo = if skip_first && n > 1 { 1 } else { 0 };
        lo + ((self.rand() * (n - lo) as f32) as u16).min(n - lo - 1)
    }
    fn push(&mut self, p: Particle) {
        if self.live.len() < MAX_PARTICLES {
            self.live.push(p);
        }
    }
}

/// Load the textures once the game data is up and make one mesh entity per texture.
pub fn load_particles(
    mut commands: Commands,
    gd: Option<Res<crate::gamedata::GameData>>,
    existing: Option<Res<Particles>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(gd), None) = (gd, existing) else { return };
    let t = std::time::Instant::now();
    let read = |name: &str, max: u32| -> Option<vtf::Vtf> {
        let path = format!("materials/{name}.vtf");
        match gd.read_file(&path).map_err(anyhow::Error::from).and_then(|b| vtf::decode(&b, max)) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("particle texture {path}: {e:#}");
                None
            }
        }
    };
    let mut layers = Vec::new();
    for &(tex, path, ramp, additive, max) in TEXTURES {
        let Some(mut v) = read(path, max) else { continue };
        if let Some(r) = ramp.and_then(|r| read(r, 256)) {
            // $texColorFromAlpha: the colour comes from the ramp, indexed by the texture's alpha.
            let row = (r.height / 2) as usize * r.width as usize;
            for px in v.rgba.chunks_exact_mut(4) {
                let a = px[3] as usize;
                let x = (a * (r.width as usize - 1)) / 255;
                let c = &r.rgba[(row + x) * 4..][..3];
                px[..3].copy_from_slice(c);
            }
        }
        let mut img = Image::new(
            Extent3d { width: v.width, height: v.height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            std::mem::take(&mut v.rgba),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = bevy::image::ImageSampler::linear();
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(images.add(img)),
            unlit: true,
            alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
            cull_mode: None,
            double_sided: true,
            fog_enabled: !additive,
            ..default()
        });
        let mesh = meshes.add(empty_mesh());
        let entity = commands
            .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster, NotShadowReceiver))
            .id();
        layers.push(Layer { tex, additive, sheet: v.sheet, mesh, entity });
    }
    log::info!("particles: {} textures in {:?}", layers.len(), t.elapsed());
    commands.insert_resource(Particles { layers, live: Vec::with_capacity(1024), rng: 0x9A27_1C1E });
}

fn empty_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3]);
    m.insert_indices(Indices::U32(vec![0, 1, 2]));
    m
}

/// The hand-built look of an effect (used when the game's particle system can't be played).
pub(crate) fn spawn_effect(p: &mut Particles, e: Effect) {
    match e {
        Effect::Muzzle { at, dir, scale: s, energy } => {
            let dir = dir.normalize_or(Vec3::NEG_Z);
            let (cloud, core) = if energy { (Tex::FlashCloudBlue, Tex::FlashCoreBlue) } else { (Tex::FlashCloud, Tex::FlashCore) };
            let rot = p.range(0.0, std::f32::consts::TAU);
            let mut c = Particle::new(cloud, at + dir * 0.1 * s, 0.06, 0.22 * s, Vec3::splat(3.0)).grow(0.36 * s);
            c.rot = rot;
            p.push(c);
            for k in 0..2 {
                let seq = p.pick_seq(core, true);
                let side = p.unit() * 0.08;
                let mut f = Particle::new(core, at, 0.05, 0.12 * s, Vec3::splat(4.0)).stretched(-(dir + side), 0.6 * s, 0.9 * s).grow(0.18 * s);
                f.seq = seq;
                f.alpha = (1.0, 0.2);
                if k == 1 {
                    f.size = (0.08 * s, 0.12 * s);
                    f.len = (0.35 * s, 0.5 * s);
                }
                p.push(f);
            }
            let glow = if energy { Vec3::new(0.6, 1.2, 3.0) } else { Vec3::new(3.0, 1.6, 0.6) };
            p.push(Particle::new(Tex::Glow, at, 0.05, 0.9 * s, glow).alpha(0.8, 0.0));
            // A thin wisp of gun smoke.
            let up = Vec3::Y * p.range(0.2, 0.5);
            let mut sm = Particle::new(Tex::Smoke, at + dir * 0.2 * s, p.range(0.5, 0.8), 0.08 * s, Vec3::splat(0.8)).grow(0.5 * s).vel(dir * 0.8 * s + up).alpha(0.12 / s.max(1.0).sqrt(), 0.0);
            sm.drag = 3.0;
            sm.seq = p.pick_seq(Tex::Smoke, false);
            sm.rot = p.range(0.0, 6.28);
            sm.spin = p.range(-1.0, 1.0);
            sm.fade_in = 0.15;
            p.push(sm);
        }
        Effect::Tracer { from, to, width, color } => {
            let d = to - from;
            let dist = d.length();
            if dist < 1.0 {
                return;
            }
            let dir = d / dist;
            let speed = 500.0;
            let mut t = Particle::new(Tex::Streak, from, dist / speed, width, color).vel(dir * speed).stretched(dir, width * 50.0, width * 50.0);
            t.from_origin = true;
            t.alpha = (1.0, 0.8);
            p.push(t);
        }
        Effect::Hit { at, normal, table, surface, scale, .. } => {
            let energy = ["accel", "elec", "arc", "plasma", "lstar", "sniper"].iter().any(|k| table.contains(k)) || surface == Surface::Shield;
            spawn_effect(p, Effect::Impact { at, normal, scale, energy });
        }
        Effect::Impact { at, normal, scale: s, energy } => {
            let n = normal.normalize_or(Vec3::Y);
            let at = at + n * 0.03 * s;
            let (glow, spark) = if energy { (Vec3::new(0.8, 1.6, 4.0), Vec3::new(1.5, 3.0, 6.0)) } else { (Vec3::new(3.0, 1.8, 0.8), Vec3::new(6.0, 3.2, 1.2)) };
            p.push(Particle::new(Tex::Glow, at, 0.09, 0.6 * s, glow).grow(0.35 * s));
            let sparks = if energy { 4 } else { 6 };
            for _ in 0..sparks {
                let v = p.around(n, 0.8) * p.range(3.0, 9.0) * s.sqrt();
                let mut sp = Particle::new(Tex::Spark, at, p.range(0.15, 0.35), 0.018 * s, spark).vel(v);
                sp.len = (0.03 * s, 0.03 * s);
                sp.stretch = 0.035;
                sp.from_origin = true;
                sp.gravity = GRAVITY;
                sp.seq = (p.rand() * 2.0) as u16; // the thin streak frames
                sp.alpha = (1.0, 0.3);
                p.push(sp);
            }
            if !energy {
                let mut dust = Particle::new(Tex::Dirt, at, p.range(0.5, 0.8), 0.25 * s, Vec3::new(0.55, 0.5, 0.45)).grow(0.8 * s).vel(n * 1.2 * s).alpha(0.75, 0.0);
                dust.animate = true;
                dust.drag = 2.5;
                dust.rot = p.range(-0.5, 0.5);
                p.push(dust);
                for _ in 0..3 {
                    let v = p.around(n, 0.7) * p.range(2.0, 5.0) * s.sqrt();
                    let mut f = Particle::new(Tex::Fleks, at, p.range(0.5, 0.9), 0.025 * s, Vec3::new(0.4, 0.38, 0.36)).vel(v).alpha(1.0, 1.0);
                    f.gravity = GRAVITY;
                    f.seq = p.pick_seq(Tex::Fleks, false);
                    f.rot = p.range(0.0, 6.28);
                    f.spin = p.range(-12.0, 12.0);
                    p.push(f);
                }
            } else {
                let mut c = Particle::new(Tex::FlashCloudBlue, at, 0.12, 0.3 * s, Vec3::splat(2.0)).grow(0.6 * s).alpha(0.8, 0.0);
                c.rot = p.range(0.0, 6.28);
                p.push(c);
            }
        }
        Effect::Explosion { at, scale: s } => explosion(p, at, s, 0.0),
        Effect::TitanDeath { at } => {
            explosion(p, at, 2.6, 0.0);
            for k in 0..3 {
                let off = Vec3::new(p.range(-1.5, 1.5), p.range(0.0, 2.0), p.range(-1.5, 1.5));
                explosion(p, at + off, 1.2, 0.25 + k as f32 * 0.3);
            }
            // A column of black smoke drifting up for several seconds.
            for k in 0..10 {
                let mut sm = Particle::new(Tex::Smoke, at + Vec3::new(p.range(-1.0, 1.0), 0.0, p.range(-1.0, 1.0)), p.range(5.0, 7.0), 2.0, Vec3::splat(0.12))
                    .grow(7.0)
                    .vel(Vec3::new(p.range(-0.4, 0.4), p.range(1.5, 3.0), p.range(-0.4, 0.4)))
                    .alpha(0.75, 0.0)
                    .delay(0.3 + k as f32 * 0.35);
                sm.seq = p.pick_seq(Tex::Smoke, false);
                sm.rot = p.range(0.0, 6.28);
                sm.spin = p.range(-0.3, 0.3);
                sm.fade_in = 0.1;
                p.push(sm);
            }
        }
        Effect::DustRing { at, radius } => {
            let n = 18;
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU + p.range(-0.1, 0.1);
                let out = Vec3::new(a.cos(), 0.0, a.sin());
                let mut d = Particle::new(Tex::Dirt, at + out * radius * 0.3 + Vec3::Y * 0.3, p.range(1.2, 1.8), 1.0, Vec3::new(0.58, 0.53, 0.47))
                    .grow(3.5)
                    .vel(out * p.range(6.0, 10.0) + Vec3::Y * p.range(0.5, 1.5))
                    .alpha(0.75, 0.0);
                d.drag = 1.8;
                d.animate = true;
                d.rot = p.range(0.0, 6.28);
                p.push(d);
                let mut sm = Particle::new(Tex::Smoke, at + out * radius * 0.5, p.range(2.0, 3.0), 1.2, Vec3::new(0.5, 0.44, 0.36)).grow(4.0).vel(out * p.range(3.0, 5.0) + Vec3::Y * 0.6).alpha(0.45, 0.0);
                sm.drag = 1.2;
                sm.seq = p.pick_seq(Tex::Smoke, false);
                sm.rot = p.range(0.0, 6.28);
                sm.fade_in = 0.1;
                p.push(sm);
            }
            for _ in 0..12 {
                let v = p.around(Vec3::Y, 0.9) * p.range(5.0, 12.0);
                let mut f = Particle::new(Tex::Fleks, at + Vec3::Y * 0.3, p.range(0.8, 1.4), 0.12, Vec3::new(0.4, 0.38, 0.35)).vel(v).alpha(1.0, 1.0);
                f.gravity = GRAVITY;
                f.seq = p.pick_seq(Tex::Fleks, false);
                f.spin = p.range(-8.0, 8.0);
                p.push(f);
            }
        }
        Effect::TrailPuff { at, scale: s } => {
            let mut sm = Particle::new(Tex::Smoke, at, p.range(0.8, 1.2), 0.15 * s, Vec3::splat(0.75)).grow(0.8 * s).alpha(0.4, 0.0).vel(p.unit() * 0.3);
            sm.seq = p.pick_seq(Tex::Smoke, false);
            sm.rot = p.range(0.0, 6.28);
            sm.spin = p.range(-1.0, 1.0);
            sm.fade_in = 0.1;
            p.push(sm);
        }
    }
}

fn explosion(p: &mut Particles, at: Vec3, s: f32, delay: f32) {
    p.push(Particle::new(Tex::Glow, at, 0.18, 2.5 * s, Vec3::new(4.0, 2.4, 1.0)).grow(3.5 * s).delay(delay));
    for _ in 0..3 {
        let off = p.unit() * 0.4 * s;
        let mut f = Particle::new(Tex::FireBall, at + off, p.range(0.6, 0.9), 0.9 * s, Vec3::splat(2.2)).grow(2.0 * s).vel(off * 2.0 + Vec3::Y * 0.6 * s).alpha(1.0, 0.0).delay(delay);
        f.animate = true;
        f.rot = p.range(0.0, 6.28);
        f.spin = p.range(-0.6, 0.6);
        p.push(f);
    }
    let mut b = Particle::new(Tex::Burst, at, 0.45, 1.0 * s, Vec3::splat(2.5)).grow(2.2 * s).alpha(1.0, 0.0).delay(delay);
    b.animate = true;
    b.rot = p.range(0.0, 6.28);
    p.push(b);
    let mut d = Particle::new(Tex::DarkFire, at + Vec3::Y * 0.3 * s, 1.3, 1.2 * s, Vec3::splat(1.0)).grow(2.6 * s).vel(Vec3::Y * 0.8 * s).alpha(0.9, 0.0).delay(delay + 0.05);
    d.animate = true;
    d.rot = p.range(0.0, 6.28);
    p.push(d);
    for _ in 0..(10.0 * s.sqrt()) as usize {
        let v = p.around(Vec3::Y, 0.95) * p.range(8.0, 20.0) * s.sqrt();
        let mut sp = Particle::new(Tex::Spark, at, p.range(0.4, 0.9), 0.04 * s.sqrt(), Vec3::new(6.0, 3.0, 1.0)).vel(v).delay(delay);
        sp.len = (0.05, 0.05);
        sp.stretch = 0.04;
        sp.from_origin = true;
        sp.gravity = GRAVITY;
        sp.seq = (p.rand() * 2.0) as u16; // the thin streak frames
        sp.alpha = (1.0, 0.2);
        p.push(sp);
    }
    for _ in 0..(6.0 * s.sqrt()) as usize {
        let v = p.around(Vec3::Y, 0.9) * p.range(4.0, 10.0) * s.sqrt();
        let mut f = Particle::new(Tex::Fleks, at, p.range(0.8, 1.4), 0.04 * s, Vec3::new(0.3, 0.28, 0.26)).vel(v).alpha(1.0, 1.0).delay(delay);
        f.gravity = GRAVITY;
        f.seq = p.pick_seq(Tex::Fleks, false);
        f.spin = p.range(-10.0, 10.0);
        p.push(f);
    }
    for _ in 0..5 {
        let off = p.unit() * 0.6 * s;
        let mut sm = Particle::new(Tex::Smoke, at + off, p.range(2.5, 3.5), 0.9 * s, Vec3::splat(0.22))
            .grow(3.2 * s)
            .vel(off + Vec3::Y * p.range(0.4, 1.0) * s)
            .alpha(0.7, 0.0)
            .delay(delay + 0.1);
        sm.drag = 0.8;
        sm.seq = p.pick_seq(Tex::Smoke, false);
        sm.rot = p.range(0.0, 6.28);
        sm.spin = p.range(-0.4, 0.4);
        sm.fade_in = 0.15;
        p.push(sm);
    }
}

/// Spawn requested effects, move every particle and rebuild the per-texture meshes.
#[allow(clippy::too_many_arguments)]
pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    particles: Option<ResMut<Particles>>,
    requests: Query<(Entity, &EffectRequest)>,
    mut glows: Query<(&GlobalTransform, &mut Glow)>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut vis: Query<&mut Visibility>,
    mut pfx: Option<ResMut<crate::pfx::Pfx>>,
) {
    let Some(mut p) = particles else {
        for (e, _) in &requests {
            commands.entity(e).despawn();
        }
        return;
    };
    let dt = time.delta_secs().min(0.1);

    // Move and age.
    for q in p.live.iter_mut() {
        q.age += dt;
        if q.age < 0.0 {
            continue;
        }
        q.vel.y -= q.gravity * dt;
        if q.drag > 0.0 {
            q.vel *= (-q.drag * dt).exp();
        }
        q.pos += q.vel * dt;
        q.rot += q.spin * dt;
    }
    p.live.retain(|q| q.age <= q.life);

    for (e, r) in &requests {
        commands.entity(e).despawn();
        // The game's systems take over once the library has loaded (pfx::update_pfx plays it,
        // or falls back to the hand-built look).
        if let Some(x) = pfx.as_mut().filter(|x| x.ready() && std::env::var_os("TF_OLD_FX").is_none()) {
            // First-person muzzle flashes (the Pilot's and the cockpit's, scale < 2.5) keep
            // the hand-built look: the game's `_FP` systems are sized for its viewmodel
            // FOV/near plane and fill the screen here.
            let first_person = matches!(r.0, Effect::Muzzle { scale, .. } if scale < 2.5);
            if !matches!(r.0, Effect::TrailPuff { .. }) && !first_person {
                x.pending.push(r.0);
                continue;
            }
        }
        spawn_effect(&mut *p, r.0);
    }
    for (g, mut glow) in &mut glows {
        let at = g.translation();
        let last = glow.last.replace(at).unwrap_or(at);
        let moved = at - last;
        let head = Particle::new(Tex::Glow, at, 1e-4, glow.size, glow.color).alpha(1.0, 1.0);
        p.push(head);
        if moved.length() > 0.01 {
            let len = moved.length().min(glow.size * 30.0);
            let mut s = Particle::new(Tex::Streak, at, 1e-4, glow.size * 0.5, glow.color).stretched(moved, len, len).alpha(1.0, 1.0);
            s.from_origin = false;
            p.push(s);
        }
        if glow.smoke && moved.length() > 0.01 {
            let mut sm = Particle::new(Tex::Smoke, last, p.range(0.6, 1.0), glow.size * 0.6, Vec3::splat(0.7)).grow(glow.size * 3.0).alpha(0.35, 0.0);
            sm.seq = p.pick_seq(Tex::Smoke, false);
            sm.rot = p.range(0.0, 6.28);
            sm.fade_in = 0.1;
            p.push(sm);
        }
    }

    let Ok(cam) = camera.single() else { return };
    let eye = cam.translation();
    // TF_FX_TEST: cycle the big effects 25 m in front of the camera every 3 s (for checking them).
    if std::env::var_os("TF_FX_TEST").is_some() {
        let (t0, t1) = (time.elapsed_secs() - dt, time.elapsed_secs());
        if (t0 / 3.0) as u32 != (t1 / 3.0) as u32 {
            let ahead = cam.forward().as_vec3();
            let at = eye + Vec3::new(ahead.x, 0.0, ahead.z).normalize_or(Vec3::NEG_Z) * 25.0 - Vec3::Y * 2.0;
            let e = match (t1 / 3.0) as u32 % 4 {
                0 => Effect::TitanDeath { at },
                1 => Effect::Explosion { at, scale: 1.0 },
                2 => Effect::DustRing { at, radius: 220.0 * crate::player::UNIT },
                _ => Effect::Impact { at, normal: Vec3::Y, scale: 3.0, energy: false },
            };
            log::info!("fx test {e:?}");
            spawn_effect(&mut p, e);
        }
    }
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let p = &mut *p;
    for layer in &p.layers {
        let mut list: Vec<&Particle> = p.live.iter().filter(|q| q.tex == layer.tex && q.age >= 0.0).collect();
        if let Ok(mut v) = vis.get_mut(layer.entity) {
            let want = if list.is_empty() { Visibility::Hidden } else { Visibility::Visible };
            if *v != want {
                *v = want;
            }
        }
        if list.is_empty() {
            continue;
        }
        if !layer.additive {
            list.sort_by(|a, b| b.pos.distance_squared(eye).total_cmp(&a.pos.distance_squared(eye)));
        }
        let n = list.len();
        let mut pos = Vec::with_capacity(n * 4);
        let mut uv = Vec::with_capacity(n * 4);
        let mut col = Vec::with_capacity(n * 4);
        let mut idx = Vec::with_capacity(n * 6);
        for q in list {
            let t = (q.age / q.life.max(1e-4)).clamp(0.0, 1.0);
            let size = q.size.0 + (q.size.1 - q.size.0) * t;
            let mut a = q.alpha.0 + (q.alpha.1 - q.alpha.0) * t;
            if q.fade_in > 0.0 {
                a *= (t / q.fade_in).min(1.0);
            }
            if a <= 0.002 {
                continue;
            }
            let rect = match layer.sheet.get(q.seq as usize) {
                Some(s) if q.animate => s.rect_at(t),
                Some(s) => s.rect_at(0.0),
                None => [0.0, 0.0, 1.0, 1.0],
            };
            let c = [q.color.x, q.color.y, q.color.z, a];
            let base = pos.len() as u32;
            if q.len.0 > 0.0 || q.stretch > 0.0 {
                let axis = q.axis;
                let mut len = q.len.0 + (q.len.1 - q.len.0) * t + q.vel.length() * q.stretch;
                if q.from_origin {
                    len = len.min(q.vel.length() * q.age.max(0.0)).max(size * 0.5);
                }
                let to_eye = (eye - q.pos).normalize_or(Vec3::Y);
                // Seen end-on the streak shrinks to a point: fall back to a camera-facing side.
                let c = axis.cross(to_eye);
                let side = if c.length() > 0.05 { c.normalize() } else { right } * size;
                let tail = q.pos - axis * len;
                for v in [q.pos - side, q.pos + side, tail + side, tail - side] {
                    pos.push(v.to_array());
                }
                uv.extend([[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]]);
            } else {
                let (s, co) = q.rot.sin_cos();
                let r = (right * co + up * s) * size;
                let u = (up * co - right * s) * size;
                for v in [q.pos - r + u, q.pos + r + u, q.pos + r - u, q.pos - r - u] {
                    pos.push(v.to_array());
                }
                uv.extend([[rect[0], rect[1]], [rect[2], rect[1]], [rect[2], rect[3]], [rect[0], rect[3]]]);
            }
            col.extend([c; 4]);
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let Some(mesh) = meshes.get_mut(&layer.mesh) else { continue };
        if idx.is_empty() {
            *mesh = empty_mesh();
            continue;
        }
        let normals = vec![(-cam.forward().as_vec3()).to_array(); pos.len()];
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(Indices::U32(idx));
    }
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (load_particles, crate::pfx::start_pfx))
            .add_systems(PostUpdate, (crate::pfx::pfx_test, update_particles, crate::pfx::update_pfx).chain().after(bevy::transform::TransformSystems::Propagate));
    }
}
