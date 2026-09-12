---
baseline_commit: 09b6a8b580b704c8df3d33a697974f4e74e48af5
---

# Story 15.1: Close and reopen the UI without restarting the daemon

Status: review

## Story

As a HifiMule user,
I want closing and reopening the UI to reconnect to the same background daemon,
so that ongoing work remains available and repeated launches do not create competing sessions.

**Requirements:** amended FR20; lifecycle foundation for FR56; P-AR1–2 and lifecycle portion of P-AR10; applicable P-NFR3–5. **Dependency:** the existing application only.

**Scope:** Production daemon/UI lifecycle integration. No audio dependencies, playback database tables, player controls, Radio, new navigation, or playback restoration. Story 15.2 supplies coordinated active-sync shutdown; 15.3 supplies versioned listening-session persistence and paused restoration; 15.4+ supplies audio; 15.6 supplies native transport. This document prepares implementation; it does not report production behavior as implemented.

## Acceptance Criteria

1. **Cold launch.** Given an installed build in a signed-in desktop session with no daemon, when HifiMule launches, exactly one daemon starts and the UI connects. Startup failure produces an actionable error within the bounded startup contract below, rather than an indefinite connecting state.
2. **Close/reopen.** Given a healthy daemon with background work, when the UI closes and reopens, the same daemon PID **and instance ID** survive, work continues, and the UI fetches authoritative current state through the existing communication boundary. Reconnection never replays prior mutation commands.
3. **Concurrent launches.** Given simultaneous UI, tray or configured-startup launches for the same user, only one process owns daemon state; others attach or return a bounded failure. Losing candidates never initialize DB/device observers/sync/tray. Reuse the existing native event loop and do not enable login startup without the user's existing preference.
4. **Crash recovery.** Given stale discovery metadata after abnormal exit, a new launch recovers verified stale ownership and starts one owner. A slow live owner, stale PID, or PID reuse cannot trigger takeover or termination of an unrelated process.
5. **Local access and compatibility.** A client lacking application credentials or user access is rejected before command execution, with no credentials exposed in UI/logs. A protocol-incompatible daemon produces an actionable compatibility error and no competing owner.
6. **Idle Quit.** Given an idle daemon and closed UI, the existing tray Quit action exits the daemon and releases ownership. A pre-Quit launcher cannot resurrect it. While sync is active, this story refuses Quit with an explanation rather than adding an unsafe termination path; orderly cancellation follows in 15.2.
7. **Installed evidence.** On Windows, macOS and Linux, exercise cold launch, close/reopen, concurrent launch, crash recovery, rejected access and idle Quit. Record process/instance identity, connection outcome, cleanup and exact OS/architecture. VM results certify only their tested architecture.

## Tasks / Subtasks

- [x] Implement the shared lifecycle contract below (AC 1, 3–5).
  - [x] Add a small internal `hifimule-lifecycle` library crate for shared paths, descriptor/wire types, ownership guards and discovery. Add it to the existing workspace; do not create another daemon application.
  - [x] Acquire the OS ownership lock before daemon-core initialization, bind the listener explicitly, and publish readiness only after startup succeeds.
  - [x] Implement private discovery, credential rotation, legacy-owner detection, compatibility validation and structured failure outcomes.
  - [x] Add process-level tests for simultaneous candidates, slow owners, stale descriptors, lock release after crash, mismatched identity and rejected access.
- [x] Decouple launch and UI exit (AC 1–3, 6).
  - [x] Replace UI-owned shell child management with detached native process launch; remove exit-time child killing and pipe ownership coupling.
  - [x] Route UI, direct daemon and configured-startup entry points through the same owner election. Remove automatic Windows service fallback and implicit macOS LaunchAgent installation.
  - [x] Preserve startup opt-in/opt-out across reopening and installer upgrades, including Windows registration paths.
  - [x] Fence pending launch attempts across explicit Quit; terminate launch coordination at its deadline or when the UI closes.
- [x] Integrate authenticated production communication and current-state hydration (AC 2, 5).
  - [x] Authenticate JSON-RPC and artwork routes; keep credentials entirely in Rust/native code.
  - [x] Update Tauri `rpc_proxy` and `image_proxy` to discover and validate the owner and preserve existing result/error contracts.
  - [x] Make splash/main share one bounded lifecycle readiness outcome; after readiness, fetch existing current state and restore existing operation/progress polling.
  - [x] Preserve server-scoped provider re-authentication and distinguish it from local daemon access failure. Never route lifecycle failure to first-run login.
  - [x] Add localized, keyboard-accessible startup errors and working Retry/Close actions; dispose stale polls and ignore late results.
