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

        // Generate all LIBMTP_FILETYPE_* constants by parsing the enum in libmtp.h.
        // The enum starts at FOLDER=0 and increments by 1; UNKNOWN is the last entry.
        for inc_path in &lib.include_paths {
            let header = inc_path.join("libmtp.h");
            if header.exists() {
                let content = std::fs::read_to_string(&header)
                    .expect("failed to read libmtp.h");
                let entries: Vec<&str> = content.lines()
                    .map(|l| l.trim())
                    .filter(|t| {
                        t.starts_with("LIBMTP_FILETYPE_")
                            && !t.contains('(')
                            && !t.contains('=')
                            && !t.contains('#')
                    })
                    .map(|t| t.trim_end_matches(','))
                    .collect();
                assert!(!entries.is_empty(), "Could not find LIBMTP_FILETYPE_ enum entries in libmtp.h");
                let out_dir = std::env::var("OUT_DIR").unwrap();
                let mut out = String::new();
                for (i, name) in entries.iter().enumerate() {
                    out.push_str(&format!("#[allow(dead_code)] const {}: u32 = {};\n", name, i));
                }
                std::fs::write(
                    format!("{out_dir}/libmtp_constants.rs"),
                    out,
                ).expect("failed to write libmtp_constants.rs");
                break;
            }
        }
    }
}
