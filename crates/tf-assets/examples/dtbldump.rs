//! Print an RPAK datatable as tab-separated text: `dtbldump <file.rpak> <datatable/name.rpak>`
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pak = tf_assets::rpak::Rpak::open(&a[1])?;
    let t = tf_assets::datatable::DataTable::load(&pak, &a[2]).ok_or_else(|| anyhow::anyhow!("no datatable {}", a[2]))?;
    println!("{}", t.columns.join("\t"));
    for r in &t.rows {
        println!("{}", r.iter().map(|c| c.text()).collect::<Vec<_>>().join("\t"));
    }
    Ok(())
}