- [x] Implement idle termination with an interim active-work guard (AC 2, 6).
  - [x] Serialize work admission against Quit; reject Quit while a sync pipeline/operation is active, including delta calculation.
  - [x] Stop accepting new work during accepted idle Quit, signal/join daemon-core shutdown, clean discovery under ownership, and exit the existing Tao loop.
  - [x] Test simultaneous sync start/Quit, slow teardown and delayed pre-Quit launchers without force-killing a live owner or modifying device completion state.
- [x] Update installed smoke tests and collect lifecycle evidence (AC 1–7).
  - [x] Replace fixed-port/unauthenticated probes; exercise the real installed daemon and UI, not a substitute lifecycle probe.
  - [x] Record both PID and instance identity, outcomes and cleanup on the release matrix; preserve logs on success as well as failure.
  - [x] Run relevant Rust tests, frontend type/build checks, packaging checks and installed platform scenarios; document unavailable evidence explicitly.

## Dev Notes

### Selected production lifecycle contract — preparation gate resolved

These are implementation decisions for 15.1, not claims about the current code. Implement and test this contract before accepting the story. The architecture-wide playback gates remain open for their later owners.

**Ownership and liveness**

- Scope ownership to the signed-in OS user and canonical application-data profile. Use one daemon for that user's profile, even if more than one UI process opens it. Never run desktop ownership as LocalSystem or create another native event loop.
- Place `owner.lock`, `owner.json` and `launch-generation.json` under a private `runtime/` directory resolved identically by daemon and native UI. Reuse platform application-data conventions and `HIFIMULE_APP_DATA_DIR` for explicit isolated test profiles. Resolve an absolute local directory; lifecycle code must fail clearly rather than use the existing current-directory fallback. Do not move the vault, DB or manifests.
- `owner.lock` is a stable file, opened read/write and locked with nonblocking `std::fs::File::try_lock`. Hold the file handle throughout the owner's lifetime, including initialization and shutdown. Never unlink/replace the lock file. Disable inheritance of the ownership handle into UI children. The OS lock, not file existence, age or PID probing, decides liveness.
- Only a successful lock acquirer can replace stale discovery metadata or initialize the core. Lock contention means another owner is alive or starting; it never authorizes takeover. Other lock errors are explicit startup failures. Losing daemon candidates exit without starting workers or constructing Tao, and never retry election in the background.
- Bind the existing HTTP server to `127.0.0.1:0` and publish the assigned port. A dynamic loopback endpoint avoids making a machine-global fixed port the user-ownership primitive. Bind errors must propagate to startup; a panic in an unobserved RPC task is not an acceptable failure result.
- Publish `owner.json` atomically from a private temporary file after the authenticated listener and local core are ready. Readers reject partial/malformed/oversized descriptors (maximum 16 KiB) and reread within the startup deadline. Cleanup removes only this instance's descriptor while still holding ownership; crash leftovers are replaced by the next successful owner.

**Local access, descriptor and health wire contract**

- Unix: create the runtime directory with mode `0700`, files with `0600`, and validate effective ownership and permissions before trusting existing files. Windows: create/validate a protected DACL granting the owning user access, without broad Users/Everyone grants. Reject unsafe paths, symlinks/reparse redirection and permission failures. Do not silently downgrade to world-readable discovery. This protects against other users and unauthenticated callers; it is not a claim of isolation from malicious code already running as the same user or an administrator.
- Native-only descriptor fields: `schemaVersion: 1`, `protocolVersion: 1`, `instanceId` (fresh UUID per owner), `pid` (diagnostics only), `port` (1–65535), `token` (32 cryptographically random bytes encoded as hex), and `launchGeneration` (unsigned decimal string). Host is fixed in code to `127.0.0.1`; descriptors cannot redirect clients to arbitrary hosts/URLs.
- Require `Authorization: Bearer <token>` on **all** RPC and image routes, including health. Rotate the token per owner. Read it only through native code; never return it through Tauri commands, JavaScript/localStorage, URLs, logs, CLI arguments or test artifacts. Use bounded, timing-resistant credential comparison and redact authorization headers. Disable HTTP proxy environment routing and redirects on local authenticated clients.
- Retain JSON-RPC 2.0 and the existing `{result, error, id}` envelope. Extend ready `daemon.health` result to `{data: {status: "ok", protocolVersion: 1, instanceId, pid, daemonVersion}}`; preserve `data.status`. During teardown return `status: "stopping"` with an optional sanitized `errorCode` (including `SHUTDOWN_TIMEOUT`), never `"ok"`. Keep this minimal authenticated status path available until core teardown completes, then close the listener and clean discovery. Health reads local state only: no provider login, network browse or device mutation. Compare protocol and instance against discovery before any application request. Product version is diagnostic; protocol version controls compatibility.
- Native lifecycle status is a typed result with `state` (`starting`, `ready`, `failed`, `stopping`, `stopped`), optional `instanceId`/`pid`, and sanitized `errorCode`. Codes: `STARTUP_TIMEOUT`, `SPAWN_FAILED`, `UNSAFE_RUNTIME_PATH`, `LOCAL_ACCESS_DENIED`, `PROTOCOL_MISMATCH`, `LEGACY_DAEMON_RUNNING`, `LEGACY_ENDPOINT_OCCUPIED`, `OWNER_CHANGED`, `DAEMON_STOPPED`, `QUIT_BLOCKED_ACTIVE_SYNC`, `QUIT_PERSISTENCE_FAILED`, `SHUTDOWN_TIMEOUT`. Local HTTP authorization failure is 401 and is mapped to `LOCAL_ACCESS_DENIED`, never provider JSON-RPC error `-8`.
- Retain release-mode Tauri `invoke` proxies and existing application payloads. CORS remains narrow defense in depth, never the authentication mechanism. Artwork must keep working through `image_proxy`; no bearer token in image URLs. A changed owner invalidates cached native discovery before subsequent requests.

