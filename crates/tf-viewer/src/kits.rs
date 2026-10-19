//! Kits (the game's passives), as its loadout menu offers them: each Titan's own kit, a Titan
//! kit and a Titanfall kit for every Titan, and the Pilot's two kits. The choices come from
//! `datatable/titan_passives.rpak` and `datatable/pilot_passives.rpak` (passive, type, name,
//! description, image), with the later Titan kits that table lacks; they are picked on the
//! LOADOUT page and saved with the settings. Nothing is fitted unless picked.
//!
//! Most kits are weapon mods of the same name in the scripts they change
//! (`Mods { pas_scorch_weapon { ... } }` in the T-203's script, `pas_power_cell` in every
//! tactical's): `fitted()` lists the ones fitted, and every script load applies them
//! (weapons.rs `env_mods`, titankit.rs `KitNumbers`, pilotability.rs). The rest are rules in
//! the code that ask `has()`.

use bevy::prelude::*;
use std::sync::RwLock;

/// Which loadout slot a kit fills.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KitSlot {
    /// TITAN_<NAME>_PASSIVE: the chassis's own (one per Titan).
    TitanOwn,
    /// TITAN_GENERAL_PASSIVE.
    Titan,
    /// TITAN_TITANFALL_PASSIVE.
    Titanfall,
    /// PILOT_PASSIVE1 / PILOT_PASSIVE2.
    Pilot1,
    Pilot2,
}

#[derive(Clone, Debug)]
pub struct Kit {
    pub id: String,
    pub slot: KitSlot,
    /// For TitanOwn: the Titan's kit name as titan_properties spells it (ION, SCORCH, ...).
    pub titan: Option<String>,
    /// Localisation keys (without '#') and the menu image.
    pub name: String,
    pub desc: String,
    pub image: String,
}

/// Later Titan kits the shipped `titan_passives` table doesn't list (the game added them in
/// updates): (passive, Titan, name key, image).
const LATER_KITS: &[(&str, &str, &str, &str)] = &[
    (
        "pas_ion_weapon_ads",
        "ION",
        "GEAR_ION_SPLIT",
        "rui/titan_loadout/passive/ion_refraction_lens",
    ),
    (
        "pas_scorch_flamecore",
        "SCORCH",
        "GEAR_SCORCH_FLAMECORE",
        "rui/titan_loadout/passive/scorch_scorched_earth",
    ),
    (
        "pas_northstar_optics",
        "NORTHSTAR",
        "GEAR_NORTHSTAR_OPTICS",
        "rui/titan_loadout/passive/northstar_threat_optics",
    ),
    (
        "pas_ronin_autoshift",
        "RONIN",
        "GEAR_RONIN_AUTOSHIFT",
        "rui/titan_loadout/passive/ronin_auto_shift",
    ),
    (
        "pas_tone_burst",
        "TONE",
        "GEAR_TONE_BURST",
        "rui/titan_loadout/passive/tone_burst",
    ),
    (
        "pas_legion_chargeshot",
        "LEGION",
        "GEAR_LEGION_CHARGESHOT",
        "rui/titan_loadout/passive/legion_charge_shot",
    ),
];

#[derive(Resource, Default, Clone)]
pub struct KitTable(pub Vec<Kit>);

