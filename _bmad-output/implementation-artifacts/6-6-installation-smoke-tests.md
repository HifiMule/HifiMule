# Story 6.6: Installation Smoke Tests

Status: done

Completion Note: Ultimate context engine analysis completed - comprehensive developer guide created for the reopened MTP smoke-test scope.

## Story

As a **System Admin (Alexis)**,
I want basic smoke tests that verify each installer produces a working application,
So that I can catch packaging regressions before releasing.

## Acceptance Criteria

1. **Install step succeeds**: Given a freshly built installer for any platform, the installer runs to completion without errors.
2. **Launch step succeeds**: The installed application launches (daemon process starts) after installation.
3. **Daemon health-check passes**: The daemon sidecar is reachable at `http://127.0.0.1:19140` and responds to a JSON-RPC `daemon.health` call with `{ "status": "ok" }`.
4. **Uninstall step succeeds**: The application uninstalls cleanly, leaving no residual processes.
5. **Failures are diagnostic**: Any step failure produces clear diagnostic output identifying which platform, which step, and the error message.
6. **MTP hardware test scope (Sprint Change 2026-04-30):** Given the smoke test environment has no physical MTP device, when the test suite runs, MTP device IO is verified by unit tests in `device_io.rs` (mock `MtpBackend` returning fixture data — defined in Story 4.0). The smoke test workflow explicitly notes: "MTP end-to-end detection requires manual hardware verification on each platform."

## Tasks / Subtasks

