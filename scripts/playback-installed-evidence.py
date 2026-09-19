#!/usr/bin/env python3
"""Collect and validate sanitized installed-playback and release evidence."""

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
REQUIRED_RELEASE_ROWS = (
    ("windows-x64", "msi"),
    ("windows-x64", "nsis"),
    ("linux-x64", "deb"),
    ("linux-x64", "appimage"),
    ("macos-x64", "dmg"),
    ("macos-arm64", "dmg"),
)
RELEASE_SCENARIOS = (
    "provider-format-playback",
    "track-and-ordered-album",
    "queue-preview-return-back-next-seek",
    "output-selection-loss-reconnection",
    "window-close-reopen",
    "duplicate-launch",
    "sleep-wake",
    "paused-restoration",
    "native-controls-ui-open-closed",
    "physical-media-keys",
    "safe-quit-real-sync",
    "physical-device-sync",
    "floating-bar-and-compact-browse",
    "keyboard-focus-final-rows",
    "screen-reader-and-live-regions",
    "themes-widths-divider-and-text-scaling",
)
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
RELEASE_SECRET_KEY = re.compile(
    r"(?:ownerToken|accessToken|refreshToken|authorization|cookie|password|requestHeaders|"
    r"authenticatedUrl|serverId|trackId|endpointId|outputId)$", re.I)