impl KitTable {
    pub fn load(gd: &crate::gamedata::GameData) -> Self {
        let mut out = Vec::new();
        for (table, pilot) in [
            ("datatable/titan_passives.rpak", false),
            ("datatable/pilot_passives.rpak", true),
        ] {
            let Some(t) = gd
                .paks
                .iter()
                .find_map(|p| tf_assets::datatable::DataTable::load(p, table))
            else {
                log::warn!("kits: no {table}");
                continue;
            };
            let col = |n: &str| t.columns.iter().position(|c| c == n);
            let (Some(pc), Some(tc), Some(nc), Some(dc), Some(ic)) = (
                col("passive"),
                col("type"),
                col("name"),
                col("description"),
                col("image"),
            ) else {
                continue;
            };
            let hidden = col("hidden");
            for r in &t.rows {
                if hidden.is_some_and(|h| r[h].text() == "true") {
                    continue;
                }
                let kind = r[tc].text();
                let (slot, titan) = match kind.as_str() {
                    "PILOT_PASSIVE1" => (KitSlot::Pilot1, None),
                    "PILOT_PASSIVE2" => (KitSlot::Pilot2, None),
                    "TITAN_GENERAL_PASSIVE" => (KitSlot::Titan, None),
                    "TITAN_TITANFALL_PASSIVE" => (KitSlot::Titanfall, None),
                    k => match k
                        .strip_prefix("TITAN_")
                        .and_then(|k| k.strip_suffix("_PASSIVE"))
                    {
                        Some(name) if !pilot => (KitSlot::TitanOwn, Some(name.to_string())),
                        _ => continue,
                    },
                };
                out.push(Kit {
                    id: r[pc].text(),
                    slot,
                    titan,
                    name: r[nc].text().trim_start_matches('#').to_string(),
                    desc: r[dc].text().trim_start_matches('#').to_string(),
                    image: r[ic].text().replace('\\', "/"),
                });
            }
        }
        for (id, titan, name, image) in LATER_KITS {
            if !out.iter().any(|k| k.id == *id) {
                out.push(Kit {
                    id: id.to_string(),
                    slot: KitSlot::TitanOwn,
                    titan: Some(titan.to_string()),
                    name: name.to_string(),
                    desc: format!("{name}_DESC"),
                    image: image.to_string(),
                });
            }
        }
        log::info!("kits: {} in the tables", out.len());
        Self(out)
    }

