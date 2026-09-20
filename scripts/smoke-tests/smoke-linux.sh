#!/usr/bin/env bash
# smoke-linux.sh — Linux .deb smoke test for HifiMule
# Runs from the directory containing the .deb installer artifact.
# Requires: dpkg, Xvfb, curl
#
# Steps:
#   1. Silent .deb install via dpkg
#   2. Launch application via Xvfb (headless display)
#   3. Poll daemon health endpoint (30s timeout)
#   4. Uninstall via dpkg -r
#
# Exit code 0 = PASS, non-zero = FAIL with diagnostic output.

set -euo pipefail

PLATFORM="linux"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/smoke-common.sh"

XVFB_PID=""
APP_PID=""
DAEMON_PID=""

cleanup() {
    [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null || true
    [[ -n "$DAEMON_PID" ]] && kill "$DAEMON_PID" 2>/dev/null || true
    [[ -n "$XVFB_PID" ]] && kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT

start_xvfb() {
    if Xvfb -help 2>&1 | grep -q -- "-displayfd"; then
        local display_file
        display_file=$(mktemp)
        Xvfb -displayfd 1 -screen 0 1024x768x24 >"$display_file" 2>/tmp/hifimule-xvfb.log &
        XVFB_PID=$!
        for _ in {1..50}; do
            if [[ -s "$display_file" ]]; then
                DISPLAY=":$(cat "$display_file")"
                export DISPLAY
                rm -f "$display_file"
                return 0
            fi
            sleep 0.1
        done
        rm -f "$display_file"
        return 1
    fi

    for display_number in 99 100; do
        if Xvfb ":${display_number}" -screen 0 1024x768x24 >/tmp/hifimule-xvfb.log 2>&1 & then
            XVFB_PID=$!
            sleep 1
            if kill -0 "$XVFB_PID" 2>/dev/null; then
                DISPLAY=":${display_number}"
                export DISPLAY
                return 0
            fi
        fi
    done
    return 1
}

fail() {
    local step="$1"
    local message="$2"
    echo ""
    echo "FAIL [platform=${PLATFORM}] [step=${step}]: ${message}"
    exit 1
}

# --- STEP 1: Install ---
echo ""
echo "==> STEP 1: Installing .deb package ..."
DEB=$(ls *.deb 2>/dev/null | head -1 || true)
if [[ -z "$DEB" ]]; then
    fail "install" "No .deb file found in working directory: $(pwd)"
fi
echo "  Package: $DEB"
echo "  SHA256: $(sha256sum "$DEB" | awk '{print $1}')"
DEB_DEPENDS=$(dpkg-deb -f "$DEB" Depends || true)
DEB_PACKAGE=$(dpkg-deb -f "$DEB" Package || true)
echo "  Depends: $DEB_DEPENDS"
if [[ "$DEB_DEPENDS" != *libmtp* ]]; then
    fail "install" "Package Depends does not include libmtp runtime dependency"
fi
if [[ -z "$DEB_PACKAGE" ]]; then
    fail "install" "Package metadata does not declare a package name"
fi
if ! sudo dpkg -i "$DEB"; then
    sudo apt-get install -f -y || fail "install" "dpkg -i and dependency fix both failed"
fi
echo "  Install OK"

# --- STEP 2: Launch ---
echo ""
echo "==> STEP 2: Launching HifiMule via Xvfb ..."
start_xvfb || fail "launch" "Unable to start Xvfb on an available display"
echo "  DISPLAY: $DISPLAY"

# Use X11 software rendering without WebKit compositing for this Xvfb smoke run.
# Export once so every UI launch below inherits the same headless configuration.
export GDK_BACKEND=x11
export LIBGL_ALWAYS_SOFTWARE=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
echo "  GDK_BACKEND: $GDK_BACKEND"
echo "  LIBGL_ALWAYS_SOFTWARE: $LIBGL_ALWAYS_SOFTWARE"
echo "  WEBKIT_DISABLE_COMPOSITING_MODE: $WEBKIT_DISABLE_COMPOSITING_MODE"

# The installed binary name comes from productName in tauri.conf.json (lowercase on Linux)
APP_BIN="hifimule-ui"
if ! command -v "$APP_BIN" &>/dev/null; then
    # Fallback search in common install locations
    APP_BIN=$(find /usr/bin /usr/local/bin /opt -name "hifimule-ui" 2>/dev/null | head -1 || true)
    if [[ -z "$APP_BIN" ]]; then
        kill "$XVFB_PID" 2>/dev/null || true
        fail "launch" "Installed binary 'hifimule-ui' not found — check package manifest"
    fi
fi
echo "  Binary: $APP_BIN"
new_ui_smoke_id
"$APP_BIN" --smoke-id "$UI_SMOKE_ID" &
APP_PID=$!
sleep 1
if ! kill -0 "$APP_PID" 2>/dev/null; then
    fail "launch" "Application exited immediately after launch"
fi

# --- STEP 3: Daemon health poll ---
echo ""
echo "==> STEP 3: Polling daemon health (30s timeout) ..."
# shellcheck source=smoke-common.sh
source "${SCRIPT_DIR}/smoke-common.sh"
if ! poll_health 30; then
    kill "$APP_PID" "$XVFB_PID" 2>/dev/null || true
    fail "daemon-health" "Daemon did not respond with status=ok after 30s"
fi
poll_ui_ready 30 || fail "ui-hydration" "Installed UI failed to render authoritative state"
echo "  Daemon responded OK"

INITIAL_IDENTITY=$(lifecycle_identity)
DAEMON_PID=${INITIAL_IDENTITY%%$'\t'*}
assert_unauthenticated_access_rejected || fail "local-access" "Unauthenticated health request was not rejected"

echo "==> STEP 3a: Concurrent launch and UI close/reopen ..."
new_ui_smoke_id
"$APP_BIN" --smoke-id "$UI_SMOKE_ID" &
SECOND_UI_PID=$!
sleep 1
poll_health 15 || fail "concurrent-launch" "Concurrent UI lost the daemon"
poll_ui_ready 30 || fail "ui-hydration" "Installed UI failed to confirm attachment"
[[ "$(lifecycle_identity)" == "$INITIAL_IDENTITY" ]] || fail "concurrent-launch" "Daemon identity changed"
kill "$SECOND_UI_PID" 2>/dev/null || true
kill "$APP_PID" 2>/dev/null || true
APP_PID=""
sleep 1
kill -0 "$DAEMON_PID" 2>/dev/null || fail "close-ui" "Closing the UI stopped the daemon"
new_ui_smoke_id
"$APP_BIN" --smoke-id "$UI_SMOKE_ID" &
APP_PID=$!
poll_health 15 || fail "reopen-ui" "Reopened UI did not attach"
poll_ui_ready 30 || fail "ui-hydration" "Installed UI failed to confirm attachment"
[[ "$(lifecycle_identity)" == "$INITIAL_IDENTITY" ]] || fail "reopen-ui" "Reopen created a competing daemon"
echo "  Concurrent launch and close/reopen preserved PID and instance"

echo "==> STEP 3b: Crash recovery from stale discovery ..."
kill -9 "$DAEMON_PID" 2>/dev/null || fail "crash-recovery" "Unable to terminate the test-owned daemon"
for _ in {1..50}; do
    kill -0 "$DAEMON_PID" 2>/dev/null || break
    sleep 0.1
done
kill -0 "$DAEMON_PID" 2>/dev/null && fail "crash-recovery" "Test-owned daemon did not terminate"
kill "$APP_PID" 2>/dev/null || true
APP_PID=""
new_ui_smoke_id
"$APP_BIN" --smoke-id "$UI_SMOKE_ID" &
APP_PID=$!
poll_health 15 || fail "crash-recovery" "Replacement owner did not become ready"
poll_ui_ready 30 || fail "ui-hydration" "Installed UI failed to confirm attachment"
RECOVERED_IDENTITY=$(lifecycle_identity)
[[ "$RECOVERED_IDENTITY" != "$INITIAL_IDENTITY" ]] || fail "crash-recovery" "Replacement reused the stale PID and instance identity"
DAEMON_PID=${RECOVERED_IDENTITY%%$'\t'*}
echo "  Crash recovery replaced the stale owner identity"

# --- STEP 4: Uninstall ---
echo ""
echo "==> STEP 4: Uninstalling ..."
kill "$APP_PID" 2>/dev/null || true
kill "$DAEMON_PID" 2>/dev/null || true
kill "$XVFB_PID" 2>/dev/null || true
APP_PID=""
DAEMON_PID=""
XVFB_PID=""
sudo dpkg -r "$DEB_PACKAGE" || fail "uninstall" "dpkg -r failed with exit code $?"
if dpkg-query -W -f='${db:Status-Status}' "$DEB_PACKAGE" 2>/dev/null | grep -qx installed; then
    fail "uninstall" "Package $DEB_PACKAGE remains installed after dpkg -r"
fi
echo "  Uninstall OK"

echo ""
record_unverified_real_device_shutdown
echo "PASS: Linux smoke test complete (real-device active Quit remains UNVERIFIED)"
