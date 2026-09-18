#!/usr/bin/env python3
"""Collect and validate sanitized Stories 15.4–15.7 installed-playback evidence."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import platform
import re
import subprocess
import sys
import urllib.request


REQUIRED_TARGETS = ("windows-x64", "linux-x64", "macos-x64", "macos-arm64")
PULSE_MAX_BUFFER_BYTES = 48_000 * 2 * 4 // 10  # 100 ms, stereo float32
FIXTURES = ("wav", "flac", "alac-m4a", "mp3", "aac-m4a", "opus")
SCENARIOS = (
    "play-pause-resume-stop",
    "restored-resume",
    "replacement-during-loading",
    "window-closure",
    "output-loss",
    "quit-while-playing",
    "quit-while-loading",
    "long-slow-stream-bounds",
    "output-switch-playing-paused",
    "output-replug-default-change",
    "output-absent-startup",
    "output-failed-open-duplicate-names",
    "output-sleep-wake-shared-audio",
)
SCENARIO_GUIDANCE = {
    "play-pause-resume-stop": "Play a configured-server track, then Pause, Resume and Stop.",
    "restored-resume": "Pause after progress, Quit, relaunch, verify paused restoration, then Resume.",
    "replacement-during-loading": "Play a slow-loading track, immediately play another, and verify only the second is published/audible.",
    "window-closure": "Close every UI window during playback, wait, reopen, and verify playback continued without a new Play.",
    "output-loss": "Invalidate the active endpoint; verify OUTPUT_LOST and no migration, then explicitly Resume after restoring output.",
    "quit-while-playing": "Quit while playing; verify immediate silence, complete exit, no worker, and paused relaunch.",
    "quit-while-loading": "Quit while loading; verify cancellation, complete exit, no worker, and paused relaunch.",
    "long-slow-stream-bounds": "Play a long/slow stream; exercise starvation and replacements, then verify recovery and cleanup.",
    "output-switch-playing-paused": "Switch between two explicit outputs while playing and paused; retain occurrence/position, measure interruption, and verify no overlapping audible output.",
    "output-replug-default-change": "Physically unplug selected headphones with built-in speakers present; verify no speaker playback, paused replug, and no route change when changing the system default.",
    "output-absent-startup": "Restart with the saved endpoint absent; verify unavailable selection, retained queue/position, no default/name substitution and no automatic Resume.",
    "output-failed-open-duplicate-names": "Use duplicate friendly names and provoke a failed open. Verify identity distinction, truthful active/unavailable state and safe Pause/Stop during rapid selection.",
    "output-sleep-wake-shared-audio": "Sleep/wake during playback, then verify safe paused recovery, explicit Resume and simultaneous audio from another application.",
}
NATIVE_OBSERVATIONS = (
    ("api-play-ui-open", "api", "open"),
    ("api-pause-ui-open", "api", "open"),
    ("api-toggle-ui-open", "api", "open"),
    ("api-stop-ui-open", "api", "open"),
    ("api-play-ui-closed", "api", "closed"),
    ("api-pause-ui-closed", "api", "closed"),
    ("api-toggle-ui-closed", "api", "closed"),
    ("api-stop-ui-closed", "api", "closed"),
    ("menu-resume-ui-closed", "desktop-menu", "closed"),
    ("ui-reopen-authoritative", "ui-reopen", "open"),
    ("physical-keys-ui-open", "physical-key", "open"),
    ("physical-keys-ui-closed", "physical-key", "closed"),
    ("metadata-cleared", "api", "closed"),
    ("output-loss-rejected", "api", "closed"),
    ("quit-deregistered-before-relaunch", "lifecycle", "closed"),
    ("new-instance-reregistered", "lifecycle", "closed"),
)
SECRET_KEY = re.compile(r"token|password|authorization|cookie|secret|url|header", re.I)
REMOTE_URL = re.compile(r"https?://", re.I)
SECRET_VALUE = re.compile(r"(?:bearer\s+|api[_-]?key\s*[=:]|token\s*[=:]|password\s*[=:])", re.I)
NATIVE_LIBRARY = re.compile(r"(?:lib)?(?:avcodec|avformat|avutil|swresample|pulse)[^/\\]*\.(?:dll|dylib|so(?:\.\d+)*)$", re.I)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def empty_record(target: str) -> dict:
    return {
        "schemaVersion": 1,
        "outputEvidenceVersion": 1,
        "nativeEvidenceVersion": 1,
        "seekEvidenceVersion": 1,
        "outputDeviceKind": "unverified",
        "recordedAt": now(),
        "target": target,
        "os": {"name": platform.system(), "release": platform.release(), "architecture": platform.machine()},
        "package": "unverified",
        "sourceRevision": "unverified",
        "installRoot": "unverified",
        "provider": "unverified",
        "audioRuntime": "unverified",
        "loadedLibraries": "unverified",
        "fixtures": {name: "unverified" for name in FIXTURES},
        "scenarios": {name: {"outcome": "unverified", "notes": ""} for name in SCENARIOS},
        "native": {
            "desktopSession": "unverified",
            "observations": {
                name: {"commandPath": path, "uiState": ui_state, "outcome": "unverified"}
                for name, path, ui_state in NATIVE_OBSERVATIONS
            },
        },
        "seek": {"rows": []},
        "outcome": "unverified",
    }


def detected_target() -> str | None:
    system = platform.system()
    architecture = platform.machine().lower()
    if system == "Windows" and architecture in {"amd64", "x86_64"}:
        return "windows-x64"
    if system == "Linux" and architecture in {"amd64", "x86_64"}:
        return "linux-x64"
    if system == "Darwin" and architecture in {"amd64", "x86_64"}:
        return "macos-x64"
    if system == "Darwin" and architecture in {"arm64", "aarch64"}:
        return "macos-arm64"
    return None


def sanitize(value, path="record"):
    if isinstance(value, dict):
        for key, child in value.items():
            if SECRET_KEY.search(str(key)):
                raise ValueError(f"secret-bearing field is forbidden at {path}.{key}")
            sanitize(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            sanitize(child, f"{path}[{index}]")
    elif isinstance(value, str):
        if REMOTE_URL.search(value):
            raise ValueError(f"remote URL is forbidden at {path}")
        if SECRET_VALUE.search(value):
            raise ValueError(f"secret-like value is forbidden at {path}")
    return value


def safe_audio_runtime(runtime: dict) -> dict:
    """Project daemon health onto the public evidence contract."""
    public_runtime = {
        key: runtime[key]
        for key in (
            "binding", "avcodec", "avformat", "avutil", "swresample",
            "sharedEndpoint", "sharedBackend", "cpalVersion", "pulseVersion", "pulseServerBufferMaxBytes", "compressedHighWaterBytes", "pcmHighWaterSamples",
        )
        if key in runtime
    }
    manifest = runtime.get("manifest")
    if isinstance(manifest, dict):
        public_runtime["manifest"] = {
            key: manifest[key]
            for key in (
                "schemaVersion", "ffmpegRelease", "configureFlags",
                "windowsConfigureFlags", "requiredLibraries", "abiVersions",
                "bindingVersions", "bufferPolicy", "licenses", "sourceSha256",
            )
            if key in manifest
        }
    return public_runtime


def safe_output_snapshot(snapshot: dict) -> dict:
    """Record identity distinction without endpoint names, IDs or media metadata."""
    output = snapshot.get("output", {})
    result = {key: snapshot.get(key) for key in ("state", "positionMs", "queueRevision")}
    result["output"] = {key: output.get(key) for key in ("revision", "status", "error")}
    for role in ("selected", "pending", "active"):
        descriptor = output.get(role)
        result["output"][role] = None if not isinstance(descriptor, dict) else {
            "identityHash": hashlib.sha256(descriptor["outputId"].encode()).hexdigest()
                if isinstance(descriptor.get("outputId"), str) and descriptor["outputId"] else None,
            **{key: descriptor.get(key) for key in ("backend", "available", "isVirtual", "identityConfidence")},
        }
    return result


def app_data_dir() -> Path:
    override = os.environ.get("HIFIMULE_APP_DATA_DIR")
    if override:
        path = Path(override)
        if not path.is_absolute():
            raise ValueError("HIFIMULE_APP_DATA_DIR must be absolute")
        return path
    system = platform.system()
    if system == "Windows":
        base = os.environ.get("APPDATA")
        if base:
            return Path(base) / "HifiMule"
    elif system == "Darwin":
        return Path.home() / "Library" / "Application Support" / "HifiMule"
    elif system == "Linux":
        return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share")) / "HifiMule"
    raise ValueError("cannot resolve HifiMule application-data directory")


def read_descriptor(root: Path) -> dict:
    descriptor = json.loads((root / "runtime" / "owner.json").read_text(encoding="utf-8"))
    for key in ("port", "token", "instanceId", "pid"):
        if key not in descriptor:
            raise ValueError(f"daemon descriptor is missing {key}")
    return descriptor


def rpc(descriptor: dict, method: str, params=None) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{int(descriptor['port'])}/",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {descriptor['token']}"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        payload = json.loads(response.read())
    error = payload.get("error")
    if error is not None:
        code = error.get("code", "unknown") if isinstance(error, dict) else "unknown"
        raise RuntimeError(f"RPC {method} failed with code {code}")
    if "result" not in payload:
        raise RuntimeError(f"RPC {method} returned neither a result nor an error")
    return payload["result"]


def loaded_libraries(pid: int) -> list[str]:
    system = platform.system()
    if system == "Linux":
        lines = Path(f"/proc/{pid}/maps").read_text(encoding="utf-8", errors="replace").splitlines()
        candidates = [line.split()[-1] for line in lines if "/" in line]
    elif system == "Darwin":
        result = subprocess.run(["lsof", "-Fn", "-p", str(pid)], check=True, capture_output=True, text=True)
        candidates = [line[1:] for line in result.stdout.splitlines() if line.startswith("n/")]
    elif system == "Windows":
        command = (
            f"(Get-Process -Id {int(pid)}).Modules | "
            "ForEach-Object { $_.FileName }"
        )
        result = subprocess.run(["powershell", "-NoProfile", "-Command", command], check=True, capture_output=True, text=True)
        candidates = result.stdout.splitlines()
    else:
        raise ValueError(f"unsupported platform: {system}")
    return sorted({path.strip() for path in candidates if NATIVE_LIBRARY.search(path.strip())})


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def revision() -> str:
    configured = os.environ.get("GITHUB_SHA")
    if configured:
        return configured
    try:
        return subprocess.run(["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unverified"


def redact_path(path: str) -> str:
    candidates = []
    for key in ("LOCALAPPDATA", "APPDATA", "USERPROFILE", "HOME"):
        value = os.environ.get(key)
        if value:
            windows_path = bool(re.match(r"^[A-Za-z]:[\\/]", value))
            candidates.append((value.rstrip("/\\"), f"%{key}%" if windows_path else "~"))
    for prefix, replacement in sorted(candidates, key=lambda item: len(item[0]), reverse=True):
        if path.lower() == prefix.lower():
            return replacement
        if path.lower().startswith(prefix.lower() + ("\\" if "\\" in path else "/")):
            return replacement + path[len(prefix):]
    return path


def is_within(path: str, root: str, target: str) -> bool:
    if target.startswith("windows-"):
        child = PureWindowsPath(path)
        parent = PureWindowsPath(root)
        return child == parent or parent in child.parents
    child = PurePosixPath(path)
    parent = PurePosixPath(root)
    return child == parent or parent in child.parents


def valid_output_snapshot(snapshot) -> bool:
    if not isinstance(snapshot, dict):
        return False
    position = snapshot.get("positionMs")
    revision = lambda value: isinstance(value, str) and re.fullmatch(r"0|[1-9][0-9]*", value) is not None
    if (type(position) is not int or position < 0
            or snapshot.get("state") not in ("idle", "paused", "playing", "buffering")
            or not revision(snapshot.get("queueRevision"))):
        return False
    output = snapshot.get("output")
    if (not isinstance(output, dict) or not revision(output.get("revision"))
            or output.get("status") not in ("unselected", "available", "switching", "unavailable", "error")
            or not {"selected", "pending", "active", "error"}.issubset(output)):
        return False
    error = output["error"]
    if error is not None and (not isinstance(error, dict)
            or not isinstance(error.get("code"), str) or not error["code"]
            or type(error.get("retryable")) is not bool):
        return False
    for role in ("selected", "pending", "active"):
        descriptor = output[role]
        if descriptor is None:
            continue  # No active stream is valid during restoration or retirement.
        if (not isinstance(descriptor, dict)
                or not isinstance(descriptor.get("identityHash"), str)
                or not re.fullmatch(r"[0-9a-f]{64}", descriptor["identityHash"])
                or descriptor["identityHash"] == hashlib.sha256(b"").hexdigest()
                or descriptor.get("backend") not in ("coreaudio", "wasapi", "pulse")
                or type(descriptor.get("available")) is not bool
                or type(descriptor.get("isVirtual")) is not bool
                or descriptor.get("identityConfidence") not in ("stable", "unverified", "ambiguous", "unsupported")):
            return False
    if output["status"] == "unselected" and output["selected"] is not None:
        return False
    if output["status"] in {"available", "unavailable"} and output["selected"] is None:
        return False
    return True


def valid_native_state(state) -> bool:
    return (isinstance(state, dict)
            and type(state.get("pid")) is int and state["pid"] > 0
            and isinstance(state.get("instanceId"), str) and bool(state["instanceId"])
            and isinstance(state.get("generationId"), str)
            and re.fullmatch(r"[0-9a-fA-F-]{36}", state["generationId"]) is not None
            and isinstance(state.get("stateSequence"), str)
            and re.fullmatch(r"0|[1-9][0-9]*", state["stateSequence"]) is not None
            and type(state.get("positionMs")) is int and state["positionMs"] >= 0
            and "occurrenceId" in state and (state["occurrenceId"] is None
                or isinstance(state["occurrenceId"], str) and bool(state["occurrenceId"]))
            and isinstance(state.get("queueRevision"), str)
            and re.fullmatch(r"0|[1-9][0-9]*", state["queueRevision"]) is not None
            and state.get("playbackStatus") in
                ("idle", "loading", "active", "paused", "stopped", "completed", "error"))


# Observations are explicit OS/UI facts, never inferred from a successful RPC.
NATIVE_FACTS = {
    "menu-resume-ui-closed": {"windowStayedClosed": True, "audibleProgress": True},
    "ui-reopen-authoritative": {"commandReplayed": False},
    "metadata-cleared": {"richFieldsObserved": True, "sparseMissingFieldsCleared": True,
                         "emptySessionFieldsCleared": True},
    "output-loss-rejected": {"playRejected": True, "outputRerouted": False, "audible": False},
    "quit-deregistered-before-relaunch": {"registrationReleased": True, "metadataCleared": True,
                                           "observedBeforeRelaunch": True, "processExited": True},
    "new-instance-reregistered": {"registrationCount": 1},
}


def native_outcome_errors(name: str, item: dict) -> list[str]:
    errors = []
    facts = item.get("facts", {})
    if not isinstance(facts, dict):
        facts = {}
    for key, expected in NATIVE_FACTS.get(name, {}).items():
        if type(facts.get(key)) is not type(expected) or facts[key] != expected:
            errors.append(f"native observation {name} does not prove {key}")
    before, after = item.get("before"), item.get("after")
    if not valid_native_state(before) or not valid_native_state(after):
        return errors
    if name != "new-instance-reregistered" and int(after["stateSequence"]) < int(before["stateSequence"]):
        errors.append(f"native observation {name} regresses state sequence")
    if name.startswith("api-") or name in {"menu-resume-ui-closed", "ui-reopen-authoritative", "output-loss-rejected"}:
        if (before["occurrenceId"], before["queueRevision"]) != (after["occurrenceId"], after["queueRevision"]):
            errors.append(f"native observation {name} changes the current occurrence or queue")
    if name.startswith("api-") or name == "menu-resume-ui-closed":
        if int(after["stateSequence"]) <= int(before["stateSequence"]):
            errors.append(f"native observation {name} does not advance state sequence")
    if name.startswith("api-stop-") and (after["positionMs"] != 0 or before["generationId"] == after["generationId"]):
        errors.append(f"native observation {name} does not prove Stop position reset and generation rotation")
    if name == "menu-resume-ui-closed" and (before["playbackStatus"] != "paused" or after["playbackStatus"] != "active"
            or before["positionMs"] <= 0 or after["positionMs"] <= before["positionMs"]):
        errors.append(f"native observation {name} does not prove Resume from preserved position")
    if name == "ui-reopen-authoritative":
        # Pause through native controls before reopening, making exact comparison possible.
        if before["playbackStatus"] != "paused" or any(before[key] != after[key] for key in
                ("playbackStatus", "positionMs", "generationId", "stateSequence")):
            errors.append(f"native observation {name} does not prove unchanged paused state on reopen")
        if facts.get("uiPositionMs") != after["positionMs"] or type(facts.get("uiPositionMs")) is not int or facts.get("uiPlaybackStatus") != after["playbackStatus"]:
            errors.append(f"native observation {name} does not prove UI state and position agreement")
    if name == "output-loss-rejected":
        if before["playbackStatus"] not in {"paused", "error"} or after["playbackStatus"] not in {"paused", "error"} or before["positionMs"] != after["positionMs"]:
            errors.append(f"native observation {name} does not prove frozen playback after rejected Play")
        identities = [facts.get(key) for key in ("selectedOutputBefore", "selectedOutputAfter")]
        if any(not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None for value in identities) or identities[0] != identities[1]:
            errors.append(f"native observation {name} does not prove preserved selected output")
    if name == "new-instance-reregistered" and (before["instanceId"] == after["instanceId"] or after["playbackStatus"] != "paused"):
        errors.append(f"native observation {name} does not prove a new paused daemon instance")
    return errors


def validate_native_evidence(record: dict) -> list[str]:
    errors = []
    if record.get("nativeEvidenceVersion") != 1:
        return ["Story 15.6 native evidence is missing; output-only results are insufficient"]
    native = record.get("native")
    if not isinstance(native, dict) or not str(native.get("desktopSession", "")).strip() \
            or native.get("desktopSession") == "unverified":
        errors.append("native desktop session is unverified")
        observations = {}
    else:
        observations = native.get("observations", {})
    for name, expected_path, expected_ui in NATIVE_OBSERVATIONS:
        item = observations.get(name) if isinstance(observations, dict) else None
        if not isinstance(item, dict):
            errors.append(f"native observation {name} is missing")
            continue
        if item.get("commandPath") != expected_path or item.get("uiState") != expected_ui:
            errors.append(f"native observation {name} has an invalid command path or UI state")
        outcome = item.get("outcome")
        delivery = item.get("delivery")
        limitation = item.get("limitation")
        if expected_path == "physical-key":
            if outcome == "passed" and delivery != "observed":
                errors.append(f"native observation {name} claims physical-key success without observed delivery")
            elif outcome == "limitation" and (delivery != "not-delivered" or not str(limitation or "").strip()):
                errors.append(f"native observation {name} lacks an explicit routing limitation")
            elif outcome not in {"passed", "limitation"}:
                errors.append(f"native observation {name} is unverified")
        elif outcome != "passed" or delivery != "observed":
            errors.append(f"native observation {name} is not a delivered pass")
        for phase in ("before", "after"):
            if not valid_native_state(item.get(phase)):
                errors.append(f"native observation {name} has invalid {phase} identity/state evidence")
        errors.extend(native_outcome_errors(name, item))
        before, after = item.get("before"), item.get("after")
        if valid_native_state(before) and valid_native_state(after):
            if name not in {"quit-deregistered-before-relaunch", "new-instance-reregistered"} \
                    and (before["pid"], before["instanceId"]) != (after["pid"], after["instanceId"]):
                errors.append(f"native observation {name} does not preserve daemon continuity")
            before_status = before["playbackStatus"]
            after_status = after["playbackStatus"]
            if name.startswith("api-play-") \
                    and (before_status not in {"paused", "stopped", "completed"}
                         or after_status != "active"):
                errors.append(f"native observation {name} does not prove Play")
            elif name.startswith("api-pause-") \
                    and (before_status not in {"active", "loading"} or after_status != "paused"):
                errors.append(f"native observation {name} does not prove Pause")
            elif name.startswith("api-toggle-") \
                    and {before_status, after_status} != {"active", "paused"}:
                errors.append(f"native observation {name} does not prove Toggle")
            elif name.startswith("api-stop-") \
                    and (before_status not in {"active", "loading", "paused"}
                         or after_status != "stopped"):
                errors.append(f"native observation {name} does not prove Stop")
    return errors


def validate_seek_evidence(record: dict) -> list[str]:
    if record.get("seekEvidenceVersion") != 1:
        return ["Story 15.7 seek evidence is missing or has an unsupported version"]
    seek = record.get("seek")
    rows = seek.get("rows") if isinstance(seek, dict) else None
    if not isinstance(rows, list) or not rows:
        return ["Story 15.7 seek evidence has no provider/representation/backend rows"]
    errors = []
    combinations = {}
    for index, row in enumerate(rows):
        label = f"seek row {index}"
        if not isinstance(row, dict):
            errors.append(f"{label} is invalid")
            continue
        key = tuple(row.get(field) for field in
                    ("provider", "serverVersion", "representation", "container", "codec", "backend"))
        if any(not isinstance(value, str) or not value.strip() for value in key):
            errors.append(f"{label} has incomplete provider/representation/backend identity")
        elif key in combinations and combinations[key] != row:
            errors.append(f"{label} conflicts with duplicate combination {key}")
        else:
            combinations[key] = row
        enabled = row.get("enabled")
        if type(enabled) is not bool:
            errors.append(f"{label} has invalid enabled capability")
            continue
        if not enabled:
            if not str(row.get("disabledReason", "")).strip() or row.get("ordinaryPlayback") != "passed":
                errors.append(f"{label} disabled capability lacks reason or ordinary-playback pass")
            continue
        if row.get("provider") != "jellyfin" or row.get("representation") != "original" \
                or row.get("container") != "wav" \
                or row.get("codec") not in {"pcm_s16le", "pcm_s24le", "pcm_s32le"}:
            errors.append(f"{label} enables an unqualified provider/representation")
        if row.get("mechanism") != "ffmpeg-post-open-media-time-seek":
            errors.append(f"{label} has an unqualified seek mechanism")
        observations = row.get("observations")
        if not isinstance(observations, list) or not observations:
            errors.append(f"{label} has no seek observations")
            continue
        directions = set()
        for observation_index, item in enumerate(observations):
            item_label = f"{label} observation {observation_index}"
            if not isinstance(item, dict):
                errors.append(f"{item_label} is invalid")
                continue
            numeric = ("requestedPositionMs", "priorCommittedPositionMs", "landedPositionMs",
                       "fixtureOraclePositionMs", "absoluteErrorMs", "durationMs",
                       "compressedHighWaterBytes", "pcmHighWaterBytes")
            if any(type(item.get(field)) not in (int, float) or isinstance(item.get(field), bool)
                   or not math.isfinite(item[field]) or item[field] < 0 for field in numeric):
                errors.append(f"{item_label} has invalid numeric evidence")
                continue
            if any(type(item[field]) is not int for field in
                   ("requestedPositionMs", "priorCommittedPositionMs", "landedPositionMs",
                    "fixtureOraclePositionMs", "durationMs", "compressedHighWaterBytes",
                    "pcmHighWaterBytes")):
                errors.append(f"{item_label} positions and buffer peaks must be integers")
            if item["requestedPositionMs"] > item["durationMs"]:
                errors.append(f"{item_label} requests beyond duration")
            expected_error = abs(item["landedPositionMs"] - item["fixtureOraclePositionMs"])
            if item["absoluteErrorMs"] != expected_error or expected_error > 50:
                errors.append(f"{item_label} does not independently prove a landing within 50 ms")
            if item.get("outcome") != "succeeded" or item.get("committedPositionMs") != item["landedPositionMs"]:
                errors.append(f"{item_label} presents pending, failed or contradictory success")
            identity_fields = ("instanceId", "sessionId", "generationId", "occurrenceId",
                               "queueRevision", "operationId")
            if any(not isinstance(item.get(field), str) or not item[field] for field in identity_fields):
                errors.append(f"{item_label} has incomplete operation identity")
            if item.get("queueRevisionAfter") != item.get("queueRevision") \
                    or item.get("occurrenceIdAfter") != item.get("occurrenceId"):
                errors.append(f"{item_label} changes queue or occurrence identity")
            if item["requestedPositionMs"] > item["priorCommittedPositionMs"]:
                directions.add("forward")
            elif item["requestedPositionMs"] < item["priorCommittedPositionMs"]:
                directions.add("backward")
            if item.get("transportBefore") not in {"active", "paused"} \
                    or item.get("transportAfter") not in {"active", "paused", "completed"}:
                errors.append(f"{item_label} has invalid transport outcome")
            if item["compressedHighWaterBytes"] > 8 * 1024 * 1024 \
                    or item["pcmHighWaterBytes"] > 1024 * 1024:
                errors.append(f"{item_label} exceeds bounded playback storage")
        if not {"forward", "backward"}.issubset(directions):
            errors.append(f"{label} has no-op or incomplete forward/backward evidence")
    return errors


def validate_record(record: dict) -> list[str]:
    errors = []
    errors.extend(validate_native_evidence(record))
    errors.extend(validate_seek_evidence(record))
    if record.get("outputEvidenceVersion") != 1:
        errors.append("Story 15.5 output evidence is missing; older playback results are insufficient")
    if record.get("outputDeviceKind") != "physical":
        errors.append("physical-output evidence is required; virtual/unverified routing cannot certify speaker safety")
    target = record.get("target")
    if target not in REQUIRED_TARGETS:
        errors.append(f"unsupported target: {target}")
    expected_platform = {
        "windows-x64": ("windows", {"amd64", "x86_64"}),
        "linux-x64": ("linux", {"amd64", "x86_64"}),
        "macos-x64": ("darwin", {"amd64", "x86_64"}),
        "macos-arm64": ("darwin", {"arm64", "aarch64"}),
    }.get(target)
    os_data = record.get("os", {})
    if expected_platform and (
        str(os_data.get("name", "")).lower() != expected_platform[0]
        or str(os_data.get("architecture", "")).lower() not in expected_platform[1]
    ):
        errors.append(f"recorded OS/architecture does not match {target}")
    package = record.get("package")
    if not isinstance(package, dict) or not re.fullmatch(r"[0-9a-f]{64}", str(package.get("sha256", ""))):
        errors.append("package SHA-256 is missing or invalid")
    if not re.fullmatch(r"[0-9a-f]{40}", str(record.get("sourceRevision", ""))):
        errors.append("source revision is missing or invalid")
    provider = record.get("provider")
    if not isinstance(provider, dict) or provider.get("kind") not in ("jellyfin", "subsonic") or not provider.get("version"):
        errors.append("provider kind/version is unverified")
    runtime = record.get("audioRuntime")
    required_versions = ("avcodec", "avformat", "avutil", "swresample", "sharedBackend", "cpalVersion")
    if not isinstance(runtime, dict) or any(not runtime.get(key) for key in required_versions):
        errors.append("audio runtime versions are unverified")
    elif not runtime.get("sharedEndpoint"):
        errors.append("shared endpoint is unverified")
    else:
        policy = runtime.get("manifest", {}).get("bufferPolicy", {})
        manifest = runtime.get("manifest", {})
        if not re.fullmatch(r"[0-9a-f]{64}", str(manifest.get("sourceSha256", ""))):
            errors.append("FFmpeg source SHA-256 is missing or invalid")
        if not isinstance(manifest.get("configureFlags"), list) or not manifest["configureFlags"]:
            errors.append("FFmpeg configure flags are unverified")
        compressed = runtime.get("compressedHighWaterBytes")
        pcm = runtime.get("pcmHighWaterSamples")
        if not isinstance(compressed, int) or compressed <= 0 or compressed > policy.get("compressedCapacityBytes", -1):
            errors.append("compressed high-water is absent, zero, or over policy")
        if not isinstance(pcm, int) or pcm <= 0 or pcm * 4 > policy.get("pcmCapacityMaxBytes", -1):
            errors.append("PCM high-water is absent, zero, or over policy")
    if target == "linux-x64" and isinstance(runtime, dict) and not runtime.get("pulseVersion"):
        errors.append("loaded Pulse runtime version is unverified")
    if target == "linux-x64" and isinstance(runtime, dict):
        server_buffer = runtime.get("pulseServerBufferMaxBytes")
        if type(server_buffer) is not int or not 0 < server_buffer <= PULSE_MAX_BUFFER_BYTES:
            errors.append("negotiated Pulse buffer is absent or exceeds 100 ms at 48 kHz stereo float32")
    libraries = record.get("loadedLibraries")
    install_root = record.get("installRoot")
    if not isinstance(libraries, list) or len(libraries) < 4 or not isinstance(install_root, str):
        errors.append("installed native-library paths are incomplete")
    else:
        library_names = {name for name in ("avcodec", "avformat", "avutil", "swresample")
                         if any(name in Path(path).name.lower() for path in libraries)}
        if target == "linux-x64" and not any("libpulse.so" in Path(path).name for path in libraries):
            errors.append("installed Pulse library resolution is unverified")
        if len(library_names) != 4:
            errors.append("loaded paths do not cover all four required native libraries")
        if any(not is_within(path, install_root, str(target)) for path in libraries):
            errors.append("a native library resolved outside the install root")
    fixtures = record.get("fixtures", {})
    for name in FIXTURES:
        if fixtures.get(name) != "passed":
            errors.append(f"fixture {name} is not passed")
    scenarios = record.get("scenarios", {})
    for name in SCENARIOS:
        if not isinstance(scenarios.get(name), dict) or scenarios[name].get("outcome") != "passed":
            errors.append(f"scenario {name} is not passed")
            continue
        scenario = scenarios[name]
        for phase in ("before", "after"):
            snapshot = scenario.get(phase, {})
            if not valid_output_snapshot(snapshot):
                errors.append(f"scenario {name} has invalid or missing {phase} output/position evidence")
        before, after = scenario.get("before"), scenario.get("after")
        if valid_output_snapshot(before) and valid_output_snapshot(after):
            selections = [snapshot["output"]["selected"] for snapshot in (before, after)]
            if not any(selections):
                errors.append(f"scenario {name} has no selected output identity evidence")
            for snapshot in (before, after):
                if any(snapshot["output"][role] and snapshot["output"][role]["isVirtual"]
                       for role in ("selected", "active")):
                    errors.append(f"scenario {name} contains virtual rather than physical-output evidence")
            if name == "output-switch-playing-paused" and (
                    not all(selections) or selections[0]["identityHash"] == selections[1]["identityHash"]):
                errors.append(f"scenario {name} does not demonstrate two distinct selected outputs")
            if name == "output-absent-startup" and (
                    selections[1] is None or selections[1]["available"]
                    or after["output"]["active"] is not None or after["state"] not in {"paused", "idle"}):
                errors.append(f"scenario {name} does not demonstrate paused unavailable restoration")
        latency = scenario.get("observedLatencyMs")
        if not isinstance(latency, (int, float)) or isinstance(latency, bool) or latency < 0 or not math.isfinite(latency):
            errors.append(f"scenario {name} has no measured switch/stop latency")
        if not scenario.get("audibleDestination"):
            errors.append(f"scenario {name} has no observed audible destination")
    try:
        sanitize(record)
    except ValueError as error:
        errors.append(str(error))
    return errors


def validate_matrix(directory: Path) -> list[str]:
    errors = []
    records = {}
    for path in directory.glob("*.json"):
        try:
            record = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"{path.name}: invalid JSON: {error}")
            continue
        target = record.get("target")
        if target in REQUIRED_TARGETS:
            if target in records:
                errors.append(f"{target}: duplicate evidence files would overwrite provider combinations")
            else:
                records[target] = (path, record)
    for target in REQUIRED_TARGETS:
        if target not in records:
            errors.append(f"{target}: evidence is missing")
            continue
        path, record = records[target]
        found = validate_record(record)
        if found or record.get("outcome") != "passed":
            errors.extend(f"{target}: invalid: {error}" for error in found or ["outcome is not passed"])
    return errors


def ask_outcome(prompt: str) -> tuple[str, str]:
    print(f"\n{prompt}")
    while True:
        answer = input("Outcome [p=passed, f=failed, u=unverified]: ").strip().lower()
        if answer in {"p", "f", "u"}:
            break
    while True:
        notes = input("Sanitized notes (optional; no URLs, tokens, or credentials): ").strip()
        try:
            sanitize({"notes": notes})
            break
        except ValueError as error:
            print(f"Notes rejected: {error}")
    return {"p": "passed", "f": "failed", "u": "unverified"}[answer], notes


def ask_json_object(prompt: str, validator=None) -> dict:
    while True:
        try:
            value = json.loads(input(prompt))
            if not isinstance(value, dict):
                raise ValueError("enter a JSON object")
            sanitize(value)
            if validator is not None and not validator(value):
                raise ValueError("required fields are missing or invalid")
            return value
        except (json.JSONDecodeError, ValueError) as error:
            print(f"Entry rejected: {error}. Previous observations are retained; retry this entry.")


def collect(args) -> int:
    actual_target = detected_target()
    if actual_target != args.target:
        print(
            f"ERROR: selected target {args.target} does not match this native host ({actual_target or 'unsupported'})",
            file=sys.stderr,
        )
        return 2
    package = Path(args.package).resolve()
    install_root = Path(args.install_root).resolve()
    app_root = Path(args.app_data).resolve() if args.app_data else app_data_dir()
    descriptor = read_descriptor(app_root)
    health = rpc(descriptor, "daemon.health")["data"]
    record = empty_record(args.target)
    record.update({
        "package": {"path": package.name, "sha256": sha256(package)},
        "sourceRevision": revision(),
        "installRoot": redact_path(str(install_root)),
        "provider": {"kind": args.provider_kind, "version": args.provider_version},
        "audioRuntime": safe_audio_runtime(health["audioRuntime"]),
        "loadedLibraries": [redact_path(path) for path in loaded_libraries(int(health["pid"]))],
        "outputDeviceKind": args.output_device_kind,
    })
    record["native"]["desktopSession"] = args.desktop_session

    def capture_runtime() -> None:
        current_descriptor = read_descriptor(app_root)
        current_health = rpc(current_descriptor, "daemon.health")["data"]
        current_runtime = safe_audio_runtime(current_health["audioRuntime"])
        recorded_runtime = record["audioRuntime"]
        for key in ("compressedHighWaterBytes", "pcmHighWaterSamples", "pulseServerBufferMaxBytes"):
            recorded_runtime[key] = max(
                int(recorded_runtime.get(key) or 0), int(current_runtime.get(key) or 0)
            )
        if not recorded_runtime.get("sharedEndpoint") and current_runtime.get("sharedEndpoint"):
            recorded_runtime["sharedEndpoint"] = current_runtime["sharedEndpoint"]
        record["loadedLibraries"] = sorted(set(record["loadedLibraries"]) | set(
            redact_path(path) for path in loaded_libraries(int(current_health["pid"]))
        ))

    for fixture in FIXTURES:
        outcome, notes = ask_outcome(f"Play the installed {fixture} configured-server fixture; verify loading → active, audible output, Pause, Resume and Stop.")
        record["fixtures"][fixture] = outcome
        if notes:
            record.setdefault("fixtureNotes", {})[fixture] = notes
        capture_runtime()
    print("\nRecord native observations. API delivery is distinct from physical-key routing.")
    for name, command_path, ui_state in NATIVE_OBSERVATIONS:
        print(f"\n{name}: path={command_path}, UI={ui_state}")
        if name == "quit-deregistered-before-relaunch":
            print("Quit, observe registration release and metadata clearing, and record the after state before relaunching HifiMule.")
        elif name == "new-instance-reregistered":
            print("Relaunch HifiMule now and verify that the new daemon instance registers once.")
        print("State fields: pid, instanceId, generationId, stateSequence, playbackStatus, positionMs, occurrenceId, queueRevision.")
        print("Use anonymous consistent occurrence/instance IDs; no media metadata. Read actual daemon snapshots.")
        if name == "menu-resume-ui-closed":
            print("Start paused at a nonzero position; Resume and wait for audible progress with UI closed.")
        elif name == "ui-reopen-authoritative":
            print("Pause natively before reopening. Compare frozen daemon state/position with the reopened UI.")
        elif name == "output-loss-rejected":
            print("Capture paused/error state after output loss, then attempt Play; compare frozen position and selected output hashes.")
        elif name == "metadata-cleared":
            print("Observe rich metadata, replace with sparse metadata, then empty session; record each clearing check.")
        elif name == "quit-deregistered-before-relaunch":
            print("For after, retain the final pre-exit snapshot of the old daemon; facts record OS observations after exit, before relaunch.")
        outcome, notes = ask_outcome("Perform the named native observation and record only what was observed.")
        delivery = input("Delivery [observed/not-delivered]: ").strip()
        limitation = input("Explicit routing/platform limitation (required for physical non-delivery): ").strip()
        before = ask_json_object("Sanitized before state JSON: ", valid_native_state)
        after = ask_json_object("Sanitized after state JSON: ", valid_native_state)
        required_facts = NATIVE_FACTS.get(name, {})
        print(f"Facts to observe (required passing values, never copy without verification): {json.dumps(required_facts)}")
        if name == "ui-reopen-authoritative":
            print("Also include uiPositionMs and uiPlaybackStatus read from reopened UI.")
        elif name == "output-loss-rejected":
            print("Also include selectedOutputBefore and selectedOutputAfter: SHA-256 hashes of selected output IDs.")
        facts = ask_json_object("Observed facts JSON ({} if none; record false/actual values for failures): ")
        record["native"]["observations"][name] = {
            "commandPath": command_path,
            "uiState": ui_state,
            "outcome": "limitation" if command_path == "physical-key" and outcome != "passed" else outcome,
            "delivery": delivery,
            "limitation": limitation,
            "notes": notes,
            "before": before,
            "after": after,
            "facts": facts,
        }
    for scenario in SCENARIOS:
        print(f"\n{SCENARIO_GUIDANCE[scenario]}")
        input("Prepare the playing/loading precondition, then press Enter to capture runtime counters before the action: ")
        capture_runtime()
        before = safe_output_snapshot(rpc(read_descriptor(app_root), "playback.getSession", {"schemaVersion": 1})["data"])
        outcome, notes = ask_outcome("Perform the action now. Relaunch HifiMule before answering if the scenario quits it.")
        after = safe_output_snapshot(rpc(read_descriptor(app_root), "playback.getSession", {"schemaVersion": 1})["data"])
        latency = input("Observed switch/stop latency in milliseconds (leave blank if unmeasured): ").strip()
        destination = input("Observed audible destination (use anonymous labels A/B/silent, no personal device names): ").strip()
        record["scenarios"][scenario] = {
            "outcome": outcome, "notes": notes, "before": before, "after": after,
            "observedLatencyMs": float(latency) if latency else None,
            "audibleDestination": destination,
        }
        capture_runtime()
    errors = validate_record(record)
    record["outcome"] = "passed" if not errors else "failed"
    if errors:
        record["validationErrors"] = errors
    sanitize(record)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"\nSaved sanitized evidence to {output}")
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    return 0 if not errors else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    collect_parser = subparsers.add_parser("collect", help="collect one installed native target")
    collect_parser.add_argument("--target", choices=REQUIRED_TARGETS, required=True)
    collect_parser.add_argument("--package", required=True)
    collect_parser.add_argument("--install-root", required=True)
    collect_parser.add_argument("--provider-kind", choices=("jellyfin", "subsonic"), required=True)
    collect_parser.add_argument("--provider-version", required=True)
    collect_parser.add_argument("--app-data")
    collect_parser.add_argument("--output-device-kind", choices=("physical", "virtual"), required=True)
    collect_parser.add_argument("--desktop-session", required=True,
                                help="Desktop/session route, for example macOS Aqua, Windows 11 Explorer, GNOME Wayland")
    collect_parser.add_argument("--output", required=True)
    validate_parser = subparsers.add_parser("validate", help="validate one record or a four-target directory")
    validate_parser.add_argument("path")
    args = parser.parse_args(argv)
    if args.command == "collect":
        return collect(args)
    path = Path(args.path)
    if path.is_dir():
        errors = validate_matrix(path)
    else:
        try:
            errors = validate_record(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError) as error:
            errors = [f"invalid evidence file: {error}"]
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    if not errors:
        print("Playback installed evidence is complete and valid.")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