HOME_PATH = re.compile(r"(?:^|[\s'\"])(?:/Users/[^/\s]+|/home/[^/\s]+|[A-Za-z]:\\Users\\[^\\\s]+)", re.I)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def empty_record(target: str) -> dict:
    return {
        "schemaVersion": 1,
        "outputEvidenceVersion": 1,
        "nativeEvidenceVersion": 1,
        "seekEvidenceVersion": 1,
        "albumPlaybackEvidenceVersion": 1,
        "continuityEvidenceVersion": 1,
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
        "albumPlayback": {"observations": []},
        "continuity": {"rows": []},
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


def release_privacy_errors(value, path="record") -> list[str]:
    """Reject release evidence that could expose credentials, raw identities, or profiles."""
    errors = []
    if isinstance(value, dict):
        for key, child in value.items():
            if RELEASE_SECRET_KEY.search(str(key)):
                errors.append(f"raw identifier or secret-bearing field is forbidden at {path}.{key}")
            errors.extend(release_privacy_errors(child, f"{path}.{key}"))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            errors.extend(release_privacy_errors(child, f"{path}[{index}]"))
    elif isinstance(value, str):
        if REMOTE_URL.search(value) or SECRET_VALUE.search(value):
            errors.append(f"authenticated/remote URL or secret-like value is forbidden at {path}")
        if HOME_PATH.search(value):
            errors.append(f"unredacted home/profile path is forbidden at {path}")
    return errors


def validate_release_record(record: dict) -> list[str]:
    """Validate the additive Story 15.17 schema without weakening schema v1."""
    if not isinstance(record, dict):
        return ["release record is not an object"]
    errors = release_privacy_errors(record)
    if record.get("schemaVersion") != 2 or record.get("releaseEvidenceVersion") != 1:
        errors.append("release evidence schema/version is unsupported")
    row = (record.get("target"), record.get("packageFormat"))
    if row not in REQUIRED_RELEASE_ROWS:
        errors.append(f"unsupported release row: {row[0]}/{row[1]}")
    evidence_id = record.get("evidenceId")
    if not isinstance(evidence_id, str) or not evidence_id.strip():
        errors.append("stable evidence identity is missing")
    decision = record.get("decision")
    if decision not in {"pass", "blocker", "unsupported"}:
        errors.append("release decision must be pass, blocker, or unsupported")
        return errors
    if decision in {"blocker", "unsupported"}:
        disposition = record.get("blocker") if decision == "blocker" else record.get("unsupported")
        if not isinstance(disposition, dict) or any(
            not isinstance(disposition.get(key), str) or not disposition[key].strip()
            for key in ("owner", "rationale", "requiredAction")
        ):
            errors.append(f"{decision} decision lacks owner, rationale, or required action")
        return errors

    artifact = record.get("artifact")
    if not isinstance(artifact, dict):
        errors.append("artifact identity is missing")
        artifact = {}
    artifact_hash = artifact.get("sha256")
    source_revision = artifact.get("sourceRevision")
    if not isinstance(artifact.get("fileName"), str) or not artifact["fileName"].strip():
        errors.append("artifact filename is missing")
    if not isinstance(artifact_hash, str) or re.fullmatch(r"[0-9a-f]{64}", artifact_hash) is None:
        errors.append("artifact SHA-256 is missing or invalid")
    if not isinstance(source_revision, str) or re.fullmatch(r"[0-9a-f]{40}", source_revision) is None:
        errors.append("artifact source revision is missing or invalid")
    if isinstance(evidence_id, str) and any(str(value) not in evidence_id for value in (*row, artifact_hash)):
        errors.append("evidence identity does not bind target, package format, and artifact hash")
    signing = artifact.get("signing", {})
    if not isinstance(signing, dict):
        errors.append("distribution signing/notarization evidence must be an object")
        signing = {}
    signing_status = signing.get("status")
    signing_identity = signing.get("identity")
    if signing_status == "passed":
        if not isinstance(signing_identity, str) or not signing_identity.strip():
            errors.append("passed distribution signing/notarization evidence requires an identity")
    elif signing_status == "not-configured":
        if signing_identity not in (None, ""):
            errors.append("unsigned distribution evidence must not claim a signing identity")
        if set(signing) - {"status", "identity"}:
            errors.append("unsigned distribution evidence must not claim signing or trust details")
    else:
        errors.append("distribution signing/notarization status must be passed or not-configured")
    licenses = artifact.get("licenses", {})
    if licenses.get("noticePresent") is not True or licenses.get("ffmpegSourceOffer") is not True:
        errors.append("license notice or FFmpeg source offer is missing")

    runtime = record.get("runtime", {})
    if re.fullmatch(r"[0-9a-f]{64}", str(runtime.get("manifestSha256", ""))) is None:
        errors.append("runtime manifest identity is missing")
    versions = runtime.get("loadedVersions", {})
    if any(not versions.get(name) for name in ("avcodec", "avformat", "avutil", "swresample")):
        errors.append("loaded runtime versions are incomplete")
    paths = runtime.get("loadedPaths")
    if not isinstance(paths, list) or not paths or any(not isinstance(path, str) or not path for path in paths):
        errors.append("installed runtime load paths are missing")

    providers = record.get("providers")
    if not isinstance(providers, list) or {item.get("kind") for item in providers if isinstance(item, dict)} != {"jellyfin", "subsonic"}:
        errors.append("exact Jellyfin and Subsonic provider sets are required")
    else:
        for provider in providers:
            if not provider.get("version") or not isinstance(provider.get("capabilities"), list) or not provider["capabilities"]:
                errors.append("provider version/capability identity is incomplete")
            if provider.get("kind") == "subsonic" and not provider.get("implementation"):
                errors.append("Subsonic implementation identity is missing")

    environment = record.get("environment", {})
    required_environment = ("osVersion", "architecture", "upgradeFrom")
    if any(not isinstance(environment.get(key), str) or not environment[key].strip() for key in required_environment):
        errors.append("install/upgrade environment identity is incomplete")
    install_key = "cleanLaunch" if row[1] == "appimage" else "cleanInstall"
    for key in (install_key, "upgrade", "interruptedMigration"):
        if environment.get(key) != "passed":
            errors.append(f"environment {key} is not passed")
    for key in ("noBuildTools", "noSystemFfmpeg"):
        if environment.get(key) is not True:
            errors.append(f"environment {key} is not proven")
    if environment.get("elevationRequired") is not False:
        errors.append("installed playback required elevation")
    permissions = record.get("permissions", {})
    if permissions.get("outcome") != "passed" or not permissions.get("policy"):
        errors.append("package permissions/origin policy is not passed")

    scenarios = record.get("scenarios", {})
    for name in RELEASE_SCENARIOS:
        if scenarios.get(name) != "passed":
            errors.append(f"release scenario {name} is not passed")

    resources = record.get("resourceWorkload", {})
    numeric_requirements = {
        "durationMinutes": (30, None), "warmupMinutes": (5, None),
        "sampleIntervalSeconds": (None, 5), "rssDeltaP95MiB": (None, 128),
        "retainedRssGrowthMiB": (None, 16),
    }
    if resources.get("outcome") != "passed":
        errors.append("resource workload is not passed")
    for key, (minimum, maximum) in numeric_requirements.items():
        value = resources.get(key)
        if type(value) not in (int, float) or not math.isfinite(value) \
                or minimum is not None and value < minimum \
                or maximum is not None and value > maximum:
            errors.append(f"resource workload {key} is invalid or outside the release bound")
    if type(resources.get("normalizedCpuP95")) not in (int, float):
        errors.append("normalized p95 CPU measurement is missing")

    material = record.get("materialEvidence")
    if not isinstance(material, list) or not material:
        errors.append("immutable material evidence is missing")
    else:
        for item in material:
            if (not isinstance(item, dict)
                    or re.fullmatch(r"[0-9a-f]{64}", str(item.get("sha256", ""))) is None
                    or not str(item.get("uri", "")).startswith(("ci-artifact://", "release-artifact://"))
                    or not item.get("retention")):
                errors.append("material evidence lacks hash, immutable URI, or retention policy")
    limitations = record.get("limitations")
    if not isinstance(limitations, list) or any(
        not isinstance(item, dict) or item.get("approved") is not True
        or not item.get("owner") or not item.get("rationale") for item in limitations
    ):
        errors.append("limitations are malformed or unapproved")
    return errors


def validate_release_manifest(manifest: dict, directory: Path) -> list[str]:
    errors = []
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 2:
        return ["release manifest schema is unsupported"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", str(manifest.get("releaseVersion", ""))):
        errors.append("release version is missing or invalid")
    revision_value = manifest.get("sourceRevision")
    if re.fullmatch(r"[0-9a-f]{40}", str(revision_value or "")) is None:
        errors.append("release source revision is missing or invalid")
    rows = manifest.get("rows")
    if not isinstance(rows, list):
        return errors + ["release manifest rows are missing"]
    found = {}
    decisions = []
    for index, item in enumerate(rows):
        if not isinstance(item, dict):
            errors.append(f"release manifest row {index} is invalid")
            continue
        key = (item.get("target"), item.get("packageFormat"))
        if key in found:
            errors.append(f"release row {key[0]}/{key[1]} is duplicated")
            continue
        found[key] = item
        record_name = item.get("record")
        if not isinstance(record_name, str) or Path(record_name).name != record_name:
            errors.append(f"release row {key[0]}/{key[1]} has an unsafe record path")
            continue
        try:
            record = json.loads((directory / record_name).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"release row {key[0]}/{key[1]} record is missing or invalid: {error}")
            continue
        if (record.get("target"), record.get("packageFormat")) != key:
            errors.append(f"release row {key[0]}/{key[1]} record identity disagrees with manifest")
        errors.extend(f"release row {key[0]}/{key[1]}: {error}" for error in validate_release_record(record))
        if record.get("decision") == "pass" and record.get("artifact", {}).get("sourceRevision") != revision_value:
            errors.append(f"release row {key[0]}/{key[1]} source revision disagrees with manifest")
        decisions.append(record.get("decision"))
    for target, package_format in REQUIRED_RELEASE_ROWS:
        if (target, package_format) not in found:
            errors.append(f"release row {target}/{package_format} is missing")
    derived = "pass" if len(decisions) == len(REQUIRED_RELEASE_ROWS) and all(value == "pass" for value in decisions) else "blocker"
    if manifest.get("aggregateDecision") != derived:
        errors.append(f"aggregate decision disagrees with required rows: expected {derived}")
    if derived != "pass":
        errors.append("aggregate release decision is blocker because one or more required installer rows are not passed")
    return errors


def safe_audio_runtime(runtime: dict) -> dict:
    """Project daemon health onto the public evidence contract."""
    public_runtime = {
        key: runtime[key]
        for key in (
            "binding", "avcodec", "avformat", "avutil", "swresample",
            "sharedEndpoint", "sharedBackend", "cpalVersion", "pulseVersion", "pulseServerBufferMaxBytes", "compressedHighWaterBytes", "compressedAggregateHighWaterBytes", "pcmHighWaterSamples",
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
    if type(record.get("seekEvidenceVersion")) is not int or record.get("seekEvidenceVersion") != 1:
        return ["Story 15.7 seek evidence is missing or has an unsupported version"]
    seek = record.get("seek")
    rows = seek.get("rows") if isinstance(seek, dict) else None
    if not isinstance(rows, list) or not rows:
        return ["Story 15.7 seek evidence has no provider/representation/backend rows"]
    errors = []
    combinations = {}
    enabled_rows = 0
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
        enabled_rows += 1
        qualified_media = (
            (row.get("container") == "wav"
             and row.get("codec") in {"pcm_s16le", "pcm_s24le", "pcm_s32le"})
            or (row.get("container") == "m4a" and row.get("codec") in {"aac", "alac"})
            or (row.get("container") in {"ogg", "oga", "opus"}
                and row.get("codec") == "opus")
            or (row.get("container") == "mp3" and row.get("codec") == "mp3")
            or (row.get("container") == "flac" and row.get("codec") == "flac")
        )
        if row.get("provider") not in {"jellyfin", "navidrome"} \
                or row.get("representation") != "original" \
                or not qualified_media:
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
                       "fixtureOraclePositionMs", "committedPositionMs", "absoluteErrorMs", "durationMs",
                       "compressedHighWaterBytes", "pcmHighWaterBytes")
            if any(type(item.get(field)) not in (int, float) or isinstance(item.get(field), bool)
                   or not math.isfinite(item[field]) or item[field] < 0 for field in numeric):
                errors.append(f"{item_label} has invalid numeric evidence")
                continue
            if any(type(item[field]) is not int for field in
                   ("requestedPositionMs", "priorCommittedPositionMs", "landedPositionMs",
                    "fixtureOraclePositionMs", "committedPositionMs", "durationMs", "compressedHighWaterBytes",
                    "pcmHighWaterBytes")):
                errors.append(f"{item_label} positions and buffer peaks must be integers")
            if item["durationMs"] <= 0 or any(item[field] > item["durationMs"] for field in
                    ("requestedPositionMs", "priorCommittedPositionMs", "landedPositionMs", "fixtureOraclePositionMs", "committedPositionMs")):
                errors.append(f"{item_label} has an unavailable duration or position beyond duration")
            if any(item[field] > 9_007_199_254_740_991 for field in
                    ("requestedPositionMs", "priorCommittedPositionMs", "landedPositionMs", "fixtureOraclePositionMs", "committedPositionMs", "durationMs")):
                errors.append(f"{item_label} exceeds safe integer positions")
            target_error = abs(item["fixtureOraclePositionMs"] - item["requestedPositionMs"])
            if target_error > 50 or abs(item["landedPositionMs"] - item["requestedPositionMs"]) > 50:
                errors.append(f"{item_label} did not reach the requested target within 50 ms")
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
            delta = item["requestedPositionMs"] - item["priorCommittedPositionMs"]
            actual_delta = item["fixtureOraclePositionMs"] - item["priorCommittedPositionMs"]
            if delta != 0 and (actual_delta == 0 or (delta > 0) != (actual_delta > 0)):
                errors.append(f"{item_label} claims a successful no-op or wrong-direction seek")
            elif target_error <= 50 and delta > 0:
                directions.add("forward")
            elif target_error <= 50 and delta < 0:
                directions.add("backward")
            expected_transport = "completed" if item["requestedPositionMs"] == item["durationMs"] else item.get("transportBefore")
            if item.get("transportAfter") != expected_transport:
                errors.append(f"{item_label} does not preserve transport intent or terminal completion")
            if item.get("transportBefore") not in {"active", "paused"} \
                    or item.get("transportAfter") not in {"active", "paused", "completed"}:
                errors.append(f"{item_label} has invalid transport outcome")
            if item["compressedHighWaterBytes"] > 8 * 1024 * 1024 \
                    or item["pcmHighWaterBytes"] > 1024 * 1024:
                errors.append(f"{item_label} exceeds bounded playback storage")
        if not {"forward", "backward"}.issubset(directions):
            errors.append(f"{label} has no-op or incomplete forward/backward evidence")
    if enabled_rows == 0:
        errors.append("Story 15.7 requires at least one qualified, usable seek combination")
    return errors


def validate_record(record: dict) -> list[str]:
    errors = []
    errors.extend(validate_native_evidence(record))
    errors.extend(validate_seek_evidence(record))
    errors.extend(validate_album_playback_evidence(record))
    errors.extend(validate_continuity_evidence(record))
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
        compressed_aggregate = runtime.get("compressedAggregateHighWaterBytes")
        pcm = runtime.get("pcmHighWaterSamples")
        if not isinstance(compressed, int) or compressed <= 0 or compressed > policy.get("compressedCapacityBytes", -1):
            errors.append("compressed high-water is absent, zero, or over policy")
        if (not isinstance(compressed_aggregate, int) or compressed_aggregate <= 0
                or compressed_aggregate > policy.get("compressedAggregateCapacityBytes", -1)
                or compressed_aggregate < compressed):
            errors.append("aggregate compressed high-water is absent, inconsistent, or over policy")
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


def validate_continuity_evidence(record: dict) -> list[str]:
    """Require measured physical captures, independently from decoder evidence."""
    if type(record.get("continuityEvidenceVersion")) is not int or record.get("continuityEvidenceVersion") != 1:
        return ["Story 15.9 continuity evidence is missing or has an unsupported version"]
    continuity = record.get("continuity")
    rows = continuity.get("rows") if isinstance(continuity, dict) else None
    if not isinstance(rows, list) or not rows:
        return ["Story 15.9 continuity evidence has no physical-capture rows"]
    errors = []
    supported = 0
    for index, row in enumerate(rows):
        label = f"continuity row {index}"
        if not isinstance(row, dict):
            errors.append(f"{label} is invalid")
            continue
        required_text = ("backend", "architecture", "runtime", "endpointFormat",
                         "captureMethod", "fixtureSha256", "captureSha256",
                         "rawCapturePath", "driftAccountingMethod")
        if any(not isinstance(row.get(key), str) or not row[key].strip() or row[key].strip().lower() == "unverified"
               for key in required_text):
            errors.append(f"{label} has missing runtime, endpoint, fixture or capture identity")
            continue
        if not re.fullmatch(r"[0-9a-f]{64}", row["fixtureSha256"]) \
                or not re.fullmatch(r"[0-9a-f]{64}", row["captureSha256"]):
            errors.append(f"{label} has an invalid fixture or capture hash")
        offsets = row.get("boundaryOffsetsFrames")
        sequence = row.get("occurrenceSequence")
        valid_offsets = (isinstance(offsets, list) and bool(offsets)
                         and all(type(value) is int and value >= 0 for value in offsets)
                         and all(left < right for left, right in zip(offsets, offsets[1:])))
        if not valid_offsets:
            errors.append(f"{label} has invalid boundary offsets")
        if (not isinstance(sequence, list) or not valid_offsets
                or len(sequence) != len(offsets) + 1
                or not all(isinstance(value, str) and value.strip() for value in sequence)
                or len(set(sequence)) != len(sequence)):
            errors.append(f"{label} has invalid occurrence identity sequence")
        numeric = ("openCount", "closeCount", "underruns", "maxGapFrames",
                   "missingFrames", "duplicateFrames")
        if any(type(row.get(key)) is not int or row[key] < 0 for key in numeric):
            errors.append(f"{label} has invalid counters")
            continue
        # A single alignment offset applies to the entire raw capture. Per-side
        # realignment can erase the very discontinuity this evidence measures.
        if row.get("alignmentMethod") != "single-global-offset":
            errors.append(f"{label} requires a single global capture alignment")
        if type(row.get("alignmentOffsetFrames")) is not int:
            errors.append(f"{label} has an invalid global alignment offset")
        for key in ("timingResolutionFrames", "timingToleranceFrames", "clockDriftPpm"):
            value = row.get(key)
            if (type(value) not in (int, float)
                    or (type(value) is float and not math.isfinite(value))
                    or (key == "timingResolutionFrames" and value <= 0)
                    or (key == "timingToleranceFrames" and value < 0)):
                errors.append(f"{label} has invalid {key}")
        if REMOTE_URL.search(row["rawCapturePath"]) or "\x00" in row["rawCapturePath"]:
            errors.append(f"{label} requires a local raw capture artifact path")
        outcome = row.get("outcome")
        if outcome not in ("passed", "unsupported"):
            errors.append(f"{label} has an invalid outcome")
        if outcome == "passed":
            supported += 1
            if row.get("listeningResult") != "passed":
                errors.append(f"{label} lacks a passing supplementary listening result")
            if (row["openCount"] != 1 or row["closeCount"] != 1
                    or row["underruns"] != 0 or row["maxGapFrames"] != 0
                    or row["missingFrames"] != 0 or row["duplicateFrames"] != 0):
                errors.append(f"{label} contradicts successful continuous output")
            if row.get("preparationStatus") != "ready-before-boundary":
                errors.append(f"{label} claims success without prepared successors")
            if row.get("measurementKind") != "physical-capture":
                errors.append(f"{label} substitutes non-physical evidence")
    if supported == 0:
        errors.append("Story 15.9 requires at least one successful physical continuity capture")
    return errors


def validate_album_playback_evidence(record: dict) -> list[str]:
    if type(record.get("albumPlaybackEvidenceVersion")) is not int or record.get("albumPlaybackEvidenceVersion") != 1:
        return ["Story 15.8 album playback evidence is missing or has an unsupported version"]
    album = record.get("albumPlayback")
    observations = album.get("observations") if isinstance(album, dict) else None
    if not isinstance(observations, list) or not observations:
        return ["Story 15.8 album playback evidence has no observations"]
    errors = []
    causes = set()
    repeated_fixture = False
    required = {"naturalCompletion", "next", "pausedNext", "technicalFailure", "retry", "finalCompletion", "offlineRestore"}
    def identity(value):
        return isinstance(value, str) and bool(value.strip()) and len(value) <= 1024

    def nonnegative(value):
        return type(value) is int and value >= 0

    for index, item in enumerate(observations):
        label = f"album observation {index}"
        def reject(message):
            errors.append(f"{label} {message}")

        if not isinstance(item, dict):
            reject("is invalid")
            continue
        cause = item.get("cause")
        if not isinstance(cause, str) or cause not in required:
            reject("has an invalid cause")
            continue
        causes.add(cause)
        sequence = item.get("ordinalSequence")
        count = item.get("totalCount")
        if (not isinstance(sequence, list) or not sequence
                or not all(nonnegative(value) for value in sequence)
                or not nonnegative(count) or not 1 <= count <= 10_000
                or sequence != list(range(count))):
            reject("has an invalid, unordered or truncated ordinal sequence")
            continue
        # The oracle comes from the complete provider fixture, independently of
        # the observed queue. Repeated source identities must stay repeated.
        sources = item.get("sourceSequence")
        expected = item.get("expectedSourceSequence")
        occurrences = item.get("occurrenceSequence")
        lists = (sources, expected, occurrences)
        if not all(isinstance(values, list) and len(values) == count
                   and all(identity(value) for value in values) for values in lists):
            reject("requires complete source, expected-source and occurrence sequences")
            continue
        if sources != expected or len(set(occurrences)) != count:
            reject("changes fixture order or collapses repeated occurrences")
        if len(set(expected)) < count:
            repeated_fixture = True
        before, after = item.get("before"), item.get("after")
        valid_states = True
        for name, state in (("before", before), ("after", after)):
            if (not isinstance(state, dict)
                    or not all(identity(state.get(key)) for key in
                               ("instanceId", "sessionId", "generationId", "occurrenceId", "sourceId", "queueRevision"))
                    or not nonnegative(state.get("ordinal")) or state["ordinal"] >= count
                    or not nonnegative(state.get("positionMs"))
                    or state.get("transport") not in ("active", "paused", "error", "completed", "stopped")):
                reject(f"has invalid {name} identity, ordinal, cursor or transport")
                valid_states = False
                continue
            if (state["occurrenceId"] != occurrences[state["ordinal"]]
                    or state["sourceId"] != sources[state["ordinal"]]):
                reject(f"has inconsistent {name} occurrence/source identity")
        if not valid_states:
            continue
        for key in ("sessionId", "queueRevision"):
            if before[key] != after[key]:
                reject(f"changes {key} during an album transition")
        if cause != "offlineRestore" and before["instanceId"] != after["instanceId"]:
            reject("changes owner instance during a live transition")
        advancing = cause in {"naturalCompletion", "next", "pausedNext"}
        if after["ordinal"] != before["ordinal"] + int(advancing):
            reject("does not select the exact successor or retain the current occurrence")
        if not nonnegative(item.get("duplicateTerminalDeliveries")):
            reject("has an invalid duplicate terminal count")
        if type(item.get("advanceCount")) is not int or item["advanceCount"] != int(advancing):
            reject("has an incorrect advance count, including duplicate delivery")
        audio = cause in {"naturalCompletion", "next", "retry"}
        if type(item.get("audioActivated")) is not bool or item["audioActivated"] != audio:
            reject("has an incorrect actual audio activation outcome")
        dispositions = {"naturalCompletion": "naturalCompletion", "next": "explicitSkip",
                        "pausedNext": "explicitSkip", "technicalFailure": "technicalFailure",
                        "retry": None, "finalCompletion": "naturalCompletion"}
        if cause in dispositions and ("disposition" not in item or item["disposition"] != dispositions[cause]):
            reject("has a contradictory or missing disposition")
        transitions = {"naturalCompletion": ("active", "active"), "next": ("active", "active"),
                       "pausedNext": ("paused", "paused"), "technicalFailure": ("active", "error"),
                       "retry": ("error", "active"), "finalCompletion": ("active", "completed")}
        if cause in transitions and (before["transport"], after["transport"]) != transitions[cause]:
            reject("has contradictory before/after transport")
        if advancing and after["positionMs"] != 0:
            reject("does not start the successor at zero")
        if (advancing or cause == "retry") and before["generationId"] == after["generationId"]:
            reject("does not fence the new playback attempt with a new generation")
        if cause in {"technicalFailure", "retry", "offlineRestore"} and before["positionMs"] != after["positionMs"]:
            reject("does not retain the committed cursor")
        if cause == "finalCompletion":
            terminal = item.get("terminalPositionMs")
            if (after["ordinal"] != count - 1 or not nonnegative(terminal)
                    or terminal != after["positionMs"] or terminal < before["positionMs"]):
                reject("does not retain the final occurrence and actual terminal cursor")
        if cause == "offlineRestore":
            outcomes = item.get("outcomeSequenceBefore")
            if (before["instanceId"] == after["instanceId"] or before["generationId"] == after["generationId"]
                    or after["transport"] != "paused" or item.get("offline") is not True
                    or not isinstance(outcomes, list) or len(outcomes) != count
                    or any(value not in (None, "naturalCompletion", "explicitSkip", "technicalFailure") for value in outcomes)
                    or outcomes != item.get("outcomeSequenceAfter")
                    or item.get("occurrenceSequenceAfter") != occurrences
                    or item.get("sourceSequenceAfter") != sources):
                reject("does not prove silent offline restoration of the complete queue and outcomes")
    for cause in sorted(required - causes):
        errors.append(f"album playback observation {cause} is missing")
    if not repeated_fixture:
        errors.append("album playback evidence needs a fixture with repeated source occurrences")
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
        for key in ("compressedHighWaterBytes", "compressedAggregateHighWaterBytes", "pcmHighWaterSamples", "pulseServerBufferMaxBytes"):
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
    print("\nRecord Story 15.8 ordered-album observations using anonymous identities and ordinals only.")
    for cause in ("naturalCompletion", "next", "pausedNext", "technicalFailure", "retry", "finalCompletion", "offlineRestore"):
        print(f"\n{cause}: use the Story 15.8 schema in docs/playback-installed-test-checklist.md. Include complete ordinalSequence, totalCount, expectedSourceSequence (independent fixture oracle), sourceSequence and unique occurrenceSequence; before/after identity, ordinal, cursor and transport objects; disposition, actual audioActivated, duplicateTerminalDeliveries and advanceCount. Final completion also needs terminalPositionMs; offlineRestore needs offline and the complete restored sequences/outcomes. Include a repeated-source fixture. Do not substitute the old beforeOrdinal/afterOrdinal or failedOccurrenceRetained claims for observed identities.")
        observation = ask_json_object("Sanitized album observation JSON: ")
        observation["cause"] = cause
        record["albumPlayback"]["observations"].append(observation)
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
    validate_parser = subparsers.add_parser(
        "validate", help="validate one legacy record/directory or a v2 release record/manifest")
    validate_parser.add_argument("path")
    args = parser.parse_args(argv)
    if args.command == "collect":
        return collect(args)
    path = Path(args.path)
    if path.is_dir():
        errors = validate_matrix(path)
    else:
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            if payload.get("schemaVersion") == 2 and isinstance(payload.get("rows"), list):
                errors = validate_release_manifest(payload, path.parent)
            elif payload.get("schemaVersion") == 2:
                errors = validate_release_record(payload)
            else:
                errors = validate_record(payload)
        except (OSError, json.JSONDecodeError) as error:
            errors = [f"invalid evidence file: {error}"]
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    if not errors:
        print("Playback installed evidence is complete and valid.")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
