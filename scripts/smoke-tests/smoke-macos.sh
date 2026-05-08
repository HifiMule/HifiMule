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
MOUNT_POINT="/Volumes/HifiMule"
APP_NAME=""
APP_PATH=""

fail() {
    local step="$1"
    local message="$2"
    echo ""
    echo "FAIL [platform=${PLATFORM}] [step=${step}]: ${message}"
    exit 1
}

cleanup() {
    echo "  Cleaning up ..."
    [[ -n "$APP_NAME" ]] && pkill -f "$APP_NAME" 2>/dev/null || true
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

# Detach existing mount if stale
hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

hdiutil attach "$DMG" -mountpoint "$MOUNT_POINT" -nobrowse -quiet ||
    fail "install" "hdiutil attach failed"

APP_IN_DMG=$(find "$MOUNT_POINT" -maxdepth 1 -name "*.app" -print -quit)
if [[ -z "$APP_IN_DMG" ]]; then
    fail "install" "No .app found at DMG mount point: $MOUNT_POINT"
fi
APP_NAME="$(basename "$APP_IN_DMG" .app)"
APP_PATH="/Applications/${APP_NAME}.app"

cp -R "$APP_IN_DMG" /Applications/ ||
    fail "install" "Failed to copy .app to /Applications"

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
open -a "$APP_NAME" || fail "launch" "open -a $APP_NAME failed"
# Give Tauri time to spawn the daemon sidecar
sleep 3
if ! pgrep -f "$APP_NAME" >/dev/null 2>&1; then
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
echo "  Daemon responded OK"

# --- STEP 5: Remove app ---
echo ""
echo "==> STEP 5: Removing installed app ..."
pkill -f "$APP_NAME" 2>/dev/null || true
sleep 1
rm -rf "$APP_PATH" || fail "uninstall" "Failed to remove $APP_PATH"
echo "  Removal OK"

echo ""
echo "PASS: macOS smoke test complete"
