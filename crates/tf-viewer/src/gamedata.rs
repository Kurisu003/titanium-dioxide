//! Access to the user's Titanfall 2 install: VPKs, RPAKs and name-indexed materials.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tf_assets::{material, rpak::Rpak, vpk::Vpk};

#[derive(bevy::prelude::Resource)]
pub struct GameData {
    pub root: PathBuf,
    pub vpks: Vec<Vpk>,
    /// The open VPKs' paths (background loaders open their own handles).
    pub vpk_paths: Vec<PathBuf>,
    pub paks: Vec<Rpak>,
    /// Lowercased material name (backslash separators) -> (pak, asset index)
    pub materials: HashMap<String, (usize, usize)>,
    /// Lowercased texture name -> (pak, asset index), from the patch paks only (see
    /// `add_rpak_patches`): the fallback for materials that live in the patched pages.
    pub textures: HashMap<String, (usize, usize)>,
}

pub fn normalize_name(s: &str) -> String {
    s.replace('/', "\\").to_ascii_lowercase()
}

impl GameData {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            vpks: Vec::new(),
            vpk_paths: Vec::new(),
            paks: Vec::new(),
            materials: HashMap::new(),
            textures: HashMap::new(),
        }
    }

    /// Another handle on the same VPKs (no paks), for a loader on another thread: `read_file`
    /// finds the same files.
    pub fn vpks_only(&self) -> Self {
        let mut gd = Self::new(&self.root);
        for p in &self.vpk_paths {
            match Vpk::open(p) {
                Ok(v) => {
                    gd.vpks.push(v);
                    gd.vpk_paths.push(p.clone());
                }
                Err(e) => log::warn!("vpk {}: {e:#}", p.display()),
            }
        }
        gd
    }

    /// Open `vpk/englishclient_<name>.bsp.pak000_dir.vpk`.
    pub fn add_vpk(&mut self, name: &str) -> Result<()> {
        let p = self
            .root
            .join("vpk")
            .join(format!("englishclient_{name}.bsp.pak000_dir.vpk"));
        let t = std::time::Instant::now();
        self.vpks.push(Vpk::open(&p)?);
        self.vpk_paths.push(p.clone());
        log::info!("opened {} in {:?}", p.display(), t.elapsed());
        Ok(())
    }

    /// Open `r2/paks/Win64/<file>` for each of `files` and index their materials, then (for
    /// each of `patch_stems`) the patch chain of `<stem>.rpak` (`<stem>(NN).rpak` with a patch
    /// header), newest first, for the assets stored in each patch's own pages (the R-101 SFP's
    /// textures live there). Patch commands on older pages aren't applied, so assets that
    /// point into them are skipped; materials already indexed from the base paks win.
    ///
    /// The paks are decompressed and their names read on every core, then indexed here in
    /// that order, as opening them one after another would.
    pub fn add_rpaks(&mut self, files: &[String], patch_stems: &[&str]) {
        let dir = self.root.join("r2/paks/Win64");
        let t = std::time::Instant::now();
        // (path, patch stem): the base paks, then each stem's patches newest first.
        let mut jobs: Vec<(PathBuf, Option<&str>)> = files
            .iter()
            .map(|f| {
                (
                    tf_assets::rpak::resolve(&dir, f).unwrap_or_else(|| dir.join(f)),
                    None,
                )
            })
            .collect();
        for &stem in patch_stems {
            for n in (1..=99).rev() {
                let p = dir.join(format!("{stem}({n:02}).rpak"));
                let Ok(mut f) = std::fs::File::open(&p) else {
                    continue;
                };
                // A numbered pak with no patch header is self-contained (common_mp(06)); read it too.
                let mut h = [0u8; 0x40];
                if std::io::Read::read_exact(&mut f, &mut h).is_err() {
                    continue;
                }
                jobs.push((p, Some(stem)));
            }
        }
        enum Name {
            Material(String),
            Texture(String),
        }
        let opened = crate::world::parallel_map(
            &jobs,
            |(p, patch)| -> Result<(Rpak, Vec<(usize, Name)>, std::time::Duration)> {
                let t = std::time::Instant::now();
                let pak = Rpak::open(p)?;
                let mut names = Vec::new();
                for (ai, a) in pak.assets.iter().enumerate() {
                    match &a.kind {
                        b"matl" => {
                            if let Ok(m) = material::load(&pak, a) {
                                names.push((ai, Name::Material(normalize_name(&m.name))));
                            }
                        }
                        // Textures by name: the patch paks' (the R-101 SFP's live there) and the
                        // map's own (its overview for the minimap); guids aren't name hashes.
                        b"txtr"
                            if patch.is_some()
                                || p.file_name().is_some_and(|f| {
                                    f.to_string_lossy().starts_with("mp_")
                                        || f.to_string_lossy().starts_with("sp_")
                                }) =>
                        {
                            if let Some(name) =
                                tf_assets::texture::info(&pak, a).ok().and_then(|i| i.name)
                            {
                                names.push((ai, Name::Texture(normalize_name(&name))));
                            }
                        }
                        _ => {}
                    }
                }
                Ok((pak, names, t.elapsed()))
            },
        );
        let mut patches: Vec<(&str, usize, usize, usize)> =
            patch_stems.iter().map(|&s| (s, 0, 0, 0)).collect();
        for ((p, patch), r) in jobs.iter().zip(opened) {
            let r = r.unwrap_or_else(|| Err(anyhow::anyhow!("worker panicked")));
            let idx = self.paks.len();
            match (patch, r) {
                (None, Ok((pak, names, took))) => {
                    let mut n = 0;
                    for (ai, name) in names {
                        match name {
                            Name::Material(name) => {
                                self.materials.entry(name).or_insert((idx, ai));
                                n += 1;
                            }
                            Name::Texture(name) => {
                                self.textures.entry(name).or_insert((idx, ai));
                            }
                        }
                    }
                    log::info!("opened {} ({} materials) in {:?}", p.display(), n, took);
                    self.paks.push(pak);
                }
                (None, Err(e)) => log::warn!("rpak {}: {e:#}", p.display()),
                (Some(stem), Ok((pak, names, _))) => {
                    let Some(counts) = patches.iter_mut().find(|c| c.0 == *stem) else {
                        continue;
                    };
                    for (ai, name) in names {
                        let (map, n) = match name {
                            Name::Material(name) => (self.materials.entry(name), &mut counts.1),
                            Name::Texture(name) => (self.textures.entry(name), &mut counts.2),
                        };
                        if let std::collections::hash_map::Entry::Vacant(e) = map {
                            e.insert((idx, ai));
                            *n += 1;
                        }
                    }
                    counts.3 += 1;
                    self.paks.push(pak);
                }
                (Some(_), Err(_)) => {}
            }
        }
        for (stem, mats, texs, opened) in patches {
            log::info!(
                "{stem} patches: {opened} paks, {mats} new materials, {texs} named textures"
            );
        }
        log::info!("{} rpaks opened in {:?}", self.paks.len(), t.elapsed());
    }

    /// A texture from the patch paks by name: (pak, guid).
    pub fn texture_by_name(&self, name: &str) -> Option<(usize, u64)> {
        let &(pi, ai) = self.textures.get(&normalize_name(name))?;
        Some((pi, self.paks[pi].assets[ai].guid))
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        for v in &self.vpks {
            if v.contains(path) {
                return v.read(path);
            }
        }
        anyhow::bail!("{path} not found in any open VPK")
    }

    pub fn material(&self, name: &str) -> Option<(usize, material::Material)> {
        let &(pi, ai) = self.materials.get(&normalize_name(name))?;
        let pak = &self.paks[pi];
        material::load(pak, &pak.assets[ai]).ok().map(|m| (pi, m))
    }

    /// A shader set's name (its feature flags) from any open pak.
    pub fn shader_set_name(&self, guid: u64) -> Option<String> {
        self.paks
            .iter()
            .find_map(|p| material::shader_set_name(p, guid))
    }

    /// Load a material by GUID from any open pak.
    pub fn material_by_guid(&self, guid: u64) -> Option<material::Material> {
        self.paks
            .iter()
            .find_map(|p| p.asset(guid).and_then(|a| material::load(p, a).ok()))
    }

    pub fn find_texture(&self, pak_hint: usize, guid: u64) -> Option<(usize, usize)> {
        if let Some(&i) = self.paks[pak_hint].by_guid.get(&guid) {
            return Some((pak_hint, i));
        }
        self.paks
            .iter()
            .enumerate()
            .find_map(|(pi, p)| p.by_guid.get(&guid).map(|&i| (pi, i)))
    }

    pub fn read_map(&self, map: &str) -> Result<tf_assets::bsp::Bsp> {
        let path = format!("maps/{map}.bsp");
        let t = std::time::Instant::now();
        let data = self
            .read_file(&path)
            .with_context(|| format!("map {map}"))?;
        let t_main = t.elapsed();
        let bsp = tf_assets::bsp::Bsp::parse(map, &data, |i| {
            let t = std::time::Instant::now();
            let r = self
                .read_file(&format!("maps/{map}.bsp.{i:04x}.bsp_lump"))
                .ok();
            if let Some(d) = &r {
                log::debug!("lump {i:#x}: {} bytes in {:?}", d.len(), t.elapsed());
            }
            r
        });
        log::info!(
            "map {map}: bsp {} bytes read in {t_main:?}, all lumps in {:?}",
            data.len(),
            t.elapsed()
        );
        bsp
    }
}