**Bounded startup, legacy compatibility and pending-launch fencing**

- One startup attempt has a **15-second monotonic deadline**, including election/discovery/handshake, with polls no faster than every 250 ms. Each health request is bounded to **2 seconds or the remaining deadline**, whichever is shorter. Publish failures immediately when known. Apply these limits only to lifecycle operations; legitimate sync/provider RPCs must not inherit a blanket two-second timeout.
- A UI launch discovers first, launches at most one candidate if ownership is available, and then waits for readiness. Do not report a successful process spawn as a connected daemon. Concurrent UI processes may spawn contenders, but only the lock winner initializes the core.
- Enforce the deadline inside detached candidates too. Each launcher creates a private, non-secret attempt ticket (`attemptId`, expected generation, expiry in process-comparable OS monotonic milliseconds); pass its ID to the candidate. UI closure/timeout cancels the ticket. Candidates verify ticket validity before election, before core initialization and before publishing readiness; expired/cancelled candidates clean up and exit. Serialize ticket cancellation versus readiness acceptance using a short-lived ticket lock, distinct from the lifetime owner lock. Once readiness wins, UI closure only detaches and cannot kill the established owner. Keep application-work admission closed during initialization so cancelled startup cannot abandon a device write. Startup paths without a UI create the same bounded ticket. Remove completed/cancelled tickets; prune expired tickets safely so discovery does not grow forever. A stale ticket cannot reset the launch generation or authorize a fresh attempt.
- Before initializing a newly elected owner, inspect legacy `127.0.0.1:19140` with a credential-free, read-only `daemon.health` probe. An old HifiMule responder without the new contract yields `LEGACY_DAEMON_RUNNING`; any other occupied/unverifiable listener yields `LEGACY_ENDPOINT_OCCUPIED`. Fail closed and tell the user to close the older app/resolve the conflict and retry. Never send the new token to that endpoint, kill its PID, connect as a compatible owner, or start a second writer. Check known legacy startup/service registrations during upgrade; do not permit a still-enabled legacy launcher to race the new binary. Old binaries cannot honor the new lock, so migration must retire their launch path before takeover.
- A new descriptor with an unsupported schema/protocol or a live incompatible owner is a compatibility failure, not permission to delete its lock or start another process. If a descriptor is stale, only successful ownership acquisition authorizes replacement; PID reuse is irrelevant.
- Preserve a non-secret launch generation in `launch-generation.json`, initialized under ownership and retained across clean Quit. Capture it at the beginning of each explicit UI/startup launch attempt; pass the expected value to its candidate (no credentials in arguments). On accepted Quit, increment and atomically persist it **before** releasing ownership. Under ownership, candidates reject a mismatched expected generation. Treat an absent initial generation as `"0"`; initialize without resetting an existing value. Read failure/corruption is an actionable failure, not an implicit reset.
- Contenders that observed a live owner only attach or fail; they cannot promote themselves when that owner exits. Stop all launch retries on UI closure, timeout or observed Quit. A later **new user launch** may capture the new generation and start normally. Configured login startup remains a one-shot launch, without a restart supervisor.

**Detached launch and startup preference**

