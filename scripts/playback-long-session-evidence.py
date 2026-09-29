#!/usr/bin/env python3
"""Create and validate fail-closed Story 16.14 sustained-session evidence."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

CONTRACT_VERSION = 1
OUTCOMES = {"PASS", "FAIL", "BLOCKED", "NOT RUN"}
ROWS = {
    ("windows-x64", "msi"), ("windows-x64", "nsis"),
    ("linux-x64", "deb"), ("linux-x64", "appimage"),
    ("macos-x64", "dmg"), ("macos-arm64", "dmg"),
}
PROFILES = {"idle", "album", "radio", "playback-only", "sync-only", "combined"}
BUDGET_LIMITS = {
    "compressedPerSourceHighWaterBytes": 8_388_608,
    "compressedAggregateHighWaterBytes": 16_777_216,
    "pcmPerSourceHighWaterBytes": 1_048_576,
    "pcmAggregateHighWaterBytes": 2_097_152,
    "automaticUpcomingHighWater": 5,
    "adaptationSamplesPerScopeHighWater": 16,
    "adaptationScopesHighWater": 64,
    "historyPageRowsHighWater": 200,
    "restoreBatchRowsHighWater": 200,
    "retainedUiHistoryRowsHighWater": 400,
    "syncStagedTracksHighWater": 2,
    "syncStagedBytesHighWater": 2_147_483_648,
    "cleanupLatencySeconds": 15,
}
FORBIDDEN_KEYS = re.compile(r"token|authorization|credential|password|providerResponse|rawServerId", re.I)
FORBIDDEN_VALUES = re.compile(r"https?://|(?:/Users/|/home/|[A-Za-z]:\\Users\\)")


def template(target: str, package_format: str) -> dict:
    return {
        "schemaVersion": 1, "contractVersion": CONTRACT_VERSION,
        "target": target, "packageFormat": package_format,
        "artifact": {"sha256": "", "sourceRevision": "", "immutableUri": ""},
        "environment": {"physicalOrVm": "", "osVersion": "", "architecture": ""},
        "runtime": {"application": "", "daemon": "", "rust": "", "ffmpegBindings": "9.0.0",
                    "loadedFfmpeg": {}, "manifestMatch": "NOT RUN"},
        "profiles": {name: {"outcome": "NOT RUN", "rawEvidenceUri": "", "samples": []}
                     for name in sorted(PROFILES)},
        "faults": {name: "NOT RUN" for name in (
            "source-stall", "source-outage", "ambiguous-write", "seek", "preview-return",
            "session-replacement", "output-loss-switch", "sleep-wake", "orderly-quit",
            "abrupt-exit", "killed-mid-checkpoint")},
        "epic16Matrix": {f"16.{number}": "NOT RUN" for number in range(1, 14)},
        "limitations": [], "decision": "BLOCKED",
    }


def _sanitize(value, path="record") -> list[str]:
    errors = []
    if isinstance(value, dict):
        for key, child in value.items():
            if FORBIDDEN_KEYS.search(str(key)):
                errors.append(f"forbidden secret/raw field at {path}.{key}")
            errors.extend(_sanitize(child, f"{path}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            errors.extend(_sanitize(child, f"{path}[{index}]"))
    elif isinstance(value, str) and FORBIDDEN_VALUES.search(value):
        errors.append(f"unsafe URL or local profile path at {path}")
    return errors


def validate(record: dict) -> list[str]:
    errors = _sanitize(record)
    if record.get("schemaVersion") != 1 or record.get("contractVersion") != CONTRACT_VERSION:
        errors.append("unsupported schema or contract version")
    row = (record.get("target"), record.get("packageFormat"))
    if row not in ROWS:
        errors.append("target/packageFormat is not a frozen artifact row")
    decision = record.get("decision")
    if decision not in OUTCOMES:
        errors.append("decision is invalid")
    profiles = record.get("profiles", {})
    if set(profiles) != PROFILES:
        errors.append("all frozen profiles are required exactly once")
    else:
        for name, profile in profiles.items():
            outcome = profile.get("outcome")
            if outcome not in OUTCOMES:
                errors.append(f"profile {name} outcome is invalid")
            if outcome == "PASS":
                if not str(profile.get("rawEvidenceUri", "")).startswith("ci-artifact://"):
                    errors.append(f"profile {name} PASS lacks immutable raw evidence")
                if name in {"album", "radio"} and profile.get("durationSeconds", 0) < 28_800:
                    errors.append(f"profile {name} is shorter than the frozen sustained duration")
                if profile.get("warmupSeconds") != 300 or profile.get("sampleIntervalSeconds") != 5:
                    errors.append(f"profile {name} does not use frozen sampling")
                if profile.get("repetitions", 0) < 3:
                    errors.append(f"profile {name} lacks three repetitions")
                budgets = profile.get("budgets", {})
                for key, limit in BUDGET_LIMITS.items():
                    value = budgets.get(key)
                    if type(value) not in (int, float) or value < 0 or value > limit:
                        errors.append(f"profile {name} {key} exceeds or omits frozen limit {limit}")
    statuses = list(record.get("faults", {}).values()) + list(record.get("epic16Matrix", {}).values())
    if any(status not in OUTCOMES for status in statuses):
        errors.append("fault and Epic 16 matrix outcomes must use frozen result semantics")
    if decision == "PASS":
        if any(profile.get("outcome") != "PASS" for profile in profiles.values()):
            errors.append("PASS requires every profile to pass")
        if any(status != "PASS" for status in statuses):
            errors.append("PASS requires every fault and Epic 16 workflow to pass")
        artifact = record.get("artifact", {})
        if not re.fullmatch(r"[0-9a-f]{64}", str(artifact.get("sha256", ""))):
            errors.append("PASS requires artifact SHA-256")
        if not str(artifact.get("immutableUri", "")).startswith("ci-artifact://"):
            errors.append("PASS requires immutable artifact URI")
        history = record.get("history")
        if history is not None and not (
            history.get("fixtureRows", 0) >= 10_000
            and history.get("visitedRows") == history.get("fixtureRows")
            and history.get("uniqueRows") == history.get("fixtureRows")
            and history.get("stableOrder") is True
            and history.get("activeExclusionsPreserved") is True
            and history.get("ambiguousOperationsPreserved") is True
        ):
            errors.append("history continuity is incomplete or unbounded")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("template")
    create.add_argument("--target", required=True)
    create.add_argument("--package-format", required=True)
    create.add_argument("--output", type=Path, required=True)
    check = sub.add_parser("validate")
    check.add_argument("path", type=Path)
    args = parser.parse_args()
    if args.command == "template":
        record = template(args.target, args.package_format)
        errors = validate(record)
        if any("artifact row" in error for error in errors):
            print("\n".join(errors), file=sys.stderr)
            return 2
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        return 0
    record = json.loads(args.path.read_text(encoding="utf-8"))
    errors = validate(record)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"valid Story 16.14 evidence: {record['target']} {record['packageFormat']} {record['decision']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
