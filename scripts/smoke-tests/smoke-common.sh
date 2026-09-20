#!/usr/bin/env bash
# smoke-common.sh — Shared helpers for HifiMule smoke tests
# Source this file from platform-specific smoke scripts.
#
# MTP end-to-end detection requires manual hardware verification on each platform. Automated MTP IO coverage is provided by unit tests in device_io.rs.

# poll_health <timeout_seconds>
# Discovers the private dynamic endpoint and authenticates without putting the
# owner token in process arguments, logs, or test output.
# Returns 1 (and prints diagnostics) if the daemon does not respond within timeout.
poll_health() {
    local timeout=$1
    local started=$SECONDS
    local descriptor
    descriptor=$(lifecycle_descriptor_path)

    for ((i = 0; i < timeout; i++)); do
        if python3 - "$descriptor" <<'PY' >/dev/null 2>&1
import json, pathlib, sys, urllib.request
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None
descriptor = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert descriptor["schemaVersion"] == 1 and descriptor["protocolVersion"] == 1
request = urllib.request.Request(
    f'http://127.0.0.1:{descriptor["port"]}',
    data=b'{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}',
    headers={"Content-Type": "application/json", "Authorization": f'Bearer {descriptor["token"]}'},
    method="POST",
)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
with opener.open(request, timeout=2) as response:
    result = json.load(response)["result"]["data"]
assert result["status"] == "ok"
assert result["protocolVersion"] == 1
assert result["instanceId"] == descriptor["instanceId"]
PY
        then
            echo "LIFECYCLE_EVIDENCE os=$(uname -s) architecture=$(uname -m) elapsedSeconds=$((SECONDS - started))"
            jq -c '{schemaVersion,protocolVersion,instanceId,pid,port,launchGeneration}' "$descriptor"
            return 0
        fi
        sleep 1
    done

    echo "DIAGNOSTIC [poll_health]: Daemon did not respond after ${timeout}s"
    if [[ -f "$descriptor" ]]; then
        jq '{schemaVersion,protocolVersion,instanceId,pid,port,launchGeneration}' "$descriptor" || true
    else
        echo "DIAGNOSTIC [poll_health]: owner descriptor was not published"
    fi
    return 1
}

lifecycle_descriptor_path() {
    if [[ -n "${HIFIMULE_APP_DATA_DIR:-}" ]]; then
        printf '%s\n' "$HIFIMULE_APP_DATA_DIR/runtime/owner.json"
    elif [[ "$(uname -s)" == "Darwin" ]]; then
        printf '%s\n' "$HOME/Library/Application Support/HifiMule/runtime/owner.json"
    else
        printf '%s\n' "${XDG_DATA_HOME:-$HOME/.local/share}/HifiMule/runtime/owner.json"
    fi
}

lifecycle_identity() {
    jq -r '[.pid,.instanceId] | @tsv' "$(lifecycle_descriptor_path)"
}

assert_unauthenticated_access_rejected() {
    python3 - "$(lifecycle_descriptor_path)" <<'PY'
import json, pathlib, sys, urllib.error, urllib.request
descriptor = json.loads(pathlib.Path(sys.argv[1]).read_text())
request = urllib.request.Request(
    f'http://127.0.0.1:{descriptor["port"]}',
    data=b'{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}',
    headers={"Content-Type": "application/json"}, method="POST")
try:
    urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=2)
except urllib.error.HTTPError as error:
    assert error.code == 401
else:
    raise AssertionError("unauthenticated lifecycle access was accepted")
PY
}

# Each real UI launch receives a fresh non-secret marker. The main webview writes
# its acknowledgment only after rendering state returned by the native RPC proxy.
new_ui_smoke_id() {
    UI_SMOKE_ID=$(python3 -c 'import uuid; print(uuid.uuid4())')
}

poll_ui_ready() {
    local timeout=$1
    local descriptor
    descriptor=$(lifecycle_descriptor_path)
    python3 - "$descriptor" "$UI_SMOKE_ID" "$timeout" "${2:-}" <<'PYCODE'
import json, os, pathlib, sys, time
owner_path = pathlib.Path(sys.argv[1])
marker = sys.argv[2]
expected_pid = int(sys.argv[4]) if sys.argv[4] else None
ready_path = owner_path.parent / f"ui-ready-{marker}.json"
deadline = time.monotonic() + int(sys.argv[3])
while time.monotonic() < deadline:
    if expected_pid is not None:
        try:
            os.kill(expected_pid, 0)
        except ProcessLookupError:
            print(f"Expected UI process {expected_pid} exited before confirming hydration", file=sys.stderr)
            sys.exit(1)
    try:
        ready = json.loads(ready_path.read_text())
        owner = json.loads(owner_path.read_text())
        assert ready["smokeId"] == marker and ready["state"] == "hydrated"
        assert expected_pid is None or ready["uiPid"] == expected_pid
        assert (ready["daemonPid"], ready["instanceId"]) == (owner["pid"], owner["instanceId"])
        os.kill(ready["uiPid"], 0)
        print("UI_ATTACHMENT_EVIDENCE " + json.dumps(ready))
        ready_path.unlink()
        sys.exit(0)
    except (OSError, ValueError, KeyError, AssertionError):
        time.sleep(0.25)
print("UI did not confirm current-state hydration", file=sys.stderr)
sys.exit(1)
PYCODE
}

# Distinct evidence that a reopened installed UI rendered the authoritative
# shutdown operation. This must never be satisfied by ordinary hydration.
poll_shutdown_rendered() {
    local timeout=$1
    local expected_shutdown_id=$2
    local descriptor
    descriptor=$(lifecycle_descriptor_path)
    python3 - "$descriptor" "$UI_SMOKE_ID" "$expected_shutdown_id" "$timeout" <<'PYCODE'
import json, os, pathlib, sys, time
owner_path = pathlib.Path(sys.argv[1])
marker, expected = sys.argv[2], sys.argv[3]
rendered_path = owner_path.parent / f"shutdown-rendered-{marker}.json"
deadline = time.monotonic() + int(sys.argv[4])
while time.monotonic() < deadline:
    try:
        rendered = json.loads(rendered_path.read_text())
        owner = json.loads(owner_path.read_text())
        assert rendered["smokeId"] == marker and rendered["state"] == "shutdownRendered"
        assert rendered["shutdownId"] == expected
        assert (rendered["daemonPid"], rendered["instanceId"]) == (owner["pid"], owner["instanceId"])
        os.kill(rendered["uiPid"], 0)
        print("SHUTDOWN_UI_EVIDENCE " + json.dumps(rendered))
        rendered_path.unlink()
        sys.exit(0)
    except (OSError, ValueError, KeyError, AssertionError):
        time.sleep(0.25)
print("UI did not confirm authoritative shutdown rendering", file=sys.stderr)
sys.exit(1)
PYCODE
}

record_unverified_real_device_shutdown() {
    echo "DEVICE_SHUTDOWN_EVIDENCE os=$(uname -s) architecture=$(uname -m) status=UNVERIFIED transport=UNVERIFIED artifact=installed shutdownId=UNVERIFIED elapsedMs=UNVERIFIED outcome=UNVERIFIED integrity=UNVERIFIED ownershipCleanup=UNVERIFIED reason=no-real-device-or-human-tray-session"
}