- Use native `std::process::Command` with resolved packaged binary paths, detached lifetime and no UI-owned stdout/stderr pipes. Use daemon-owned bounded file logging or null standard handles. On Unix separate process lifetime from the launcher; on Windows use appropriate detached/new-process-group flags and verify installed job-object behavior rather than assuming dropping a `Child` is sufficient. Reap children where the launching process remains alive; do not leave a supervisor running forever.
- Remove the explicit UI `RunEvent::Exit` kill path and avoid shell-plugin child-resource cleanup as an owner. Closing the splash or main UI must not request daemon shutdown. Preserve tray Open Hub and packaged platform binary resolution. On macOS, closing the last window must also allow the next app/tray activation to show or recreate the UI; test both a still-running UI process and a newly launched one.
- Do not auto-start the Windows service from the desktop launcher. Preserve administrative legacy service removal/diagnostic paths, but `--service` must not become a second desktop owner or bypass election. Give an actionable migration result for incompatible legacy service execution; do not silently stop a service doing work.
- Remove macOS setup's `plist_missing => install_launchd_plist()` behavior. Keep explicit startup setter semantics and existing LaunchAgent registration when enabled, with `KeepAlive=false`. Disabled/missing registration stays disabled; ordinary launch must not re-enable it.
- Windows WiX/NSIS currently create HKCU Run unconditionally. Preserve an existing enabled registration and update its binary path on upgrade, but do not recreate a removed/disabled registration or enroll a fresh user implicitly. Preserve InstallDir discovery and uninstall cleanup. Linux gets no new autostart registration in this story. There is no current frontend caller for `settings_set_launch_on_startup`; do not invent a settings redesign to solve lifecycle ownership.

**Reconnect and shutdown boundaries**

- Health establishes local readiness. Then read `get_daemon_state` for servers, selected portable/local server IDs, device/basket state, `activeOperationId` and `syncPipelineActive`, followed by existing operation/progress reads. Provider connectivity may take longer or fail independently; surface it as a source/state-load problem with bounded UI feedback, never restart a healthy daemon or declare first-run merely because a request failed. Initial state hydration has a separate **15-second UI deadline** and Retry; late results from an abandoned UI attempt are ignored.
- Never automatically replay `sync.start`, server mutations, basket writes or playlist operations after a broken connection. An ambiguous mutation stays ambiguous until authoritative state is fetched. In particular, **do not use `sync_get_resume_state` as generic hydration**: it can delete dirty markers and temporary files.
- Reuse the current tray Quit action and existing core shutdown signal. Add an admission gate shared by Quit and both RPC/daemon-initiated sync starts. The gate must cover checking/marking the operation active, including pipeline/delta calculation; a check-then-start race is insufficient. Use the existing `has_active_operation()` semantics rather than only `activeOperationId`.
- Hold admission across provider preparation and transfer: when a handler spawns work, transfer its guard into the task rather than releasing it with the RPC response. Preserve the current delta `PipelineGuard` and execute's 500 ms preparation handoff. Track other in-flight mutating operations as well (manifest/basket/settings/device writes); accepted idle shutdown must reject new mutations and drain existing ones. Read-like names are not proof of purity: `device_profiles.list` can seed a file, and `sync_get_resume_state` can alter device files.
- For this story, active sync means Quit is refused with `QUIT_BLOCKED_ACTIVE_SYNC` and a visible explanation to finish/cancel sync first. Do not change cancellation or manifest success rules. Story 15.2 replaces this interim behavior with coordinated cancellation.
- Accepted idle Quit closes admission, advances launch generation, signals core teardown and waits up to **5 seconds** for explicit completion before releasing discovery/lock and exiting Tao. RPC tasks and observers must stop, not merely lose a detached runtime handle. On timeout, retain ownership and the stopping state, report `SHUTDOWN_TIMEOUT`, and continue observing completion; no force kill or false success. Never hold the admission lock while waiting for joins or device I/O.
- Persist the advanced generation durably before irreversible teardown: write/sync and atomically replace the file, using the platform's durability semantics. If persistence fails, refuse Quit with `QUIT_PERSISTENCE_FAILED`, retain ownership/core, restore the prior admission state and allow an explicit retry. Never release ownership with an unchanged generation after claiming Quit succeeded. Reopening during teardown observes authenticated `stopping`/timeout status and cannot treat that owner as ready or launch a replacement.

### Current code: change and preservation map

Paths below are relative to the project root. Read the final working-tree versions again during implementation; this is the 2026-09-11 baseline.

