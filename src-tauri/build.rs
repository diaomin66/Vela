fn main() {
    tauri_build::build();
    // Tauri attaches the activation manifest to application binaries. GNU
    // examples also need it to load Common Controls v6 for the updater probe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
    {
        let resource = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("libresource.a");
        println!("cargo:rustc-link-arg-examples={}", resource.display());
    }
}
