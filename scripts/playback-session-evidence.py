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
    "unsupported-version evidence preservation",
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
    command = [
        "cargo",
        "test",
        "-p",
        "hifimule-daemon",
        "playback::session::tests",
        "--",
        "--nocapture",
    ]
    started = datetime.now(timezone.utc)
    result = subprocess.run(command, text=True)
    record = {
        "schemaVersion": 1,
        "recordedAt": datetime.now(timezone.utc).isoformat(),
        "startedAt": started.isoformat(),
        "os": platform.system(),
        "osRelease": platform.release(),
        "architecture": platform.machine(),
        "binaryRevision": revision(),
        "command": " ".join(command),
        "databaseScope": "isolated test databases only",
        "fixtures": FIXTURES,
        "outcome": "passed" if result.returncode == 0 else "failed",
        "exitCode": result.returncode,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record, indent=2))
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