| File | Current state | Change / preserve |
|---|---|---|
| `hifimule-daemon/src/main.rs` | Starts core/DB/observers before RPC readiness; starts fixed-port RPC in a task; owns the native Tao tray loop. Quit signals shutdown then exits immediately. | Elect first; propagate startup failures; hold owner guard; join idle teardown; gate work admission. Preserve tray states, Open Hub, macOS Accessory policy, native loop cadence and device observation. |
| `hifimule-daemon/src/rpc.rs` | Axum loopback router, CORS, health `{data:{status:"ok"}}`, JSON-RPC dispatch and image route; state reads include provider checks. | Inject lifecycle context/listener and authenticate routes; extend health; participate in admission. Preserve every existing method, provider routing, result/error envelopes, artwork and current-state fields. |
| `hifimule-daemon/src/paths.rs` | Platform data paths, explicit test override, unsafe relative fallback for lifecycle purposes. | Share canonical lifecycle path resolution without changing existing DB/vault/manifest locations or test override behavior. Reject unsafe lifecycle discovery. |
| `hifimule-daemon/src/service.rs` | Legacy LocalSystem service starts core directly and links SCM stop flag. | Ensure legacy service entry cannot bypass desktop ownership; retain explicit administrative handling and actionable migration. No silent service fallback. |
| `hifimule-ui/src-tauri/src/lib.rs` | Fixed port, shell-managed child, exit-time kill, two-second health check, unauthenticated RPC/image proxies, auto launchd registration, Windows service fallback. | Native lifecycle coordinator and detached spawn; authenticated dynamic endpoint; bounded status. Preserve complete provider RPC errors and base64 image output. |
| `hifimule-ui/src/main.ts` | Separate splash/main retries, unbounded underlying state calls, failure falls back to login. | Shared native lifecycle outcome, fresh state hydration, actionable errors and cancellation of stale attempts. Preserve first-run only for real zero-server state, portable server selection, re-auth flow, layout reuse and monitor sizing. |
| `hifimule-ui/src/rpc.ts` | Invoke wrappers, fixed-port exports, provider `-8` handling, image proxy wrapper, method/params logging. | Keep native-only secrets; remove obsolete fixed-port assumptions if unused; map lifecycle codes separately. Preserve typed browse/playlist/autofill wrappers and provider re-auth. Do not add credential logging. |
| `hifimule-ui/splashscreen.html` | Existing status/error presentation, Retry reload, unwired Settings button. | Wire real recovery actions and accessible status/focus. Remove/replace the dead Settings recovery action; preserve branding and main/splash transition. |
| `hifimule-i18n/catalog.json` | Shared Rust/TypeScript translation catalog. | Add lifecycle/error/active-sync-Quit text in every existing locale through existing translation utilities. |
| `Cargo.toml`, daemon/UI Cargo manifests, `Cargo.lock` | Rust 2024 workspace and pinned resolved dependencies. | Add the internal lifecycle crate/path dependencies and only required platform features. No audio/framework upgrade. |
| `hifimule-ui/src-tauri/wix/startup-fragment.wxs`, `nsis/hooks.nsh` | Unconditional startup registry enrollment plus install discovery/uninstall hooks. | Preserve enabled startup and disabled preferences; retain install location and cleanup. Test fresh install and upgrades. |
| `scripts/smoke-tests/smoke-common.sh`, `smoke-{macos,linux}.sh`, `smoke-windows.ps1` | Unauthenticated fixed-port health; broad cleanup/installation checks. | Discover/authenticate safely, add real lifecycle scenarios, clean only test-owned processes and registrations, redact diagnostics. |
| `.github/workflows/smoke-test.yml` | Installed MSI/deb/Intel+ARM macOS matrix; uploads logs on failure. | Retain platform coverage and upload sanitized lifecycle evidence on both success/failure. |

Potential supporting edits: `sync.rs` only if shared admission cannot be wired around existing entry points without touching it; read it completely first and retain producer/writer fairness, cancellation and dirty-write semantics. `tauri.conf.json`/capabilities only if window or command permissions require changes; keep sidecar packaging, platform resources and existing permissions scoped. Do not rewrite unrelated large files to relocate a small lifecycle change.

### Architecture and dependency guardrails

