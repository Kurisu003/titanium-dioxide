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
        Self { root: root.as_ref().to_path_buf(), vpks: Vec::new(), vpk_paths: Vec::new(), paks: Vec::new(), materials: HashMap::new(), textures: HashMap::new() }
    }

    /// Open `vpk/englishclient_<name>.bsp.pak000_dir.vpk`.
    pub fn add_vpk(&mut self, name: &str) -> Result<()> {
        let p = self.root.join("vpk").join(format!("englishclient_{name}.bsp.pak000_dir.vpk"));
        let t = std::time::Instant::now();
        self.vpks.push(Vpk::open(&p)?);
        self.vpk_paths.push(p.clone());
        log::info!("opened {} in {:?}", p.display(), t.elapsed());
        Ok(())
    }

    /// Open `r2/paks/Win64/<file>` and index its materials.
    pub fn add_rpak(&mut self, file: &str) -> Result<()> {
        let dir = self.root.join("r2/paks/Win64");
        let p = tf_assets::rpak::resolve(&dir, file).unwrap_or_else(|| dir.join(file));
        let t = std::time::Instant::now();
        let pak = Rpak::open(&p)?;
        let idx = self.paks.len();
        let mut n = 0;
        for (ai, a) in pak.assets.iter().enumerate() {
            if &a.kind != b"matl" {
                continue;
            }
            if let Ok(m) = material::load(&pak, a) {
                self.materials.entry(normalize_name(&m.name)).or_insert((idx, ai));
                n += 1;
            }
        }
        log::info!("opened {} ({} materials) in {:?}", p.display(), n, t.elapsed());
        self.paks.push(pak);
        Ok(())
    }

    /// Open the patch chain of `<stem>.rpak` (`<stem>(NN).rpak` with a patch header), newest
    /// first, for the assets stored in each patch's own pages (the R-101 SFP's textures live
    /// there). Patch commands on older pages aren't applied, so assets that point into them are
    /// skipped; materials already indexed from the base pak win.
    pub fn add_rpak_patches(&mut self, stem: &str) {
        let dir = self.root.join("r2/paks/Win64");
        let t = std::time::Instant::now();
        let (mut mats, mut texs, mut opened) = (0, 0, 0);
        for n in (1..=99).rev() {
            let p = dir.join(format!("{stem}({n:02}).rpak"));
            let Ok(mut f) = std::fs::File::open(&p) else { continue };
            // A numbered pak with no patch header is self-contained (common_mp(06)); read it too.
            let mut h = [0u8; 0x40];
            if std::io::Read::read_exact(&mut f, &mut h).is_err() {
                continue;
            }
            let Ok(pak) = Rpak::open(&p) else { continue };
            let idx = self.paks.len();
            for (ai, a) in pak.assets.iter().enumerate() {
                match &a.kind {
                    b"matl" => {
                        if let Ok(m) = material::load(&pak, a) {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.materials.entry(normalize_name(&m.name)) {
                                e.insert((idx, ai));
                                mats += 1;
                            }
                        }
                    }
                    b"txtr" => {
                        if let Some(name) = tf_assets::texture::info(&pak, a).ok().and_then(|i| i.name) {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.textures.entry(normalize_name(&name)) {
                                e.insert((idx, ai));
                                texs += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.paks.push(pak);
            opened += 1;
        }
        log::info!("{stem} patches: {opened} paks, {mats} new materials, {texs} named textures in {:?}", t.elapsed());
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
        self.paks.iter().find_map(|p| material::shader_set_name(p, guid))
    }

    /// Load a material by GUID from any open pak.
    pub fn material_by_guid(&self, guid: u64) -> Option<material::Material> {
        self.paks.iter().find_map(|p| p.asset(guid).and_then(|a| material::load(p, a).ok()))
    }

    pub fn find_texture(&self, pak_hint: usize, guid: u64) -> Option<(usize, usize)> {
        if let Some(&i) = self.paks[pak_hint].by_guid.get(&guid) {
            return Some((pak_hint, i));
        }
        self.paks.iter().enumerate().find_map(|(pi, p)| p.by_guid.get(&guid).map(|&i| (pi, i)))
    }

    pub fn read_map(&self, map: &str) -> Result<tf_assets::bsp::Bsp> {
        let path = format!("maps/{map}.bsp");
        let t = std::time::Instant::now();
        let data = self.read_file(&path).with_context(|| format!("map {map}"))?;
        let t_main = t.elapsed();
        let bsp = tf_assets::bsp::Bsp::parse(map, &data, |i| {
            let t = std::time::Instant::now();
            let r = self.read_file(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok();
            if let Some(d) = &r {
                log::debug!("lump {i:#x}: {} bytes in {:?}", d.len(), t.elapsed());
            }
            r
        });
        log::info!("map {map}: bsp {} bytes read in {t_main:?}, all lumps in {:?}", data.len(), t.elapsed());
        bsp
    }
}
