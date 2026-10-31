fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GUI examples also need the Common Controls v6 activation manifest.
        let output = std::env::var("OUT_DIR").expect("Cargo supplies OUT_DIR");
        println!("cargo:rustc-link-arg-examples={output}/resource.lib");
    }
}
