fn main() {
    // `cfg(msfs)`: MSFS support is compiled in. It needs both the `msfs` feature and a
    // Windows target, because SimConnect only exists on Windows; the feature is on by
    // default, so this keeps a plain build working on macOS and Linux.
    println!("cargo:rustc-check-cfg=cfg(msfs)");
    let feature = std::env::var_os("CARGO_FEATURE_MSFS").is_some();
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if feature && windows {
        println!("cargo:rustc-cfg=msfs");
    }

    tauri_build::build()
}
