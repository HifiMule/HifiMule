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
    if [[ "$(uname -s)" == "Darwin" ]]; then
        descriptor="$HOME/Library/Application Support/HifiMule/runtime/owner.json"
    else
        descriptor="${XDG_DATA_HOME:-$HOME/.local/share}/HifiMule/runtime/owner.json"
    fi

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
    if [[ "$(uname -s)" == "Darwin" ]]; then
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