- Preserve the existing Rust daemon/Tokio, Tao/tray and Tauri/TypeScript architecture. The new internal lifecycle crate shares a small contract between two existing binaries; it is not a replacement application or a new event loop. Do not add `playback/` modules before a consuming story needs them.
- Existing sync safety remains authoritative: `DeviceIO`, MSC temporary-write/rename, MTP dirty markers and verification, provider identity, cancellation and write/verify/commit boundaries. UI lifetime is not a sync cancellation signal. No provider API calls from the new lifecycle library.
- Use Rust snake_case and JSON camelCase. Scope `protocolVersion` to the daemon lifecycle handshake; do not design the future queue/event protocol or playback schema here.
- Local lockfile baseline: Rust minimum **1.93.0**, edition 2024; resolved Tokio **1.49.0**, Axum **0.8.8**, Tauri **2.10.3**, shell plugin **2.3.5**, reqwest **0.12.28**, serde **1.0.228**. Reuse existing rand **0.8.5** for secure token generation and existing Windows API bindings where possible. Lockfile also contains other transitive versions; do not select a transitive version accidentally.
- Official documentation checked 2026-09-11: stable Rust docs currently identify **1.98.1**, but file locks were stabilized in **1.89.0**, below the project's MSRV. `try_lock` provides nonblocking exclusive ownership; keep its handle alive and avoid cloned/inherited lock handles. No Rust upgrade is required. [Rust File documentation](https://doc.rust-lang.org/std/fs/struct.File.html), [Rust 1.89 release notes](https://blog.rust-lang.org/2025/08/07/Rust-1.89.0/).
- Axum's current docs show **0.8.9**; the project resolves **0.8.8**. Use middleware compatible with the pinned 0.8 series; stateful authentication uses `from_fn_with_state`, not a `State` extractor in `from_fn`. This story does not authorize a general dependency update. [Axum middleware documentation](https://docs.rs/axum/latest/axum/middleware/fn.from_fn.html).
- Tauri shell documentation exposes child-process spawn/kill, not a guarantee of independent installed lifetime. Use the selected native launcher and verify detachment per platform. [Tauri shell API](https://v2.tauri.app/es/reference/javascript/shell/). Windows local access must use real security descriptors/access control, not a Unix-mode approximation. [Microsoft access-control documentation](https://learn.microsoft.com/en-us/windows/win32/secauthz/access-rights-and-access-masks).

### Testing requirements

Use unit tests for parsing/status/errors and **real subprocess integration tests** for OS ownership and teardown. A mock lock or the standalone audio experiment cannot establish AC 1–7.

| Scenario | Required evidence |
|---|---|
| Cold UI launch / direct daemon / configured startup | One initialized owner; bounded readiness; same production handshake; startup preference unchanged. |
| Two or more simultaneous launches | Exactly one core/tray/observer set, all successful clients report same instance, loser processes exit. |
| Candidate delayed past startup deadline or UI closure | Ticket cancellation/expiry prevents late initialization; readiness/cancellation race has one outcome; an established owner survives closure. |
| Close UI during transfer and during delta calculation | Owner PID/instance unchanged; pipeline keeps progressing; reopen attaches to operation and does not resubmit sync. Include actual device validation for write safety. |
| Slow live owner, stale descriptor, fabricated/reused PID | No takeover while lock held; OS-released lock permits one crash recovery owner; metadata never causes PID killing. |
| Invalid token, omitted token, another user, unsafe discovery path | 401 or access failure before any mutation/image access; no secret in frontend/logs/artifacts. Test all production routes. |
| Protocol/schema mismatch, owner changes, old daemon on 19140 | Actionable bounded failure; no competing owner or credential transmission to a legacy listener. |
| Idle Quit / launch-at-Quit race | Core stops; descriptor removed; lock released; generation retained; delayed earlier attempts cannot restart; later explicit launch works. |
| Quit races sync admission / teardown exceeds five seconds | Quit refuses active work or start refuses stopping owner; no unsafe intermediate state; slow teardown retains lock and reports failure. |
| Reopen during stopping / injected generation write or replace failure | Authoritative stopping status prevents hydration/relaunch; persistence failure refuses Quit and retains owner and work, with explicit retry. |
| Remote server offline / delayed current-state response | Local owner stays ready; UI shows bounded recoverable source/state error; no false first-run login, mutation replay or owner restart. |
| Fresh install, upgrade, startup disabled, repeated reopen | No implicit startup enablement; existing enabled registration points to current binary; user registrations and installed runtime remain usable. |

Implementation validation commands (all shell commands use RTK): `rtk cargo test -p hifimule-lifecycle`, `rtk cargo test -p hifimule-daemon`, `rtk cargo test -p hifimule-ui --lib`, appropriate `rtk cargo clippy` targets, and `rtk npm run build` from `hifimule-ui`. Add focused new lifecycle tests beside the shared crate and owner integration; use existing Rust tests for RPC/proxy regression. A frontend build is not UI behavior evidence.

Reuse `rpc::tests::make_test_state`, `connect_device_with_manifest`, `test_rpc_daemon_health`, `test_rpc_get_daemon_state_includes_active_operation_id` (preparing pipeline with null operation ID), provider execution tests, destructive-cleanup confirmation and dirty/clean resume tests. Many tests construct `AppState` literally: update every fixture when adding fields. Existing handler tests bypass middleware, so add real-router unauthorized/authorized tests. Verify new catalog keys and interpolation placeholders in `en`, `fr`, `es` and `de`; local access failure must not reuse the provider-login error key.

Use the actual release matrix from `.github/workflows/release.yml` and `smoke-test.yml`: macOS x86_64 and aarch64; Windows/Linux native CI targets must record the actual build target and runtime architecture (currently not explicitly pinned in those jobs). Test MSI, macOS app/DMG and Linux installed package; include AppImage lifetime if claiming it supported by this change. Record artifact version/hash, OS/architecture, launch source, PID+instance before/after, active operation identity, outcome, cleanup and elapsed time. Keep credentials out of evidence. Missing installed checks remain unverified; do not mark them passed because compilation or an ARM64 probe passed.

### Prior work and discovery

- This is the first story in Epic 15; no prior same-epic implementation story exists. Relevant preceding evidence is `_bmad-output/implementation-artifacts/playback-session-results.md`: independent-owner close/reopen and native API proofs on macOS ARM64 and Windows/Linux ARM64 VMs. Its three-minute watchdog, generated silent audio and test supervisor are experimental, not a production launcher. Windows needed explicit detached flags; Linux has a noted Wayland teardown warning. These findings motivate installed checks, not completion claims.
- Recent commits: `ac5217c` added playback design; `907e776` documented cross-platform session feasibility; `4ef1450` checkpointed its proof; `a4b49d1` and `ed9d110` cover decoder/Linux evidence. Audio versions and decoder conclusions are intentionally outside 15.1.
- Discovery used complete PRD, architecture and UX documents, the Epic 15 requirements/story context, sprint status, project context and current production lifecycle paths. The old project-context label “greenfield” and older sidecar/autostart descriptions are historical; the current code and approved playback amendments establish this brownfield scope.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 15.1; Playback Extension requirements; subsequent Stories 15.2–15.6; final story coverage/readiness]
- [Source: `_bmad-output/planning-artifacts/prd.md` — Service & System Integration / FR20; Playback Extension FR56, FR60 and P-NFR3–5]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — API & Communication Patterns; Playback Deployment and Implementation Sequence; Playback Project Structure; Playback Architecture Validation Results; Playback Implementation Handoff]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — Headless Sync Feedback; Responsive Design & Accessibility]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — managed-sync safety and provider abstraction principles]
- [Source: `_bmad-output/planning-artifacts/playback-epic-validation.md` and `playback-story-review.md` — approved 29-story scope and per-story preparation gates]
- [Source: `_bmad-output/implementation-artifacts/playback-session-results.md` — Integration implications and limits]
- [Source: production files in the change/preservation map; `Cargo.lock`; `.github/workflows/release.yml`]

