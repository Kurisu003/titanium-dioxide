//! Inspect a model: `mdlinfo <dir.vpk> <models/x.mdl> [anim-sequence-filter]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&args[1])?;
    let m = Model::parse(vpk.read(&args[2])?)?;
    println!("{} bones={} textures={} bodyparts={:?}", m.name, m.bones.len(), m.textures.len(), m.bodyparts.iter().map(|b| (&b.name, b.num_models)).collect::<Vec<_>>());
    println!("hull {:?} skins {}x{}", m.hull, m.skin_families.len(), m.skin_families.first().map(|f| f.len()).unwrap_or(0));
    for (i, b) in m.bones.iter().take(if std::env::var_os("ALL_BONES").is_some() { usize::MAX } else { 6 }).enumerate() { println!(" bone {i} {} parent {} pos {:?} q {:?} p2b {:?}", b.name, b.parent, b.pos, b.quat, b.pose_to_bone); }
    let t = std::time::Instant::now();
    // BODY="0,0,0,0,0,3": body selection per body part.
    let body: Vec<usize> = std::env::var("BODY").map(|b| b.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let meshes = m.meshes(&body)?;
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let verts: usize = meshes.iter().map(|m| m.positions.len()).sum();
    println!("{} meshes, {verts} verts, {tris} tris in {:?}", meshes.len(), t.elapsed());
    for md in &meshes {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &md.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        let mut bones: Vec<u16> = md.joints.iter().zip(&md.weights).flat_map(|(j, w)| (0..4).filter(move |&i| w[i] > 0.0).map(move |i| j[i])).collect();
        bones.sort(); bones.dedup();
        let names: Vec<&str> = bones.iter().take(6).map(|&b| m.bones.get(b as usize).map(|x| x.name.as_str()).unwrap_or("?")).collect();
        println!("  mat {} -> {} bp {} verts {} tris {} bounds {:?}..{:?} bones {:?}", md.material, m.textures[m.skin_families[0][md.material] as usize], md.bodypart, md.positions.len(), md.indices.len()/3, lo.map(|x| x.round()), hi.map(|x| x.round()), names);
    }
    let mut lo = [f32::MAX; 3]; let mut hi = [f32::MIN; 3];
    for md in &meshes { for p in &md.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } } }
    println!("bounds {lo:?} {hi:?}");
    let filter = args.get(3).cloned();
    for inc in &m.include_models {
        let Ok(data) = vpk.read(inc) else { println!("include {inc} MISSING"); continue };
        let im = Model::parse(data)?;
        println!("include {inc}: {} anims, {} seqs, {} bones", im.num_anims, im.sequences.len(), im.bones.len());
        if let Some(f) = &filter {
            for s in im.sequences.iter().filter(|s| s.label.contains(f.as_str())) {
                println!("   seq {} act={} grid={:?} anims={:?}", s.label, s.activity, s.group_size, s.anims);
                if let Some(&a) = s.anims.first() { let t = std::time::Instant::now(); let an = im.animation(a)?; println!("      anim {} fps {} frames {} flags {:#x} ({:?}) root0 {:?}", an.name, an.fps, an.num_frames, an.flags, t.elapsed(), an.frames[0][0]); }
            }
        }
    }
    Ok(())
}
