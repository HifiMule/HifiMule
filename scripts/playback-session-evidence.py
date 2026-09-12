#!/usr/bin/env python3
"""Run isolated playback-session evidence and always write a sanitized JSON record."""

from __future__ import annotations

import json
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
    started = datetime.now(timezone.utc)
    results = [subprocess.run(command, text=True) for command in commands]
    exit_code = next((result.returncode for result in results if result.returncode), 0)
    record = {
        "schemaVersion": 1,
        "recordedAt": datetime.now(timezone.utc).isoformat(),
        "startedAt": started.isoformat(),
        "os": platform.system(),
        "osRelease": platform.release(),
        "architecture": platform.machine(),
        "binaryRevision": revision(),
        "commands": [" ".join(command) for command in commands],
        "databaseScope": "isolated test databases only",
        "fixtures": FIXTURES,
        "outcome": "passed" if exit_code == 0 else "failed",
        "exitCode": exit_code,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record, indent=2))
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
