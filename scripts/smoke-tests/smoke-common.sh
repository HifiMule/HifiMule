#!/usr/bin/env bash
# smoke-common.sh — Shared helpers for JellyfinSync smoke tests
# Source this file from platform-specific smoke scripts.

# poll_health <timeout_seconds>
# Polls http://127.0.0.1:19140 with a daemon.health JSON-RPC call.
# Returns 0 when the daemon responds with { "data": { "status": "ok" } }.
# Returns 1 (and prints diagnostics) if the daemon does not respond within timeout.
poll_health() {
    local timeout=$1
    local body='{"jsonrpc":"2.0","method":"daemon.health","params":{},"id":1}'
    local response

    for ((i = 0; i < timeout; i++)); do
        response=$(curl -sf -X POST http://127.0.0.1:19140 \
            -H "Content-Type: application/json" \
            -d "$body" 2>/dev/null || true)
        if echo "$response" | grep -q '"status":"ok"'; then
            return 0
        fi
        sleep 1
    done

    echo "DIAGNOSTIC [poll_health]: Daemon did not respond after ${timeout}s"
    echo "DIAGNOSTIC [poll_health]: Last curl attempt:"
    curl -v -X POST http://127.0.0.1:19140 \
        -H "Content-Type: application/json" \
        -d "$body" 2>&1 || true
    return 1
}
