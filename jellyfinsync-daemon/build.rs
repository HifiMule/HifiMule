fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../jellyfinsync-ui/src-tauri/icons/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
    // TODO: replace with libmtp-rs when stable — currently using FFI fallback
    if std::env::var("CARGO_CFG_UNIX").is_ok() {
        println!("cargo:rustc-link-lib=mtp");
    }
}