    /// The kits a slot offers (for TitanOwn, the given Titan's).
    pub fn options(&self, slot: KitSlot, titan: &str) -> Vec<&Kit> {
        self.0
            .iter()
            .filter(|k| k.slot == slot)
            .filter(|k| {
                slot != KitSlot::TitanOwn
                    || k.titan
                        .as_deref()
                        .is_some_and(|t| t.eq_ignore_ascii_case(titan))
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&Kit> {
        self.0.iter().find(|k| k.id == id)
    }
}

/// The kits fitted now: the Pilot's two, the Titan kit, the Titanfall kit and the current
/// Titan's own kit (kept in step with the settings and the Titan picked by `sync_fitted`).
static FITTED: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub fn fitted() -> Vec<String> {
    FITTED.read().map(|f| f.clone()).unwrap_or_default()
}

/// Whether a kit is fitted.
pub fn has(id: &str) -> bool {
    FITTED
        .read()
        .is_ok_and(|f| f.iter().any(|k| k.eq_ignore_ascii_case(id)))
}

/// Bumped whenever the fitted kits change, so loaded weapons know to read their script again.
pub static GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn generation() -> u32 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// A script or .set file with the fitted kits' mods (`Mods`) and class mods (`ClassMods`)
/// applied; one without those mods is unchanged.
pub fn apply(s: tf_assets::settings::PlayerSettings) -> tf_assets::settings::PlayerSettings {
    let f = fitted();
    if f.is_empty() {
        return s;
    }
    let refs: Vec<&str> = f.iter().map(String::as_str).collect();
    s.with_mods(&refs).with_class_mods(&refs)
}

/// The kits fitted with these settings and Titan (TITAN_ARSENAL index).
pub fn fitted_for(settings: &crate::settings::Settings, titan: usize) -> Vec<String> {
    let chassis = crate::weapons::TITAN_ARSENAL
        .get(titan)
        .map(|g| g.kit)
        .unwrap_or("");
    [
        settings.pilot_kit1.as_str(),
        settings.pilot_kit2.as_str(),
        settings.titan_kit.as_str(),
        settings.titanfall_kit.as_str(),
        settings.titan_own_kit(chassis),
    ]
    .into_iter()
    .filter(|k| !k.is_empty())
    .map(str::to_string)
    .collect()
}

/// Set the fitted kits (at start, before anything reads a script).
pub fn set_fitted(v: Vec<String>) {
    if let Ok(mut f) = FITTED.write() {
        if *f != v {
            log::info!("kits fitted: {v:?}");
            *f = v;
            GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Keep the fitted kits in step with the menu. A change reads the scripts again with the new
/// mods: the Titan's kit numbers (and its kit applied afresh), the Pilot's abilities, and the
/// Titan's gun (`switch_titan_weapon` sees the new generation).
pub fn sync_fitted(
    mut commands: Commands,
    settings: Res<crate::settings::Settings>,
    titan: Res<crate::weapons::TitanKit>,
    gd: Res<crate::gamedata::GameData>,
    mut active: ResMut<crate::titankit::ActiveKit>,
    status: Option<ResMut<crate::pilotability::PilotStatus>>,
    table: Option<Res<KitTable>>,
    args: Res<crate::Args>,
    titan_settings: Option<Res<crate::player::TitanSettings>>,
) {
    if table.is_none() {
        commands.insert_resource(KitTable::load(&gd));
    }
    let want = fitted_for(&settings, titan.0);
    if want == fitted() {
        return;
    }
    set_fitted(want);
    commands.remove_resource::<crate::titankit::KitDefs>();
    active.index = usize::MAX;
    let mut read = |p: &str| {
        gd.read_file(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    let defs = crate::pilotability::PilotAbilityDefs::load(&mut read);
    if let Some(mut status) = status {
        status.refill(&defs);
    }
    commands.insert_resource(defs);
    // Movement: the Pilot's (hover, wall hang) and the Titan's (Turbo Engine's dash).
    commands.insert_resource(crate::pilotctl::PilotSettings(crate::pilot_params(&gd)));
    if let Some(old) = titan_settings {
        let (path, sp) = crate::player_titan_settings(&args.map, args.titan);
        let params = crate::titan_params_for(&gd, path, sp, true);
        if params.dash_drain != old.0.dash_drain {
            crate::combat::set_dash_drain(params.dash_drain);
        }
        commands.insert_resource(crate::player::TitanSettings(params));
    }
}

/// The Titan (kit name, ION, SCORCH, ...) of a TITAN_ARSENAL index.
pub fn chassis(titan: usize) -> &'static str {
    crate::weapons::TITAN_ARSENAL
        .get(titan)
        .map_or("", |g| g.kit)
}

/// The kit picked for a slot ("" for none).
pub fn picked(settings: &crate::settings::Settings, slot: KitSlot, titan: usize) -> String {
    match slot {
        KitSlot::TitanOwn => settings.titan_own_kit(chassis(titan)).to_string(),
        KitSlot::Titan => settings.titan_kit.clone(),
        KitSlot::Titanfall => settings.titanfall_kit.clone(),
        KitSlot::Pilot1 => settings.pilot_kit1.clone(),
        KitSlot::Pilot2 => settings.pilot_kit2.clone(),
    }
}

/// Step a slot through none and the kits it offers.
pub fn cycle(
    table: &KitTable,
    settings: &mut crate::settings::Settings,
    slot: KitSlot,
    titan: usize,
    change: i32,
) {
    let chassis = chassis(titan);
    let mut ids: Vec<&str> = vec![""];
    ids.extend(table.options(slot, chassis).iter().map(|k| k.id.as_str()));
    let cur = picked(settings, slot, titan);
    let i = ids.iter().position(|k| *k == cur).unwrap_or(0) as i32;
    let next = ids[(i + change).rem_euclid(ids.len() as i32) as usize].to_string();
    match slot {
        KitSlot::TitanOwn => settings.set_titan_own_kit(chassis, &next),
        KitSlot::Titan => settings.titan_kit = next,
        KitSlot::Titanfall => settings.titanfall_kit = next,
        KitSlot::Pilot1 => settings.pilot_kit1 = next,
        KitSlot::Pilot2 => settings.pilot_kit2 = next,
    }
}

/// The menu's name for a slot's kit, and its image.
pub fn label(
    table: Option<&KitTable>,
    strings: &crate::pilotweapon::Strings,
    settings: &crate::settings::Settings,
    slot: KitSlot,
    titan: usize,
) -> (String, Option<String>) {
    let id = picked(settings, slot, titan);
    match table.and_then(|t| t.get(&id)) {
        Some(k) => (
            strings
                .0
                .get(&k.name)
                .cloned()
                .unwrap_or_else(|| k.id.trim_start_matches("pas_").to_string())
                .to_uppercase(),
            Some(k.image.clone()),
        ),
        None if id.is_empty() => ("NONE".into(), None),
        None => (id.to_uppercase(), None),
    }
}