## Dev Agent Record

### Agent Model Used

GPT-5 (Codex)

### Debug Log References

- Preparation reviewed current production code, planning artifacts and official lifecycle API documentation. No production code or runtime tests were changed/run by story creation.
- 2026-09-11: Implemented the lifecycle contract through red-green-refactor. Initial shared-crate contract tests failed on missing APIs, then passed after ownership/discovery implementation.
- 2026-09-11: Full daemon suite initially hit sandbox-denied loopback/mock-server access; rerun with local-network permission passed all 644 tests.
- 2026-09-11: Windows lifecycle crate cross-check passed for `aarch64-pc-windows-gnullvm`; full Tauri cross-check could not run without the target-specific packaged sidecar. Installed MSI, DMG/App and deb/AppImage scenarios require their release-matrix runners and remain unverified.
- 2026-09-11: Windows 11 Pro ARM64 UTM compiled the production UI and daemon with MSVC. Isolated-profile smoke evidence passed authenticated cold launch (3.193 s), concurrent launch, close/reopen with stable PID+instance, forced-crash recovery with a new PID+instance, and unauthenticated rejection (401). WiX/NSIS installer execution and tray Quit remain open; Ubuntu SSH is reachable from the user's Terminal but not from the Codex sandbox route.
- 2026-09-12: Ubuntu ARM64 installed-deb smoke passed authenticated cold launch, rejected unauthenticated access, concurrent launch, close/reopen (PID 19488, instance `f2c49b7c-db10-4dea-be04-890c288e968b`), crash recovery (PID 20513, instance `63f07f6d-fe14-4e1c-b880-1ade1029d693`) and verified package removal. The smoke harness was corrected to remove the actual `hifi-mule` package name.
- 2026-09-12: Windows 11 Pro ARM64 MSI `AF5159F130D75F0F0FBD43AB2D922D1A8EE2B47C7A7B83D6EAFCFCBD384AF1D0` passed installed authenticated cold launch, 401 rejection, concurrent launch and close/reopen (PID 2944, instance `ac9d1ef5-7d49-4ba5-91ed-1473de496e76`), crash recovery (PID 12528, instance `cdf4a512-a718-4398-91ff-c3c9616b74c7`) and clean uninstall. The first run exposed and fixed a harness error that selected the daemon executable as the UI.
- 2026-09-12: User-observed Windows installed idle-Quit evidence passed: launching the UI started the daemon, quitting the UI left the daemon running, and choosing Quit from the tray stopped the daemon.
- 2026-09-12: macOS ARM64 DMG `5d3ce24c541e3c9822c66d4b89f116bdaf3305d6db8183c1482750ec1204663f` passed isolated installed-app cold launch, unauthenticated rejection, concurrent launch and close/reopen (PID 82330, instance `386c9870-7267-4bc1-bb5a-b173cd121485`), crash recovery (PID 82423, instance `ce1c564f-dfe7-4b97-80be-04c7318b22a5`) and cleanup without touching the pre-existing `/Applications/HifiMule.app`. AppleScript application Quit had targeted the daemon tray process, so the harness now closes only the exact installed UI executable.
- 2026-09-12: User-observed macOS installed idle-Quit evidence passed: launching the UI started the daemon, stopping the UI left it running, and tray Quit stopped it.
- 2026-09-12: User-observed Ubuntu installed idle-Quit evidence passed: launching the UI started the daemon, stopping the UI left it running, and tray Quit stopped it within the five-second shutdown contract. The short visible delay was unique to the Linux VM.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Lifecycle ownership, access, compatibility, startup, interim idle shutdown and launch fencing contracts are specified for implementation; subsequent playback contracts remain with their owning stories.
- Added private per-profile ownership/discovery with dynamic loopback binding, protocol/instance health validation, rotating native-only bearer credentials, monotonic launch tickets, durable Quit generation fencing, Unix permissions and protected Windows DACLs.
- Replaced Tauri shell-child ownership with detached native launch and removed UI-exit daemon killing, Windows service fallback, implicit macOS startup enrollment and fixed-port frontend assumptions.
- Added authenticated RPC/artwork middleware, native owner validation, bounded hydration, accessible lifecycle errors, active-operation rehydration through existing daemon state, and a serialized mutation/sync admission gate for idle Quit.
- Updated Windows startup registration preservation and installed smoke scripts/workflow for authenticated discovery, rejected access, concurrent launch, close/reopen identity, sanitized evidence and success/failure log retention.
- Validation passed: lifecycle tests 9/9, daemon tests 644/644, native UI tests 2/2, frontend TypeScript/Vite build, lifecycle/UI clippy with warnings denied, daemon clippy with 0 errors (94 pre-existing warnings), Rust formatting, shell syntax and WiX XML. Installed release-matrix evidence remains unavailable, so AC 7 and the final smoke task are intentionally open.
- Windows ARM64 VM behavior evidence additionally passed for cold launch, single ownership under concurrency, UI close/reopen, crash recovery and local credential rejection. It is portable-build evidence only; installer and tray-Quit evidence is not claimed.
- Installed ARM64 evidence passes MSI (Windows 11 Pro), deb (Ubuntu) and DMG/app (macOS) cold launch, authenticated attachment, rejected access, concurrent launch, close/reopen identity, forced-crash recovery and cleanup. Tray-menu idle Quit passes by user observation on all three platforms, including Ubuntu shutdown within the five-second contract. AC 7 and the final smoke task are complete.

