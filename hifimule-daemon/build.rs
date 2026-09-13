fn main() {
    const VERIFIED_ENV: &str = "HIFIMULE_FFMPEG_RUNTIME_VERIFIED";
    const VERIFIED_VALUE: &str = "ffmpeg-9-runtime-verified-v2";

    println!("cargo:rerun-if-env-changed={VERIFIED_ENV}");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let runtime_was_verified = match std::env::var(VERIFIED_ENV) {
        Ok(value) if value == VERIFIED_VALUE => true,
        Ok(value) => panic!(
            "invalid {VERIFIED_ENV} marker {value:?}; do not set this build-orchestration variable manually. Use `npm run build:daemon -- <cargo arguments>` so the exact controlled FFmpeg runtime is validated first"
        ),
        Err(std::env::VarError::NotPresent) => false,
        Err(std::env::VarError::NotUnicode(_)) => panic!(
            "invalid non-Unicode {VERIFIED_ENV} marker; remove it and use `npm run build:daemon -- <cargo arguments>`"
        ),
    };

    if !runtime_was_verified {
        panic!(
            "the controlled FFmpeg runtime was not verified. Raw `cargo` is unsupported; use `npm run build:daemon -- <cargo arguments>` (for example `npm run build:daemon -- build -p hifimule-daemon`)"
        );
    }
    if target_os == "linux" {
        println!("cargo:rerun-if-env-changed=HIFIMULE_FFMPEG_PREFIX");
        let prefix = std::env::var_os("HIFIMULE_FFMPEG_PREFIX").expect(
            "Linux builds require the verified HIFIMULE_FFMPEG_PREFIX; use npm run build:daemon",
        );
        // Keep the private libraries ahead of system search directories emitted
        // by libmtp and other native dependencies, which may also contain FFmpeg.
        println!(
            "cargo:rustc-link-search=native={}",
            std::path::PathBuf::from(prefix).join("lib").display()
        );
    }
    if target_os == "macos" {
        // Embed Info.plist so macOS reads LSUIElement=true at process launch,
        // suppressing the Dock icon before NSApplication is even initialised.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg=-sectcreate");
        println!("cargo:rustc-link-arg=__TEXT");
        println!("cargo:rustc-link-arg=__info_plist");
        println!("cargo:rustc-link-arg={manifest_dir}/Info.plist");
    }
    if target_os == "windows" {
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
                let content = std::fs::read_to_string(&header).expect("failed to read libmtp.h");
                let entries: Vec<&str> = content
                    .lines()
                    .map(|l| l.trim())
                    .filter(|t| {
                        t.starts_with("LIBMTP_FILETYPE_")
                            && !t.contains('(')
                            && !t.contains('=')
                            && !t.contains('#')
                    })
                    .map(|t| t.trim_end_matches(','))
                    .collect();
                assert!(
                    !entries.is_empty(),
                    "Could not find LIBMTP_FILETYPE_ enum entries in libmtp.h"
                );
                let out_dir = std::env::var("OUT_DIR").unwrap();
                let mut out = String::new();
                for (i, name) in entries.iter().enumerate() {
                    out.push_str(&format!(
                        "#[allow(dead_code)] const {}: u32 = {};\n",
                        name, i
                    ));
                }
                std::fs::write(format!("{out_dir}/libmtp_constants.rs"), out)
                    .expect("failed to write libmtp_constants.rs");
                break;
            }
        }
    }
}
