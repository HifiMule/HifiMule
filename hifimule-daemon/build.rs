fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "macos" {
        // Embed Info.plist so macOS reads LSUIElement=true at process launch,
        // suppressing the Dock icon before NSApplication is even initialised.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg=-sectcreate");
        println!("cargo:rustc-link-arg=__TEXT");
        println!("cargo:rustc-link-arg=__info_plist");
        println!("cargo:rustc-link-arg={manifest_dir}/Info.plist");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../hifimule-ui/src-tauri/icons/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
    if std::env::var("CARGO_CFG_UNIX").is_ok() {
        let lib = pkg_config::probe_library("libmtp")
            .expect("libmtp not found — install libmtp-dev / libmtp");

        // Generate LIBMTP_FILETYPE_UNKNOWN by counting the enum entries in libmtp.h.
        // The enum starts at FOLDER=0 and UNKNOWN is always the last entry.
        // We count enum lines (start with LIBMTP_FILETYPE_, no '(', '=', or '#') and
        // subtract 1 to get the 0-based index of UNKNOWN.
        for inc_path in &lib.include_paths {
            let header = inc_path.join("libmtp.h");
            if header.exists() {
                let content = std::fs::read_to_string(&header)
                    .expect("failed to read libmtp.h");
                let count = content.lines().filter(|l| {
                    let t = l.trim();
                    t.starts_with("LIBMTP_FILETYPE_")
                        && !t.contains('(')
                        && !t.contains('=')
                        && !t.contains('#')
                }).count();
                assert!(count > 0, "Could not find LIBMTP_FILETYPE_ enum entries in libmtp.h");
                let unknown_val = count - 1; // FOLDER=0, UNKNOWN=count-1
                let out_dir = std::env::var("OUT_DIR").unwrap();
                std::fs::write(
                    format!("{out_dir}/libmtp_constants.rs"),
                    format!("const LIBMTP_FILETYPE_UNKNOWN: u32 = {unknown_val};\n"),
                ).expect("failed to write libmtp_constants.rs");
                break;
            }
        }
    }
}
