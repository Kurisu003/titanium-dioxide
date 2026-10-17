//! Print the asset GUID of each name given: `guidof <name>...`
fn main() {
    for a in std::env::args().skip(1) {
        println!("{:016x} {a}", tf_assets::string_to_guid(&a));
    }
}
