# JellyfinSync — Development Guide

**Generated:** 2026-05-07 | **Scan depth:** Exhaustive

---

## Prerequisites

### All Platforms
- **Rust** (stable, MSRV 1.93.0) — install via [rustup](https://rustup.rs/)
- **Node.js** (LTS) + **pnpm** — for the UI
- **Tauri CLI v2** — `pnpm add -D @tauri-apps/cli` (installed via `package.json`)

### macOS
```bash
brew install pkg-config libmtp
# For universal builds, the CI also merges arm64+x86_64 libmtp dylibs (see release.yml)
```

### Ubuntu / Linux
```bash
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  patchelf pkgconf libxdo-dev libmtp-dev
```

### Windows
- Visual Studio Build Tools (MSVC)
- No extra system deps — WPD is part of the Windows SDK; MTP support is via `windows-sys` crate

---

## Repository Layout

```
jellyfinsync/
├── Cargo.toml                 Cargo workspace root
├── jellyfinsync-daemon/       Rust backend
└── jellyfinsync-ui/           Tauri 2 + TypeScript frontend
```

All `cargo` commands should be run from the repo root (workspace). All `pnpm`/`npm` commands from `jellyfinsync-ui/`.

---

## Build the Daemon

```bash
# Debug build
cargo build -p jellyfinsync-daemon

# Release build
cargo build -p jellyfinsync-daemon --release

# Check only (fast type-check, no binary)
cargo check -p jellyfinsync-daemon

# Clippy
cargo clippy -p jellyfinsync-daemon
```

Binary output: `target/debug/jellyfinsync-daemon` or `target/release/jellyfinsync-daemon`.

### Windows Service Flags

```bash
# Install as Windows Service (requires admin)
jellyfinsync-daemon.exe --install-service

# Uninstall
jellyfinsync-daemon.exe --uninstall-service

# Run as service (called by SCM, not directly)
jellyfinsync-daemon.exe --service
```

---

## Build the UI

```bash
cd jellyfinsync-ui

# Install dependencies
pnpm install

# Development mode (Vite dev server + Tauri hot reload)
pnpm tauri dev

# Production build
pnpm run build                           # tsc + vite → dist/
node ../scripts/prepare-sidecar.mjs     # copies daemon binary to src-tauri/sidecars/
pnpm tauri build                         # bundles: .dmg / .deb / .exe
```

### Important: Sidecar Preparation

Before running `pnpm tauri build`, the daemon binary must exist in `src-tauri/sidecars/` with the correct Tauri triple name. `prepare-sidecar.mjs` handles this automatically. In CI, the daemon is built first, then `prepare-sidecar.mjs` is run.

---

## Run Tests

```bash
# All daemon tests (unit + integration)
cargo test -p jellyfinsync-daemon

# Specific test module
cargo test -p jellyfinsync-daemon --lib db::tests
cargo test -p jellyfinsync-daemon --lib auto_fill::tests

# With output (for debugging)
cargo test -p jellyfinsync-daemon -- --nocapture
```

Tests in `api.rs` use `mockito` (HTTP mock server). Tests in `db.rs` use in-memory SQLite. No external services required.

---

## Running Daemon Standalone

The daemon can run standalone without the UI:

```bash
# Start daemon (interactive mode with system tray)
./target/debug/jellyfinsync-daemon

# The daemon listens on localhost:19140
# Test it with:
curl -X POST http://localhost:19140 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"get_daemon_state","params":{},"id":1}'
```

---

## Running UI in Dev Mode

```bash
cd jellyfinsync-ui
pnpm tauri dev
```

This starts:
1. Vite dev server on `http://localhost:1420`
2. Tauri app pointing at the dev server
3. The Tauri shell will attempt to launch the daemon sidecar (falls back gracefully if not found)

For faster UI iteration, you can run the daemon separately and start just Vite:
```bash
# Terminal 1: start daemon
./target/debug/jellyfinsync-daemon

# Terminal 2: start Vite + Tauri
cd jellyfinsync-ui && pnpm tauri dev
```

---

## Configuration Files

### Daemon App Data

Platform-specific app data directory (`get_app_data_dir()` in `paths.rs`):

| Platform | Path |
|----------|------|
| Windows | `%APPDATA%\JellyfinSync\` |
| macOS | `~/Library/Application Support/JellyfinSync/` |
| Linux | `$XDG_DATA_HOME/JellyfinSync/` or `~/.local/share/JellyfinSync/` |

Contents:
- `config.json` — `{ "url": "...", "user_id": "..." }`
- `jellyfinsync.db` — SQLite database
- `device-profiles.json` — transcoding profiles (auto-created from embedded asset on first run)
- `daemon.log` — daemon log (release builds only)
- `ui.log` — UI log (release builds, Windows only)

### UI Vite Config

`jellyfinsync-ui/vite.config.ts` — standard Vite config. RPC port can be overridden via `VITE_RPC_PORT` environment variable.

---

## Transcoding Profiles

`device-profiles.json` is seeded from `jellyfinsync-daemon/assets/device-profiles.json` on first run. To add a new profile:

1. Edit `jellyfinsync-daemon/assets/device-profiles.json`
2. Or edit the live file in the app data directory

Format:
```json
[
  {
    "id": "my-profile",
    "name": "My Device Profile",
    "description": "Convert to MP3 for classic iPod",
    "jellyfinProfileId": "..."
  }
]
```

The `"passthrough"` profile (id: `"passthrough"`) means no transcoding; the original file is downloaded as-is.

---

## Adding a New RPC Method

1. Add the method name to the dispatch `match` in `rpc.rs:handler()` 
2. Write the async handler function `async fn handle_my_method(state: &AppState, params: Option<Value>) -> Result<Value, JsonRpcError>`
3. Call the handler from the match arm: `"my_method" => handle_my_method(&state, params).await`
4. Add TypeScript typings / calls in the UI as needed

---

## MTP Development Notes

### Windows
WPD (Windows Portable Devices) requires the app to run with sufficient privileges. In development, run as Administrator or ensure the device is accessible to your user account. COM must be initialized before WPD calls — the daemon initializes COM in the relevant threads.

### Unix
libmtp must be installed (`libmtp-dev` on Debian/Ubuntu). The daemon links against it dynamically. `pkg-config --libs libmtp` must succeed at build time (enforced by `build.rs`).

MTP device paths on Unix use a `mtp://` prefix internally — `DeviceManager` handles this distinction from MSC paths.

---

## Debugging

### Enable Verbose Logging

In debug builds, all `daemon_log!` and `eprintln!` output goes to stdout/stderr. Run the daemon from a terminal to see it.

### Inspect the Manifest

```bash
cat /Volumes/YourDevice/.jellyfinsync.json | python3 -m json.tool
```

### Inspect the Database

```bash
sqlite3 ~/Library/Application\ Support/JellyfinSync/jellyfinsync.db
.tables
SELECT * FROM devices;
SELECT COUNT(*) FROM scrobble_history;
```

### RPC Direct Calls

```bash
# Check daemon state
curl -s -X POST http://localhost:19140 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"get_daemon_state","params":{},"id":1}' | python3 -m json.tool

# List transcoding profiles
curl -s -X POST http://localhost:19140 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"device_profiles.list","params":{},"id":1}' | python3 -m json.tool
```

---

## Release Process

Releases are triggered by pushing a `v*` tag:

```bash
git tag v0.3.0
git push origin v0.3.0
```

The GitHub Actions `release.yml` workflow:
1. Builds daemon + UI on macOS, Ubuntu, Windows
2. Packages Tauri bundles (`.dmg`, `.deb`, `.exe`)
3. Creates a GitHub Release with all artifacts

See `.github/workflows/release.yml` for the full pipeline.

---

## Common Issues

### `libmtp not found` (build error on Unix)

Install `libmtp-dev`:
```bash
# Ubuntu/Debian
sudo apt-get install libmtp-dev

# macOS
brew install libmtp
```

### Daemon port 19140 already in use

Another instance of the daemon is running. Kill it:
```bash
# macOS/Linux
pkill jellyfinsync-daemon
# or
lsof -i :19140
kill <pid>
```

### Device not detected

- **MSC**: ensure the device is mounted and accessible in Finder/Explorer
- **MTP**: on Linux, ensure `libmtp` is installed and the device is recognized (`mtp-detect`)
- **Windows MTP**: ensure WPD driver is installed (Device Manager)

### Manifest Dirty Flag

If the daemon crashed during sync, `.jellyfinsync.json` will have `"dirty": true`. Use the UI's Repair Modal (shown automatically when dirty is detected), or manually clear it:
```bash
# Edit .jellyfinsync.json on the device, set "dirty": false, remove "pending_item_ids": []
```
