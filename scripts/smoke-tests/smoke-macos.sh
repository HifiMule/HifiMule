#!/usr/bin/env bash
# smoke-macos.sh — macOS DMG smoke test for HifiMule
# Runs from the directory containing the .dmg installer artifact.
# Requires: hdiutil, xattr, open, curl
#
# Steps:
#   1. Mount DMG and copy .app to /Applications
#   2. Remove quarantine attribute (required for unsigned builds on CI)
#   3. Launch application
#   4. Poll daemon health endpoint (30s timeout)
#   5. Kill app and remove .app from /Applications
#
# Exit code 0 = PASS, non-zero = FAIL with diagnostic output.

set -euo pipefail

PLATFORM="macos"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/smoke-common.sh"
MOUNT_POINT="/Volumes/HifiMule"
APP_NAME=""
APP_PATH=""
DAEMON_PID=""
INSTALL_ROOT="${HIFIMULE_SMOKE_INSTALL_ROOT:-/Applications}"

fail() {
    local step="$1"
    local message="$2"
    echo ""
    echo "FAIL [platform=${PLATFORM}] [step=${step}]: ${message}"
    exit 1
}

# Match the executable literally: pgrep -f would treat the app path as a regex
# and could match unrelated command lines. comm excludes potentially secret args.
installed_ui_pids() {
    local pid executable
    ps -axww -o pid=,comm= | while read -r pid executable; do
        if [[ "$executable" == "$APP_PATH/Contents/MacOS/hifimule-ui" ]]; then
            printf '%s\n' "$pid"
        fi
    done
}

ui_diagnostics() {
    local stage="$1" pid
    echo "DIAGNOSTIC [ui] stage=$stage smokeId=${UI_SMOKE_ID:-unset}"
    for pid in $(installed_ui_pids); do
        ps -p "$pid" -o pid=,ppid=,stat=,etime=,comm= || true
    done
}

close_installed_ui() {
    local stage="$1" pid remaining
    local deadline=$((SECONDS + 10))
    for pid in $(installed_ui_pids); do
        kill "$pid" 2>/dev/null || true
    done
    while true; do
        remaining=$(installed_ui_pids)
        [[ -z "$remaining" ]] && return 0
        if (( SECONDS >= deadline )); then
            ui_diagnostics "$stage"
            fail "$stage" "Installed UI did not terminate within 10s"
        fi
        sleep 0.25
    done
}

launch_installed_ui() {
    local stage="$1"
    new_ui_smoke_id
    open -n "$APP_PATH" --args --smoke-id "$UI_SMOKE_ID" || {
        ui_diagnostics "$stage"
        fail "$stage" "Could not launch a fresh installed UI"
    }
}

require_ui_ready() {
    local stage="$1"
    poll_ui_ready 30 || {
        ui_diagnostics "$stage"
        fail "ui-hydration" "Installed UI failed to confirm attachment at $stage"
    }
}

cleanup() {
    echo "  Cleaning up ..."
    local pid
    if [[ -n "$APP_PATH" ]]; then
        for pid in $(installed_ui_pids); do
            kill "$pid" 2>/dev/null || true
        done
    fi
    [[ -n "$DAEMON_PID" ]] && kill "$DAEMON_PID" 2>/dev/null || true
    hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
    [[ -n "$APP_PATH" ]] && rm -rf "$APP_PATH" 2>/dev/null || true
}
trap cleanup EXIT

# --- STEP 1: Mount DMG and install ---
echo ""
echo "==> STEP 1: Mounting DMG and installing .app ..."
DMG=$(ls *.dmg 2>/dev/null | head -1 || true)
if [[ -z "$DMG" ]]; then
    fail "install" "No .dmg file found in working directory: $(pwd)"
fi
echo "  Installer: $DMG"
echo "  SHA256: $(shasum -a 256 "$DMG" | awk '{print $1}')"

# Detach existing mount if stale
hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

hdiutil attach "$DMG" -mountpoint "$MOUNT_POINT" -nobrowse -quiet ||
    fail "install" "hdiutil attach failed"

APP_IN_DMG=$(find "$MOUNT_POINT" -maxdepth 1 -name "*.app" -print -quit)
if [[ -z "$APP_IN_DMG" ]]; then
    fail "install" "No .app found at DMG mount point: $MOUNT_POINT"
