#!/usr/bin/env bash
# smoke-linux.sh — Linux .deb smoke test for JellyfinSync
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

XVFB_PID=""
APP_PID=""

cleanup() {
    [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null || true
    [[ -n "$XVFB_PID" ]] && kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT

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
sudo dpkg -i "$DEB" || sudo apt-get install -f -y || fail "install" "dpkg -i and dependency fix both failed"
echo "  Install OK"

# --- STEP 2: Launch ---
echo ""
echo "==> STEP 2: Launching JellyfinSync via Xvfb ..."
Xvfb :99 -screen 0 1024x768x24 &
XVFB_PID=$!
export DISPLAY=:99

# The installed binary name comes from productName in tauri.conf.json (lowercase on Linux)
APP_BIN="jellyfinsync"
if ! command -v "$APP_BIN" &>/dev/null; then
    # Fallback search in common install locations
    APP_BIN=$(find /usr/bin /usr/local/bin /opt -name "jellyfinsync" 2>/dev/null | head -1 || true)
    if [[ -z "$APP_BIN" ]]; then
        kill "$XVFB_PID" 2>/dev/null || true
        fail "launch" "Installed binary 'jellyfinsync' not found — check package manifest"
    fi
fi
echo "  Binary: $APP_BIN"
"$APP_BIN" &
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
echo "  Daemon responded OK"

# --- STEP 4: Uninstall ---
echo ""
echo "==> STEP 4: Uninstalling ..."
kill "$APP_PID" 2>/dev/null || true
kill "$XVFB_PID" 2>/dev/null || true
APP_PID=""
XVFB_PID=""
# Package name from productName (lowercase)
sudo dpkg -r jellyfinsync || fail "uninstall" "dpkg -r failed with exit code $?"
echo "  Uninstall OK"

echo ""
echo "PASS: Linux smoke test complete"
