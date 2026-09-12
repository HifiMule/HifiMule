#!/usr/bin/env python3
"""Run isolated playback-session evidence and always write a sanitized JSON record."""

from __future__ import annotations

import json
import hashlib
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


FIXTURES = [
    "paused round trip with deliberate repeats",
    "explicit clear restores idle",
    "offline source restoration and availability",
    "stale queue revision and command-id reuse",
    "generation-fenced position checkpoint",
    "revision-bound bounded paging",
    "10,000-occurrence real-file restart",
    "bounded append and indexed last-occurrence selection",
    "unsupported-version evidence preservation",
    "64-request mailbox saturation and retryable overflow",
    "shutdown control under saturation and dropped-caller admission",
    "transactional migration rollback and checkpoint retry",
    "abrupt child-process termination during SQLite transaction",
    "authenticated playback router and exact JSON/error contract",
    "shutdown checkpoint failure visibility and authorized retry",
    "committed shutdown cancellation during stalled session storage",
    "owner-bound checkpoint retry and responsive health during blocked reads",
    "malformed restore retry, camelCase selection, coalesced progress and response recovery",
]


def revision() -> str:
    configured = os.environ.get("GITHUB_SHA")
    if configured:
        return configured
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def main() -> int:
    output = Path(os.environ.get("HIFIMULE_EVIDENCE_PATH", "playback-evidence.json"))
    commands = [
        [
            "cargo", "test", "-p", "hifimule-daemon",
            "playback::session::tests", "--", "--nocapture",
        ],
        [
            "cargo", "test", "-p", "hifimule-daemon",
            "playback::persistence::tests", "--", "--nocapture",
        ],
        [
            "cargo", "test", "-p", "hifimule-daemon",
            "playback_contract_is_exact_bounded_offline_and_conflict_shaped",
            "--", "--nocapture",
        ],
        [
            "cargo", "test", "-p", "hifimule-daemon",
            "production_router_rejects_missing_token_before_rpc_dispatch",
            "--", "--nocapture",
        ],
        [
            "cargo", "test", "-p", "hifimule-daemon",
            "playback_checkpoint_failure_surfaces_and_retry_preserves_shutdown_contract",
            "--", "--nocapture",
        ],
    ]
    for fixture in [
        "stalled_playback_checkpoint_does_not_delay_sync_cancellation_or_health",
        "playback_retry_checkpoint_router_is_authenticated_owner_bound_and_shutdown_scoped",
        "playback_read_waiting_for_database_does_not_block_health_executor",
        "playback_join_failure_is_not_advertised_as_a_checkpoint_retry",
    ]:
        commands.append(["cargo", "test", "-p", "hifimule-daemon", fixture, "--", "--nocapture"])
    started = datetime.now(timezone.utc)
    results = [subprocess.run(command, text=True) for command in commands]
    exit_code = next((result.returncode for result in results if result.returncode), 0)
    source_diff = subprocess.run(
        ["git", "diff", "--binary", "HEAD", "--", "Cargo.toml", "Cargo.lock",
         "hifimule-daemon", "hifimule-lifecycle", "hifimule-ui/src-tauri",
         "hifimule-ui/src", "hifimule-i18n", "scripts/playback-session-evidence.py"],
        check=True, capture_output=True,
    ).stdout
    record = {
        "schemaVersion": 1,
        "recordedAt": datetime.now(timezone.utc).isoformat(),
        "startedAt": started.isoformat(),
        "os": platform.system(),
        "osRelease": platform.release(),
        "architecture": platform.machine(),
        "binaryRevision": revision(),
        "workingTreeDirty": bool(source_diff),
        "sourceDiffSha256": hashlib.sha256(source_diff).hexdigest() if source_diff else None,
        "commands": [" ".join(command) for command in commands],
        "databaseScope": "isolated test databases only",
        "fixtures": FIXTURES,
        "outcome": "passed" if exit_code == 0 else "failed",
        "exitCode": exit_code,
        "commandExitCodes": [result.returncode for result in results],
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record, indent=2))
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
