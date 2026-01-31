use std::process::Command;

fn main() {
    // Only run npm build if we are NOT in a dev environment
    // This prevents cargo build from stalling during development
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    if profile == "release" {
        let (shell, args) = if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", "npm run build"])
        } else {
            ("sh", vec!["-c", "npm run build"])
        };

        println!("cargo:warning=Executing Vite production build...");
        let status = Command::new(shell)
            .args(&args)
            .current_dir("..")
            .status()
            .expect("Failed to build frontend");

        if !status.success() {
            panic!("Frontend build failed");
        }
    }

    tauri_build::build();
}
