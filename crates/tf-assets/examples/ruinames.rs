//! List every `rui/...` string in an rpak's data (image paths of UI atlases): `ruinames <ui.rpak>`.
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = tf_assets::rpak::Rpak::open(&a[1])?;
    let d = &pak.data;
    let mut names = std::collections::BTreeSet::new();
    let mut i = 0;
    while i + 4 < d.len() {
        if &d[i..i + 4] == b"rui/" && (i == 0 || d[i - 1] == 0) {
            let end = d[i..].iter().position(|&b| b == 0).map(|e| i + e).unwrap_or(d.len());
            if let Ok(s) = std::str::from_utf8(&d[i..end]) {
                if s.len() < 120 && s.chars().all(|c| c.is_ascii_graphic()) {
                    names.insert(s.to_string());
                }
            }
            i = end;
        }
        i += 1;
    }
    for n in names {
        println!("{n}");
    }
    Ok(())
}