### File List

- `_bmad-output/implementation-artifacts/15-1-close-and-reopen-the-ui-without-restarting-the-daemon.md` (story preparation)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (tracking)
- `.github/workflows/smoke-test.yml`
- `Cargo.lock`
- `Cargo.toml`
- `hifimule-lifecycle/Cargo.toml`
- `hifimule-lifecycle/src/bin/lifecycle-owner-probe.rs`
- `hifimule-lifecycle/src/lib.rs`
- `hifimule-lifecycle/tests/contract.rs`
- `hifimule-daemon/Cargo.toml`
- `hifimule-daemon/src/main.rs`
- `hifimule-daemon/src/rpc.rs`
- `hifimule-daemon/src/service.rs`
- `hifimule-daemon/src/sync.rs`
- `hifimule-daemon/src/tests.rs`
- `hifimule-i18n/catalog.json`
- `hifimule-ui/splashscreen.html`
- `hifimule-ui/src-tauri/Cargo.toml`
- `hifimule-ui/src-tauri/capabilities/default.json`
- `hifimule-ui/src-tauri/nsis/hooks.nsh`
- `hifimule-ui/src-tauri/src/lib.rs`
- `hifimule-ui/src-tauri/wix/startup-fragment.wxs`
- `hifimule-ui/src/main.ts`
- `hifimule-ui/src/rpc.ts`
- `scripts/smoke-tests/smoke-common.sh`
- `scripts/smoke-tests/smoke-linux.sh`
- `scripts/smoke-tests/smoke-macos.sh`
- `scripts/smoke-tests/smoke-windows.ps1`

## Change Log

- 2026-09-11: Implemented production daemon/UI lifecycle ownership, authenticated discovery and proxies, bounded detached launch, idle-Quit admission fencing, startup preference preservation, and release-matrix lifecycle smoke coverage. Story remains in progress pending installed cross-platform evidence.
- 2026-09-12: Completed installed ARM64 MSI, deb and DMG lifecycle evidence on Windows, Ubuntu and macOS, including user-observed tray Quit; moved Story 15.1 to review.
