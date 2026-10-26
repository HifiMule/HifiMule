# Dependency upgrade — 2026-10-26

Application version remains 0.20.0. Versions were checked against npm, crates.io,
and upstream release sources on 2026-10-26. No release was published.

## Toolchains and frontend

| Component | Previous project version | Updated version |
| --- | --- | --- |
| Rust minimum / CI | 1.93.0 | 1.99.1 |
| Node.js CI | 24.x | 26.12.1 |
| npm | Implicit/bundled | 12.2.1 |
| Vite, committed lockfile | 6.4.3 | 8.3.5 |
| TypeScript build compiler | 5.6.3 | 7.0.2 |
| Tauri API / CLI | 2.12.1 | 2.12.4 |
| Tauri opener, Rust and JS | 2.5.3 | 2.7.2 |
| source-map-js | 1.2.1 | 1.2.2 |

Shoelace remains at its current stable 2.20.1. TypeScript 7 supplies `tsc` through
the `@typescript/native` npm alias. The existing tests need the JavaScript
compiler API, so the `typescript` alias uses Microsoft's stable TypeScript 6.0.2
compatibility package, following the [official migration guidance](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/).
esbuild 0.28.2 is now an explicit test dependency: Vite 8 stopped supplying it
transitively. Vite configuration uses Rolldown options and Vite's client types.

Both CI workflows and local sidecar preparation install from the committed npm
lockfile. `.nvmrc`, `rust-toolchain.toml`, npm metadata, and setup documentation
declare the selected toolchains. The release workflow keeps an explicit Node
version so older immutable candidate refs without `.nvmrc` still build. Tauri
action commands leave npm argument separation to the action runner; regression
checks cover all three release branches. Verification used an isolated Node/npm install
and an additional Rust toolchain; global toolchain defaults were unchanged.

## Rust and native runtime

Direct Rust dependencies and the application lockfile were refreshed. Major
migrations include reqwest 0.13.5, rusqlite 0.41.0, tower-http 0.7.1, tray-icon
0.26.1, tao 0.37.2, rand 0.10.3, secrecy 0.10.3, zstd 0.14.3, and current
RustCrypto libraries. Tauri resolves to 2.12.5. Unused workspace keyring was
removed.

The networking migration explicitly retains native TLS, system proxy support,
HTTP/2, and query serialization. Credential encryption retains its nonce and
ciphertext format. A regression checks nonce freshness, round trips, and tamper
rejection. The old and new macOS machine-ID representations were also compared
locally and matched, preserving the input to credential-key derivation.

The Windows implementation was adapted to current namespaces and pointer
arguments. `windows` and `windows-core` remain aligned at 0.62.2; independently
selecting the latest core 0.100.0 would mismatch that binding family.

The controlled audio build now uses ffmpeg-sys-next 9.0.1, Ogg 1.3.6, and Opus
1.6.1, with updated archive hashes and notices. FFmpeg 9.0.2 remains the latest
stable release shown by the [official download page](https://ffmpeg.org/download.html).
CPAL 0.18.2, Souvlaki 0.8.3, ffmpeg-next 9.0.0, and libpulse-binding 2.30.1
remain current. Local playback source patches were retained. CPAL's separate
example dependency was upgraded to ringbuf 0.5.3 to fix RUSTSEC-2026-0293;
the feedback example compiles. CPAL's generated standalone lockfile is ignored
by Git; the committed manifests now require the patched version.

## Security results and compatibility limits

- **npm audit:** zero vulnerabilities, down from two high-severity packages.
- **Application Cargo.lock:** zero vulnerability-list entries, with the five
  RustSec informational warnings below. No advisories were suppressed.
- **CPAL standalone lockfile:** zero vulnerability-list entries after updating
  ringbuf.

| Remaining advisory | Dependency path / reason |
| --- | --- |
| [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html): GLib iterator unsoundness | `glib 0.18.5` through Tauri/GTK 3 and the UI's GTK integration; fixed in GLib 0.20+, which cannot replace this binding family independently. This memory-safety advisory remains unresolved. |
| [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html): unmaintained proc-macro-error | GTK / GLib macros require 1.0.4. |
| [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html): unmaintained zbus_names | `notify-rust` and `tauri-plugin-opener` depend on zbus 5. |
| [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html): unmaintained zvariant | Same zbus 5 dependency family. |
| [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html): unmaintained zvariant_derive | Same zbus 5 dependency family. |

GTK 0.18.2, gio 0.18.4, and glib 0.18.5 remain coherent with Tauri and Tao.
Upstream framework updates are needed to replace this family safely.

The independently resolved historical playback experiments were inventoried
without changing their APIs: their lockfiles have no vulnerability-list entries,
but retain informational warnings for anyhow, memmap2, instant, and ttf-parser.
Souvlaki's ignored standalone development lockfile also retains informational
warnings. These standalone resolutions are separate from the application lockfile.

GitHub's authenticated Dependabot alert list was unavailable locally; these are
registry/RustSec audit results, not confirmation that GitHub has closed alerts.

## Verification

- Clean `npm ci --ignore-scripts` and TypeScript 7 / Vite 8 production build pass.
- Node script/UI regressions: **636 passed, 1 skipped**.
- Python script regressions: **47 passed**.
- Controlled daemon unit tests: **1,702 passed, 16 ignored**.
- Mediacheck and daemon integration suites: **13 passed**.
- UI backend, lifecycle, and localization tests: **46 passed**.
- Controlled FFmpeg runtime rebuilt on macOS arm64 and passed ABI and native
  encoding checks.
- Windows lifecycle/i18n cross-check passes. An isolated checker compiles the
  actual MTP/WASAPI sources and shortcut implementation for Windows MSVC.
- npm's direct dependency outdated report is empty. `git diff --check` passes.

Full Windows linking, Linux builds, macOS Intel builds, and installed-package
smoke tests still need the existing CI matrix. No remote workflow was triggered.
The frontend retains its existing browser targets; Vite now warns about existing
BigInt literals targeting Safari 13 and an ineffective dynamic import. Existing
dead-code, atomic API deprecation, and Souvlaki future-compatibility warnings
remain visible.