- [x] **T1: Add `daemon.health` RPC endpoint to daemon** (AC: #3)
  - [x] T1.1: In `jellyfinsync-daemon/src/rpc.rs`, add `"daemon.health"` to the match dispatch (line ~169) calling a new `handle_daemon_health()` handler
  - [x] T1.2: `handle_daemon_health()` returns `Ok(serde_json::json!({ "status": "ok" }))` — no state needed, pure connectivity probe
  - [x] T1.3: Add a unit test in the existing `#[cfg(test)]` block in `rpc.rs` verifying the handler returns `{ "status": "ok" }`

- [x] **T2: Create platform smoke-test scripts** (AC: #1–#5)
  - [x] T2.1: Create `scripts/smoke-tests/smoke-windows.ps1` — MSI install (silent), launch daemon, poll `daemon.health`, uninstall (silent)
  - [x] T2.2: Create `scripts/smoke-tests/smoke-linux.sh` — `.deb` install via `dpkg -i`, launch with Xvfb, poll `daemon.health`, uninstall via `dpkg -r`
  - [x] T2.3: Create `scripts/smoke-tests/smoke-macos.sh` — mount DMG, copy `.app` to `/Applications`, bypass quarantine, launch, poll `daemon.health`, remove `.app`
  - [x] T2.4: Create `scripts/smoke-tests/smoke-common.sh` (sourced by Linux/macOS scripts) — shared `poll_health` function: POST JSON-RPC to port 19140, retry up to 30s with 1s intervals, exit 1 with diagnostic output on timeout

- [x] **T4: Add MTP hardware test scope note (AC: #6 — Sprint Change 2026-04-30)**
  - [x] Add a comment block to `smoke-test.yml` (and/or `smoke-common.sh`) stating: "MTP end-to-end detection requires manual hardware verification on each platform. Automated MTP IO coverage is provided by unit tests in device_io.rs."
  - [x] Verify Story 4.0's `device_io.rs` includes a mock `MtpBackend` with fixture-data unit tests (verified during story creation; see Story-Creation Verification below)
  - **Depends on:** Story 4.0 (DeviceIO abstraction — provides mock MtpBackend unit tests)

- [x] **T3: Create `smoke-test.yml` GitHub Actions workflow** (AC: #1–#5)
  - [x] T3.1: Create `.github/workflows/smoke-test.yml` with `workflow_dispatch` trigger (manual run with input: `release_tag`)
  - [x] T3.2: Three jobs: `smoke-windows`, `smoke-linux`, `smoke-macos` — each on the matching runner
  - [x] T3.3: Each job downloads the installer artifact for its platform from the GitHub Release identified by `release_tag`
  - [x] T3.4: Each job calls the matching platform script from `scripts/smoke-tests/`
  - [x] T3.5: Each job uploads a smoke-test log as a workflow artifact on failure
  - [x] T3.6: Linux job installs Xvfb and GTK runtime libs before running the smoke script

### Story-Creation Verification for Active Reopen Scope

- T4 is the only remaining development scope. T1-T3 are already implemented and reviewed.
- Add the MTP scope note to `.github/workflows/smoke-test.yml` and/or `scripts/smoke-tests/smoke-common.sh` with this exact wording: "MTP end-to-end detection requires manual hardware verification on each platform. Automated MTP IO coverage is provided by unit tests in device_io.rs."
- `jellyfinsync-daemon/src/device_io.rs` has already been verified during story creation: it includes `MockMtpHandle` and `MtpBackend` tests for target-only write-with-verify behavior, manifest probing from fixture JSON, and dirty-marker listing.

## Dev Notes

### What to Build (Scope)

- **Active reopened scope only:** T1-T3 are already implemented and reviewed. The remaining development work is T4: document the MTP hardware-test boundary in the smoke-test workflow/helper and keep the automated verification pointed at `device_io.rs` unit tests.

- **T1** adds a dedicated `daemon.health` RPC method to the daemon — this is the canonical health-check call referenced by the AC. It must be a real RPC method, not a curl to an existing method, so it is stable and explicit.
- **T2+T3** are scripts + a new GitHub Actions workflow. The workflow runs *after* release artifacts exist (triggered manually, not from the release pipeline directly).
- **No changes to UI, frontend, or existing daemon logic** beyond the single new RPC handler.

### Daemon RPC Health Check — Critical Details

- **Port**: `19140` (hardcoded in `main.rs:191` — `rpc::run_server(19140, ...)`; also `RPC_PORT: u16 = 19140` in `jellyfinsync-ui/src-tauri/src/lib.rs:14`)
- **Protocol**: JSON-RPC 2.0 over HTTP POST to `http://127.0.0.1:19140`
- **New method**: `daemon.health` → handler returns `{ "status": "ok" }`
- **No existing health endpoint** — `get_daemon_state` exists but requires a full DB/device state and is heavier; `daemon.health` is purpose-built and always succeeds if the server is up
- **Example request**:
  ```json
  { "jsonrpc": "2.0", "method": "daemon.health", "params": {}, "id": 1 }
  ```
- **Expected response**:
  ```json
  { "jsonrpc": "2.0", "result": { "data": { "status": "ok" } }, "id": 1 }
  ```
- **Dispatch location**: `jellyfinsync-daemon/src/rpc.rs`, match arm list starting at line 127. Add before the `_ =>` catch-all at line 169. No state required.

### Adding `daemon.health` to `rpc.rs`

Insert into the match block (around line 167, before `"device.list"`):

```rust
"daemon.health" => Ok(serde_json::json!({ "status": "ok" })),
```

The result is wrapped by the `Ok(res)` branch in `match result` (line 176), so the final JSON-RPC response will be:
```json
{ "jsonrpc": "2.0", "result": { "data": { "status": "ok" } }, "id": 1 }
```

No imports needed. No state needed. No async needed — but the match arm expects `Result<Value, JsonRpcError>` matching the other handlers' return type.

### Current File State to Preserve

- `.github/workflows/smoke-test.yml` already has three independent jobs (`smoke-windows`, `smoke-linux`, `smoke-macos`), `timeout-minutes: 15`, failure log upload via `actions/upload-artifact@v4`, and runner labels `windows-latest`, `ubuntu-22.04`, and `macos-latest`. Do not rebuild this workflow; add only the MTP scope note unless verification exposes a defect.
- `scripts/smoke-tests/smoke-common.sh` already provides `poll_health()` using JSON-RPC `daemon.health` and parses `.result.data.status == "ok"` with `jq`. If the note is added here, keep it near the helper header so it is visible in every platform smoke path.
- `scripts/smoke-tests/smoke-linux.sh`, `smoke-macos.sh`, and `smoke-windows.ps1` already implement install -> launch -> daemon health -> uninstall with diagnostics. Do not add physical MTP probing to these scripts; CI runners have no reliable attached MTP hardware.
- `jellyfinsync-daemon/src/device_io.rs` already proves the automated MTP IO boundary with `MockMtpHandle` and tests named `mtp_write_with_verify_writes_target_only`, `mtp_backend_manifest_probe`, and `mtp_dirty_marker_detected_on_reconnect`.

### Implementation Guardrails for T4

- Prefer a YAML comment in `.github/workflows/smoke-test.yml` near the workflow name or job definitions so anyone running the manual smoke workflow sees the limitation before interpreting results.
- Optionally mirror the same note in `scripts/smoke-tests/smoke-common.sh` if you want the message to travel with local script usage. Avoid duplicating it in every platform script.
- The note must not weaken the smoke test itself: keep install, launch, `daemon.health`, uninstall, timeouts, and failure-log upload behavior unchanged.
- Do not mark AC #6 as physical hardware automation. The requirement is explicit: automated smoke tests acknowledge the hardware gap, while MTP IO behavior remains covered by unit tests.
- Do not introduce new dependencies, GitHub Actions, release triggers, or physical-device mocks in CI for this story.

### Platform-Specific Smoke Test Design

#### Windows (MSI)

```powershell
# smoke-windows.ps1
$ErrorActionPreference = "Stop"
$msi = Get-Item "*.msi" | Select-Object -First 1

Write-Host "[STEP 1] Installing $($msi.Name) ..."
Start-Process msiexec.exe -ArgumentList "/i `"$($msi.FullName)`" /qn /norestart" -Wait -NoNewWindow
if ($LASTEXITCODE -ne 0) { Write-Error "FAIL: Install step returned $LASTEXITCODE"; exit 1 }

Write-Host "[STEP 2] Launching JellyfinSync ..."
# MSI installs to Program Files — find and launch the executable
$exe = Get-ChildItem "C:\Program Files\JellyfinSync" -Filter "*.exe" -Recurse | Select-Object -First 1
Start-Process $exe.FullName -WindowStyle Hidden

Write-Host "[STEP 3] Polling daemon health (30s timeout) ..."
$body = '{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}'
$ok = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:19140" -Method Post -Body $body -ContentType "application/json" -TimeoutSec 2
        if ($r.result.data.status -eq "ok") { $ok = $true; break }
    } catch {}
    Start-Sleep 1
}
if (-not $ok) { Write-Error "FAIL: Daemon health check timed out after 30s"; exit 1 }
Write-Host "[STEP 3] Daemon responded OK"

Write-Host "[STEP 4] Uninstalling ..."
Start-Process msiexec.exe -ArgumentList "/x `"$($msi.FullName)`" /qn /norestart" -Wait -NoNewWindow
Write-Host "PASS: Windows smoke test complete"
```

#### Linux (`.deb` + Xvfb)

```bash
#!/usr/bin/env bash
set -euo pipefail
DEB=$(ls *.deb | head -1)
echo "[STEP 1] Installing $DEB ..."
sudo dpkg -i "$DEB"

echo "[STEP 2] Launching JellyfinSync via Xvfb ..."
Xvfb :99 -screen 0 1024x768x24 &
XVFB_PID=$!
export DISPLAY=:99
jellyfinsync &       # installed by deb to /usr/bin or similar; check package manifest
APP_PID=$!

echo "[STEP 3] Polling daemon health (30s timeout) ..."
source "$(dirname "$0")/smoke-common.sh"
poll_health 30 || { kill $APP_PID $XVFB_PID 2>/dev/null; echo "FAIL: Daemon health check timed out"; exit 1; }
echo "[STEP 3] Daemon responded OK"

echo "[STEP 4] Uninstalling ..."
kill $APP_PID $XVFB_PID 2>/dev/null || true
sudo dpkg -r jellyfinsync
echo "PASS: Linux smoke test complete"
```

> **Note:** Verify the installed binary name from `.deb` package manifest. It should match `productName` in `tauri.conf.json` ("JellyfinSync" → likely `jellyfinsync` lowercase on Linux). Check `jellyfinsync-ui/src-tauri/tauri.conf.json` `bundle.linux.deb` or `productName` for the actual binary name.

#### macOS (DMG)

```bash
#!/usr/bin/env bash
set -euo pipefail
DMG=$(ls *.dmg | head -1)
echo "[STEP 1] Mounting $DMG ..."
hdiutil attach "$DMG" -mountpoint /Volumes/JellyfinSync -nobrowse -quiet

echo "[STEP 1] Installing app ..."
cp -R /Volumes/JellyfinSync/JellyfinSync.app /Applications/
hdiutil detach /Volumes/JellyfinSync -quiet

# Remove quarantine attr — required for unsigned builds on CI runners
xattr -d com.apple.quarantine /Applications/JellyfinSync.app 2>/dev/null || true

echo "[STEP 2] Launching ..."
open -a JellyfinSync
sleep 3  # give Tauri time to spawn the sidecar

echo "[STEP 3] Polling daemon health (30s timeout) ..."
source "$(dirname "$0")/smoke-common.sh"
poll_health 30 || { echo "FAIL: Daemon health check timed out"; exit 1; }
echo "[STEP 3] Daemon responded OK"

echo "[STEP 4] Removing app ..."
pkill -f JellyfinSync || true
rm -rf /Applications/JellyfinSync.app
echo "PASS: macOS smoke test complete"
```

#### Common Helper (`smoke-common.sh`)

```bash
# poll_health <timeout_seconds>
poll_health() {
    local timeout=$1
    local body='{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}'
    for ((i=0; i<timeout; i++)); do
        response=$(curl -sf -X POST http://127.0.0.1:19140 \
            -H "Content-Type: application/json" \
            -d "$body" 2>/dev/null || true)
        if echo "$response" | grep -q '"status":"ok"'; then
            return 0
        fi
        sleep 1
    done
    echo "DIAGNOSTIC: Last curl attempt: $(curl -v -X POST http://127.0.0.1:19140 -H 'Content-Type: application/json' -d "$body" 2>&1 || true)"
    return 1
}
```

### GitHub Actions Workflow Design (`smoke-test.yml`)

```yaml
name: Smoke Test

on:
  workflow_dispatch:
    inputs:
      release_tag:
        description: 'Release tag to test (e.g. v0.1.0)'
        required: true

jobs:
  smoke-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Download MSI artifact
        run: gh release download ${{ inputs.release_tag }} --pattern "*.msi" --dir ./installers
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - name: Run smoke test
        working-directory: ./installers
        run: powershell -ExecutionPolicy Bypass -File ../scripts/smoke-tests/smoke-windows.ps1

  smoke-linux:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - name: Install runtime dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y xvfb libgtk-3-0 libwebkit2gtk-4.1-0 libappindicator3-1
      - name: Download .deb artifact
        run: gh release download ${{ inputs.release_tag }} --pattern "*.deb" --dir ./installers
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - name: Run smoke test
        working-directory: ./installers
        run: bash ../scripts/smoke-tests/smoke-linux.sh

  smoke-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - name: Download DMG artifact
        run: gh release download ${{ inputs.release_tag }} --pattern "*.dmg" --dir ./installers
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - name: Run smoke test
        working-directory: ./installers
        run: bash ../scripts/smoke-tests/smoke-macos.sh
```

> **Note on `gh` CLI**: GitHub Actions runners have `gh` pre-installed. `gh release download` requires `GH_TOKEN`.

### Platform-Specific Gotchas

#### macOS — Gatekeeper & Quarantine
- Unsigned DMGs/apps (code signing is post-MVP per architecture.md) will be quarantined
- `xattr -d com.apple.quarantine` removes the quarantine flag — **must run before `open`**
- `open -a JellyfinSync` uses the app name; or use `open /Applications/JellyfinSync.app`
- The universal DMG produced by 6.5 targets `universal-apple-darwin` — will run on both Intel and Apple Silicon runners

#### Linux — Virtual Display
- Tauri v2 requires a WebKit display. On headless CI: Xvfb (X virtual framebuffer) provides `:99`
- Runtime packages: `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libappindicator3-1` (runtime, not `-dev`)
- The daemon binary itself is headless — only the UI needs Xvfb. If we only need to verify the daemon starts, we could launch just the daemon binary. However, the installer smoke test must verify the full installed app (Tauri launches the daemon as a sidecar).
- The installed binary name from the `.deb` comes from `tauri.conf.json` → `productName` → likely `jellyfinsync` (lowercase). Verify against the actual .deb after building.

#### Windows — MSI Silent Install
- `/qn` = quiet mode, no UI; `/norestart` = suppress reboot prompt
- Installation target: `C:\Program Files\JellyfinSync\` (standard Tauri MSI default)
- The Tauri-built MSI may register an uninstall GUID — `msiexec /x MSI_PATH /qn` works more reliably than hunting the GUID in the registry
- The daemon starts as a child process of the UI (Tauri sidecar model) — the UI must be running for the daemon to start

### Project Structure Notes

```
c:\Workspaces\JellyfinSync\
├── .github/
│   └── workflows/
│       ├── release.yml                    ← existing (6.5); no changes
│       └── smoke-test.yml                 ← CREATE (T3)
├── scripts/
│   ├── prepare-sidecar.mjs               ← existing; no changes
│   └── smoke-tests/                      ← CREATE directory (T2)
│       ├── smoke-common.sh               ← shared poll_health helper
│       ├── smoke-windows.ps1             ← Windows MSI smoke test
│       ├── smoke-linux.sh                ← Linux .deb + Xvfb smoke test
│       └── smoke-macos.sh               ← macOS DMG smoke test
└── jellyfinsync-daemon/
    └── src/
        └── rpc.rs                        ← ADD "daemon.health" handler (T1)
```

### Key Files to Create/Modify

| File | Action | Notes |
|------|--------|-------|
| `.github/workflows/smoke-test.yml` | **CREATE** | Manual `workflow_dispatch` trigger |
| `scripts/smoke-tests/smoke-common.sh` | **CREATE** | `poll_health` shared function |
| `scripts/smoke-tests/smoke-windows.ps1` | **CREATE** | MSI lifecycle test |
| `scripts/smoke-tests/smoke-linux.sh` | **CREATE** | .deb lifecycle test with Xvfb |
| `scripts/smoke-tests/smoke-macos.sh` | **CREATE** | DMG lifecycle test |
| `jellyfinsync-daemon/src/rpc.rs` | **MODIFY** | Add `"daemon.health"` match arm + unit test |

### What NOT to Do

- Do NOT add code signing (post-MVP per architecture.md)
- Do NOT run smoke tests from the `release.yml` workflow directly — smoke tests need a published release artifact (chicken-and-egg); keep them as `workflow_dispatch`
- Do NOT use `get_daemon_state` as the health check — add the dedicated `daemon.health` endpoint (T1) so the check is lightweight and unambiguous
- Do NOT use `ubuntu-latest` — use `ubuntu-22.04` for consistency with the release pipeline (webkit2gtk-4.1 package availability)
- Do NOT skip the Xvfb step on Linux — Tauri webview requires a display even for a headless smoke test
- Do NOT use `msiexec /x {GUID}` — use `/x MSI_PATH` to avoid needing to look up the product GUID
- Do NOT use `AppImage` for the Linux smoke test (AppImage doesn't install, making uninstall trivial) — test `.deb` instead as it exercises the actual install/uninstall lifecycle

### Previous Story Intelligence (6.5 — CI/CD Pipeline)

- The release workflow produces: `.msi` (Windows), universal `.dmg` (macOS), `.AppImage` + `.deb` (Linux)
- The macOS build uses `--target universal-apple-darwin` — the `.dmg` contains a universal binary
- `pnpm/action-setup@v4` and `tauri-apps/tauri-action@v0` are floating — acceptable for MVP
- The daemon starts as a Tauri sidecar — the UI must be launched first (not just the daemon binary) for the smoke test to mirror real user flow
- `fail-fast: false` convention from 6.5: apply the same to `smoke-test.yml` matrix so all platform results are visible even on partial failure
- **123 Rust unit tests pass** as of 6.5 completion — do not regress; adding `daemon.health` handler and test must not break existing tests

### Latest Technical Information (checked 2026-05-03)

- GitHub-hosted runner labels are moving targets. Current GitHub docs and runner-image metadata map `windows-latest` to Windows Server 2025, `ubuntu-22.04` to Ubuntu 22.04 x64, and `macos-latest` to macOS 15 arm64. Keep the existing labels unless a runner-specific smoke failure proves a pin is needed.
- GitHub-hosted Linux/macOS runners have passwordless `sudo`, and Windows runners run as administrators with UAC disabled. The existing install/uninstall scripts can continue to use `sudo dpkg`, `/Applications`, and `msiexec` without adding privilege workarounds.
- `actions/upload-artifact@v4` remains the correct artifact action generation for this workflow, and `if-no-files-found: ignore` is a supported option. Keep the unique artifact names per job.

### References

- Daemon RPC dispatch: [jellyfinsync-daemon/src/rpc.rs:127-174](jellyfinsync-daemon/src/rpc.rs#L127-L174)
- Daemon port: [jellyfinsync-daemon/src/main.rs:184](jellyfinsync-daemon/src/main.rs#L184) — hardcoded `19140`
- UI RPC_PORT constant: [jellyfinsync-ui/src-tauri/src/lib.rs:14](jellyfinsync-ui/src-tauri/src/lib.rs#L14)
- Release workflow: [.github/workflows/release.yml](.github/workflows/release.yml)
- Architecture IPC pattern: architecture.md §API & Communication Patterns — JSON-RPC 2.0 over localhost HTTP
- Code signing deferred: architecture.md §Packaging & Distribution — post-MVP

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Debug Log References

- 2026-05-03: Confirmed exact MTP scope note was absent before T4 change via `rtk grep`.
- 2026-05-03: Confirmed exact MTP scope note is present in `.github/workflows/smoke-test.yml` and `scripts/smoke-tests/smoke-common.sh` via `rtk grep`.
- 2026-05-03: Ran `rtk cargo test` — 184 tests passed.
- 2026-05-03: Ran `rtk cargo clippy` — passed with 32 existing warnings.
- 2026-05-03: Ran `rtk cargo clippy --all-targets --all-features -- -D warnings` — failed on pre-existing unrelated warnings in Rust files such as `device/tests.rs`, `api.rs`, `auto_fill.rs`, `device/mtp.rs`, `main.rs`, `rpc.rs`, `sync.rs`, and `device/mod.rs`; no T4 code was changed.

### Completion Notes List

- T1: Added `"daemon.health"` match arm inline in `rpc.rs` dispatch block (before `_ =>` catch-all). Returns `Ok(serde_json::json!({ "data": { "status": "ok" } }))` — consistent with other handler response shapes and the PowerShell smoke-test's `result.data.status` check. No imports or state required.
- T1.3: Unit test `test_rpc_daemon_health` calls the full `handler()` function with `"daemon.health"` method and asserts `result["data"]["status"] == "ok"`. All 164 daemon tests pass (no regressions from prior 123 baseline).
- T2: Created `scripts/smoke-tests/` directory with four scripts. `smoke-common.sh` provides the shared `poll_health` function (30s retry loop, diagnostic curl on timeout). Platform scripts each cover the 4-step lifecycle: install → launch → poll health → uninstall. Failures emit `FAIL [platform=X] [step=Y]: message` for AC #5.
- T3: Created `.github/workflows/smoke-test.yml` with `workflow_dispatch` trigger and `release_tag` input. Three independent jobs (`smoke-windows`, `smoke-linux`, `smoke-macos`) — each downloads its installer via `gh release download`, runs the matching script, and uploads a `smoke-log-*` artifact on failure. Linux job installs `xvfb libgtk-3-0 libwebkit2gtk-4.1-0 libappindicator3-1` before the smoke script. `ubuntu-22.04` used (not `ubuntu-latest`) for webkit2gtk-4.1 compatibility.
- T4: Added the exact MTP hardware-test boundary note to `.github/workflows/smoke-test.yml` and `scripts/smoke-tests/smoke-common.sh`, preserving install, launch, daemon health, uninstall, timeout, and failure-log behavior. Verified `device_io.rs` contains `MockMtpHandle`, `MtpBackend`, and fixture-data MTP unit tests named in the story.

### File List

- `jellyfinsync-daemon/src/rpc.rs` (modified — added `daemon.health` match arm + unit test `test_rpc_daemon_health`)
- `scripts/smoke-tests/smoke-common.sh` (created — shared `poll_health` helper; modified in T4 with MTP manual hardware verification scope note)
- `scripts/smoke-tests/smoke-windows.ps1` (created — Windows MSI smoke test)
- `scripts/smoke-tests/smoke-linux.sh` (created — Linux .deb + Xvfb smoke test)
- `scripts/smoke-tests/smoke-macos.sh` (created — macOS DMG smoke test)
- `.github/workflows/smoke-test.yml` (created — manual `workflow_dispatch` smoke-test workflow; modified in T4 with MTP manual hardware verification scope note)
- `_bmad-output/implementation-artifacts/6-6-installation-smoke-tests.md` (modified — story status, T4 checkbox, validation notes, file list, and change log)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (modified — story status tracking)

### Review Findings

- [x] [Review][Patch] No .log file written by any script — artifact upload always finds nothing [.github/workflows/smoke-test.yml + all smoke scripts]
- [x] [Review][Patch] `poll_health` grep fragile — `'"status":"ok"'` false-negatives on `"status": "ok"` (spaces) and false-positives on substring match [scripts/smoke-tests/smoke-common.sh:18]
- [x] [Review][Patch] `smoke-macos.sh`: `cleanup()` not registered as EXIT trap — DMG/app left behind on step 1–2 failures [scripts/smoke-tests/smoke-macos.sh:31-36]
- [x] [Review][Patch] `smoke-linux.sh`: no EXIT trap — Xvfb and app process leak if any step fails before STEP 4 [scripts/smoke-tests/smoke-linux.sh:41-57]
- [x] [Review][Patch] Windows: app not stopped before `msiexec /x` — locked files cause uninstall failure; `Start-Process` missing `-PassThru` so no handle exists to stop it [scripts/smoke-tests/smoke-windows.ps1:57,95]
- [x] [Review][Patch] Windows: `Write-Error` inside `Fail()` + `$ErrorActionPreference = "Stop"` — `exit 1` is unreachable; terminating error exit code is non-deterministic [scripts/smoke-tests/smoke-windows.ps1:20-22]
- [x] [Review][Patch] No `timeout-minutes` on any CI job — hung daemon runs 6h before GitHub kills it [.github/workflows/smoke-test.yml:11,33,62]
- [x] [Review][Patch] `smoke-macos.sh`: `open -a` exit code not checked and no post-launch process verification — app crash is indistinguishable from daemon timeout [scripts/smoke-tests/smoke-macos.sh:68]
- [x] [Review][Patch] `smoke-linux.sh`: `dpkg -i` dependency failures not auto-recovered — no `apt-get -f install` fallback [scripts/smoke-tests/smoke-linux.sh:35]
- [x] [Review][Patch] `smoke-linux.sh`: app exit not verified before `poll_health` — immediate crash causes slow 30s timeout with no early diagnostic [scripts/smoke-tests/smoke-linux.sh:56-57]
- [x] [Review][Defer] Xvfb `:99` port may be occupied on shared runner — pre-existing CI environment concern [scripts/smoke-tests/smoke-linux.sh:41] — deferred, pre-existing
- [x] [Review][Defer] Windows install dir hardcoded to `C:\Program Files\JellyfinSync` — WiX MSI default; low risk [scripts/smoke-tests/smoke-windows.ps1:44] — deferred, pre-existing
- [x] [Review][Defer] macOS DMG `.app` name assumed to match `APP_NAME` — controlled by tauri.conf.json; low risk [scripts/smoke-tests/smoke-macos.sh:53] — deferred, pre-existing
- [x] [Review][Defer] No automated post-release trigger for smoke workflow — intentional per spec constraints [.github/workflows/smoke-test.yml:3] — deferred, pre-existing

## Change Log

- 2026-05-03: Completed reopened T4 scope. Added exact MTP manual hardware verification note to smoke workflow and shared smoke helper; verified exact wording and full Rust test suite.
- 2026-04-30: Reopened — MTP support (Sprint Change 2026-04-30). AC #6 and T4 added. Requires Story 4.0 (DeviceIO abstraction) to provide mock MtpBackend unit tests.
- 2026-04-06: Implemented all tasks (T1–T3). Added daemon.health RPC endpoint with unit test; created platform smoke scripts (Windows/Linux/macOS + common helper); created smoke-test.yml GitHub Actions workflow with workflow_dispatch trigger.
- 2026-04-06: All 10 review patches applied. Status set to done.
