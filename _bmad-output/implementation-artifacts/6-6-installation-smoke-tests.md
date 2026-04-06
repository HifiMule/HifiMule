# Story 6.6: Installation Smoke Tests

Status: ready-for-dev

## Story

As a **System Admin (Alexis)**,
I want basic smoke tests that verify each installer produces a working application,
So that I can catch packaging regressions before releasing.

## Acceptance Criteria

1. **Install step succeeds**: Given a freshly built installer for any platform, the installer runs to completion without errors.
2. **Launch step succeeds**: The installed application launches (daemon process starts) after installation.
3. **Daemon health-check passes**: The daemon sidecar is reachable at `http://127.0.0.1:19140` and responds to a JSON-RPC `get_daemon_state` call with a valid result (not an error).
4. **Uninstall step succeeds**: The application uninstalls cleanly, leaving no residual processes.
5. **Failures are diagnostic**: Any step failure produces clear diagnostic output identifying which platform, which step, and the error message.

## Tasks / Subtasks

- [ ] **T1: Add `daemon.health` RPC endpoint to daemon** (AC: #3)
  - [ ] T1.1: In `jellyfinsync-daemon/src/rpc.rs`, add `"daemon.health"` to the match dispatch (line ~169) calling a new `handle_daemon_health()` handler
  - [ ] T1.2: `handle_daemon_health()` returns `Ok(serde_json::json!({ "status": "ok" }))` — no state needed, pure connectivity probe
  - [ ] T1.3: Add a unit test in the existing `#[cfg(test)]` block in `rpc.rs` verifying the handler returns `{ "status": "ok" }`

- [ ] **T2: Create platform smoke-test scripts** (AC: #1–#5)
  - [ ] T2.1: Create `scripts/smoke-tests/smoke-windows.ps1` — MSI install (silent), launch daemon, poll `daemon.health`, uninstall (silent)
  - [ ] T2.2: Create `scripts/smoke-tests/smoke-linux.sh` — `.deb` install via `dpkg -i`, launch with Xvfb, poll `daemon.health`, uninstall via `dpkg -r`
  - [ ] T2.3: Create `scripts/smoke-tests/smoke-macos.sh` — mount DMG, copy `.app` to `/Applications`, bypass quarantine, launch, poll `daemon.health`, remove `.app`
  - [ ] T2.4: Create `scripts/smoke-tests/smoke-common.sh` (sourced by Linux/macOS scripts) — shared `poll_health` function: POST JSON-RPC to port 19140, retry up to 30s with 1s intervals, exit 1 with diagnostic output on timeout

- [ ] **T3: Create `smoke-test.yml` GitHub Actions workflow** (AC: #1–#5)
  - [ ] T3.1: Create `.github/workflows/smoke-test.yml` with `workflow_dispatch` trigger (manual run with input: `release_tag`)
  - [ ] T3.2: Three jobs: `smoke-windows`, `smoke-linux`, `smoke-macos` — each on the matching runner
  - [ ] T3.3: Each job downloads the installer artifact for its platform from the GitHub Release identified by `release_tag`
  - [ ] T3.4: Each job calls the matching platform script from `scripts/smoke-tests/`
  - [ ] T3.5: Each job uploads a smoke-test log as a workflow artifact on failure
  - [ ] T3.6: Linux job installs Xvfb and GTK runtime libs before running the smoke script

## Dev Notes

### What to Build (Scope)

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

### Completion Notes List

### File List