fi
APP_NAME="$(basename "$APP_IN_DMG" .app)"
mkdir -p "$INSTALL_ROOT"
INSTALL_ROOT=$(cd "$INSTALL_ROOT" && pwd -P)
APP_PATH="${INSTALL_ROOT}/${APP_NAME}.app"

cp -R "$APP_IN_DMG" "$INSTALL_ROOT/" ||
    fail "install" "Failed to copy .app to $INSTALL_ROOT"

hdiutil detach "$MOUNT_POINT" -quiet || true
echo "  Install OK"

# --- STEP 2: Remove quarantine (unsigned builds on CI) ---
echo ""
echo "==> STEP 2: Removing quarantine attribute ..."
xattr -d com.apple.quarantine "$APP_PATH" 2>/dev/null || true
echo "  Quarantine removed (or not present)"

# --- STEP 3: Launch ---
echo ""
echo "==> STEP 3: Launching ${APP_NAME} ..."
launch_installed_ui "launch"
# Give Tauri time to spawn the daemon sidecar
sleep 3
if [[ -z "$(installed_ui_pids)" ]]; then
    ui_diagnostics "launch"
    fail "launch" "Application process not found after launch — may have crashed immediately"
fi
echo "  Launch triggered"

# --- STEP 4: Daemon health poll ---
echo ""
echo "==> STEP 4: Polling daemon health (30s timeout) ..."
# shellcheck source=smoke-common.sh
source "${SCRIPT_DIR}/smoke-common.sh"
if ! poll_health 30; then
    cleanup
    fail "daemon-health" "Daemon did not respond with status=ok after 30s"
fi
require_ui_ready "initial-launch"
echo "  Daemon responded OK"

INITIAL_IDENTITY=$(lifecycle_identity)
DAEMON_PID=${INITIAL_IDENTITY%%$'\t'*}
assert_unauthenticated_access_rejected || fail "local-access" "Unauthenticated health request was not rejected"

echo "==> STEP 4a: Concurrent launch and UI close/reopen ..."
launch_installed_ui "concurrent-launch"
poll_health 15 || fail "concurrent-launch" "Concurrent launch lost the daemon"
require_ui_ready "concurrent-launch"
[[ "$(lifecycle_identity)" == "$INITIAL_IDENTITY" ]] || fail "concurrent-launch" "Daemon identity changed"
close_installed_ui "close-ui"
kill -0 "$DAEMON_PID" 2>/dev/null || fail "close-ui" "Closing the UI stopped the daemon"
launch_installed_ui "reopen-ui"
poll_health 15 || fail "reopen-ui" "Reopened UI did not attach"
require_ui_ready "reopen-ui"
[[ "$(lifecycle_identity)" == "$INITIAL_IDENTITY" ]] || fail "reopen-ui" "Reopen created a competing daemon"
echo "  Concurrent launch and close/reopen preserved PID and instance"

echo "==> STEP 4b: Daemon crash recovery ..."
kill -9 "$DAEMON_PID" || fail "crash-recovery" "Could not terminate the original daemon"
for _ in $(seq 1 20); do
    ! kill -0 "$DAEMON_PID" 2>/dev/null && break
    sleep 0.25
done
kill -0 "$DAEMON_PID" 2>/dev/null && fail "crash-recovery" "Original daemon did not terminate"
close_installed_ui "crash-recovery-close-ui"
launch_installed_ui "crash-recovery"
poll_health 30 || fail "crash-recovery" "UI did not recover a fresh authenticated daemon"
require_ui_ready "crash-recovery"
RECOVERED_IDENTITY=$(lifecycle_identity)
[[ "$RECOVERED_IDENTITY" != "$INITIAL_IDENTITY" ]] || fail "crash-recovery" "Recovered daemon retained stale identity"
DAEMON_PID=${RECOVERED_IDENTITY%%$'\t'*}
echo "  Crash recovery published a new PID and instance"

# --- STEP 5: Remove app ---
echo ""
echo "==> STEP 5: Removing installed app ..."
close_installed_ui "uninstall-close-ui"
rm -rf "$APP_PATH" || fail "uninstall" "Failed to remove $APP_PATH"
echo "  Removal OK"

echo ""
record_unverified_real_device_shutdown
echo "PASS: macOS smoke test complete (real-device active Quit remains UNVERIFIED)"
