fn main() {
    println!("cargo:rerun-if-changed=src/process_group_macos.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/process_group_macos.c")
            .warnings(true)
            .compile("imboard_process_group");
        println!("cargo:rustc-link-lib=proc");
    }
}
