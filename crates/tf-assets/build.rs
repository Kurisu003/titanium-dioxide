fn main() {
    let dir = "third_party/lzham_alpha/lzhamdecomp";
    let mut b = cc::Build::new();
    b.cpp(true)
        .include(dir)
        .include("third_party/lzham_alpha/include")
        .define("LZHAM_ANSI_CPLUSPLUS", None)
        .flag_if_supported("-w")
        .file("third_party/lzham_wrap.cpp");
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "cpp") {
            b.file(&p);
        }
    }
    b.compile("tflzham");
    println!("cargo:rerun-if-changed=third_party");
}
