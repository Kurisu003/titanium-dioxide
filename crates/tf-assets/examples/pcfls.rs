//! Parse every particle file in a VPK and list systems: `pcfls <frontend dir.vpk> [name filter] [-v]`
//! (`-v` also prints every operator of the matching systems with its parameters)
use tf_assets::{pcf, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let verbose = a.iter().any(|s| s == "-v");
    let filter = a.get(2).filter(|s| *s != "-v").map(|s| s.to_ascii_lowercase());
    let t = std::time::Instant::now();
    let (mut files, mut systems) = (0, 0);
    let mut paths: Vec<&String> = vpk.entries.keys().filter(|p| p.ends_with(".pcf")).collect();
    paths.sort();
    for path in paths {
        let defs = pcf::parse_pcf(&vpk.read(path)?).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        files += 1;
        systems += defs.len();
        for d in defs.iter().filter(|d| filter.as_ref().is_some_and(|f| d.name.to_ascii_lowercase().contains(f.as_str()))) {
            println!(
                "{path}: {} material={:?} max={} emit={:?} init={} ops={} render={:?} children={:?}",
                d.name,
                d.material,
                d.max_particles,
                d.emitters.iter().map(|o| o.function.as_str()).collect::<Vec<_>>(),
                d.initializers.len(),
                d.operators.len(),
                d.renderers.iter().map(|o| o.function.as_str()).collect::<Vec<_>>(),
                d.children.iter().map(|c| (c.name.as_str(), c.delay)).collect::<Vec<_>>()
            );
            if verbose {
                println!("  radius={} color={:?} hdr={} initial={}", d.radius, d.color, d.hdr_scale, d.initial_particles);
                for (kind, ops) in [("emit", &d.emitters), ("init", &d.initializers), ("op", &d.operators), ("render", &d.renderers), ("force", &d.forces), ("constraint", &d.constraints)] {
                    for o in ops {
                        let mut ps: Vec<(&String, &pcf::Value)> = o.params.iter().collect();
                        ps.sort_by(|x, y| x.0.cmp(y.0));
                        let ps: Vec<String> = ps.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
                        println!("  {kind} {}: {}", o.function, ps.join(", "));
                    }
                }
            }
        }
    }
    println!("{files} files, {systems} systems in {:?}", t.elapsed());
    Ok(())
}
