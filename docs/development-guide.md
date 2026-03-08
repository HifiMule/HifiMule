# JellyfinSync — Development Guide

_Generated: 2026-03-08 | Scan Level: Quick_

## Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| Rust | Edition 2021 (MSRV 1.93.0) | Install via [rustup](https://rustup.rs/) |
| Node.js | LTS recommended | Required for UI and dev tooling |
| npm | Bundled with Node.js | Package manager |
| Tauri 2 CLI | ^2 | Installed as dev dependency in UI |
| VS Code | Latest | Recommended IDE |
| CodeLLDB / C/C++ ext | Latest | Required for Rust debugging |

## Repository Setup

```bash
# Clone the repository
git clone <repo-url>
cd JellyfinSync

# Install root dev dependencies (ESLint, Prettier, Jest, etc.)
npm install

# Install UI dependencies
cd jellyfinsync-ui && npm install && cd ..

# Build the entire project
npm run build
```

## Build Commands

| Command | Scope | Description |
|---------|-------|-------------|
| `npm run build` | All | Build UI (Tauri) + daemon (Cargo release) |
| `npm run build:ui` | UI | Build Tauri UI only |
| `npm run build:daemon` | Daemon | `cargo build --release` |
| `cargo build` | All | Build workspace in debug mode |
| `cargo build -p jellyfinsync-daemon` | Daemon | Build daemon only (debug) |

## Development Workflow

### Running the Daemon

```bash
# Standard run
cargo run -p jellyfinsync-daemon

# With backtraces for debugging
RUST_BACKTRACE=1 cargo run -p jellyfinsync-daemon

# Using the no-opt profile (faster compile, slower runtime)
cargo run -p jellyfinsync-daemon --profile no-opt
```

The daemon starts a system tray icon and listens on `http://127.0.0.1:19140/`.

### Running the UI

```bash
cd jellyfinsync-ui

# Vite dev server only (frontend hot reload, port 1420)
npm run dev

# Full Tauri dev mode (includes Rust backend)
npm run tauri dev
```

### Running Both Together

1. Start the daemon in one terminal: `cargo run -p jellyfinsync-daemon`
2. Start the UI in another terminal: `cd jellyfinsync-ui && npm run tauri dev`

## Testing

```bash
# Run all workspace tests
cargo test

# Run daemon tests only
cargo test -p jellyfinsync-daemon

# Run with output visible
cargo test -p jellyfinsync-daemon -- --nocapture
```

### Test Infrastructure
- **mockito** — HTTP request mocking for Jellyfin API tests
- **tempfile** — Temporary file/directory creation for isolated tests

## Debugging

### VS Code

1. Open Run and Debug (`Ctrl+Shift+D`)
2. Select "Debug jellysync-daemon" from the dropdown
3. Press `F5` to start
4. Set breakpoints in Rust source files

### JSON-RPC Testing

```bash
# Test connection
curl -X POST http://127.0.0.1:19140/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"test_connection","params":{"url":"http://your-jellyfin","token":"your-token"},"id":1}'

# Get daemon state
curl -X POST http://127.0.0.1:19140/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"get_daemon_state","params":{},"id":1}'
```

## Code Quality

| Tool | Config | Purpose |
|------|--------|---------|
| ESLint | `^9.39.2` (root) | TypeScript/JS linting |
| Prettier | `^3.8.1` (root) | Code formatting |
| lint-staged | `^16.2.7` (root) | Pre-commit hook linting |
| TypeScript | `strict: true` | Type checking (UI) |
| Cargo clippy | workspace | Rust linting |

## Project Profiles

The workspace `Cargo.toml` defines custom build profiles:

| Profile | Purpose | Settings |
|---------|---------|----------|
| `release` | Production builds | LTO, 1 codegen unit, abort on panic, stripped |
| `no-opt` | Fast debug builds | No optimization, full debug info, assertions enabled |

## Environment Variables

| Variable | Location | Purpose |
|----------|----------|---------|
| `RUST_BACKTRACE` | Runtime | Enable Rust panic backtraces |
| `TAURI_DEV_HOST` | UI dev | Custom Vite dev server host |
| `TAURI_ENV_PLATFORM` | Build | Target platform (set by Tauri) |
| `TAURI_ENV_DEBUG` | Build | Enable sourcemaps (set by Tauri) |
