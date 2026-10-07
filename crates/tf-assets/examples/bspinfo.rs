//! Summarise a map: `bspinfo <englishclient_X.bsp.pak000_dir.vpk> <mapname>`
use tf_assets::{bsp::Bsp, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&args[1])?;
    let map = &args[2];
    let t = std::time::Instant::now();
    let data = vpk.read(&format!("maps/{map}.bsp"))?;
    let bsp = Bsp::parse(map, &data, |i| vpk.read(&format!("maps/{map}.bsp.{i:04x}.bsp_lump")).ok())?;
    eprintln!("read in {:?}", t.elapsed());
    for (i, h) in bsp.headers.iter().enumerate() {
        if bsp.lumps[i].len() > 0 { eprint!("{i:02x}:{}(v{}{}) ", bsp.lumps[i].len(), h.version, if h.length == 0 { ",ext" } else { "" }); }
    }
    eprintln!();
    let meshes = bsp.meshes();
    let models = bsp.models();
    let td = bsp.texture_data();
    eprintln!("{} meshes, {} models (world: {:?}), {} texdata, {} sorts", meshes.len(), models.len(), models.first(), td.len(), bsp.material_sorts().len());
    let mut flags = std::collections::BTreeMap::new();
    for m in &meshes { *flags.entry(m.flags).or_insert(0) += 1; }
    eprintln!("mesh flags: {:x?}", flags);
    for t in td.iter().take(15) { eprintln!("  {:?}", t); }
    let sp = bsp.static_props()?;
    eprintln!("{} prop models, {} props; first {:?}", sp.model_names.len(), sp.props.len(), sp.props.first());
    for n in sp.model_names.iter().take(5) { eprintln!("  {n}"); }
    let ents = tf_assets::bsp::parse_entities(&bsp.entities());
    let mut classes = std::collections::BTreeMap::new();
    for e in &ents { if let Some((_, c)) = e.iter().find(|(k, _)| k == "classname") { *classes.entry(c.clone()).or_insert(0) += 1; } }
    eprintln!("{} entities: {:?}", ents.len(), classes);
    Ok(())
}
