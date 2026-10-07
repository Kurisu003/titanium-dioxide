//! Model-space position of a bone through a sequence (its first animation), every few frames:
//! `seqbone <dir.vpk> <model.mdl> <sequence> <bone> [step]`
use tf_assets::{mdl::Model, vpk::Vpk};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let vpk = Vpk::open(&a[1])?;
    let m = Model::parse(vpk.read(&a[2])?)?;
    let seq = m.sequences.iter().find(|s| s.label.eq_ignore_ascii_case(&a[3])).ok_or_else(|| anyhow::anyhow!("no sequence"))?;
    let bone = m.bone_index(&a[4]).ok_or_else(|| anyhow::anyhow!("no bone"))?;
    let step: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(5);
    let an = m.animation(seq.anims[0])?;
    let mut f = 0;
    loop {
        let p = tf_assets::mdl::screen_cull::pose_frame(&m, &an, f);
        let mv = an.movement.get(f).copied().unwrap_or_default();
        println!("frame {f:3}: {:?} movement {:?}", p[bone].1.map(|x| (x * 10.0).round() / 10.0), mv.map(|x| (x * 10.0).round() / 10.0));
        if f + 1 >= an.num_frames {
            break;
        }
        f = (f + step).min(an.num_frames - 1);
    }
    Ok(())
}
