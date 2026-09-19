"""Tests for the installed playback evidence collector and validator."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "playback_installed_evidence",
    Path(__file__).resolve().parents[1] / "playback-installed-evidence.py",
)
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)


def output_snapshot(identity="a", available=True):
    selected = {"identityHash": identity * 64, "backend": "wasapi", "available": available,
                "isVirtual": False, "identityConfidence": "stable" if available else "unverified"}
    return {"state": "paused", "positionMs": 100, "queueRevision": "1", "output": {
        "revision": "1", "status": "available" if available else "unavailable", "error": None,
        "selected": selected, "pending": None, "active": None,
    }}


def native_state(pid=123, instance="instance-a", status="paused"):
    return {"pid": pid, "instanceId": instance,
            "generationId": "12345678-1234-1234-1234-123456789abc",
            "stateSequence": "7", "playbackStatus": status, "positionMs": 100,
            "occurrenceId": "anonymous-occurrence", "queueRevision": "1"}


def native_observation(name, path, ui_state):
    before_status, after_status = "paused", "paused"
    if name.startswith("api-play-") or name == "menu-resume-ui-closed":
        before_status, after_status = "paused", "active"
    elif name.startswith("api-pause-"):
        before_status, after_status = "active", "paused"
    elif name.startswith("api-toggle-"):
        before_status, after_status = "paused", "active"
    elif name.startswith("api-stop-"):
        before_status, after_status = "active", "stopped"
    item = {"commandPath": path, "uiState": ui_state,
            "outcome": "passed", "delivery": "observed", "limitation": "",
            "before": native_state(status=before_status),
            "after": native_state(status=after_status),
            "facts": dict(evidence.NATIVE_FACTS.get(name, {}))}
    if name.startswith("api-") or name == "menu-resume-ui-closed":
        item["after"]["stateSequence"] = "8"
    if name.startswith("api-stop-"):
        item["after"].update(positionMs=0, generationId="22345678-1234-1234-1234-123456789abc")
    if name == "menu-resume-ui-closed":
        item["after"]["positionMs"] = 200
    if name == "ui-reopen-authoritative":
        item["facts"].update(uiPositionMs=100, uiPlaybackStatus="paused")
    if name == "output-loss-rejected":
        item["facts"].update(selectedOutputBefore="a" * 64, selectedOutputAfter="a" * 64)
    if name == "new-instance-reregistered":
        item["after"].update(instanceId="instance-b", pid=456)
    return item


def seek_row():
    def observation(operation, prior, requested, landed):
        record = {
            "operationId": operation, "instanceId": "instance-a", "sessionId": "session-a",
            "generationId": "generation-a", "occurrenceId": "occurrence-a",
            "occurrenceIdAfter": "occurrence-a", "queueRevision": "1",
            "queueRevisionAfter": "1", "requestedPositionMs": requested,
            "priorCommittedPositionMs": prior, "landedPositionMs": landed,
            "committedPositionMs": landed, "fixtureOraclePositionMs": landed,
            "absoluteErrorMs": 0, "durationMs": 10_000, "transportBefore": "active",
            "transportAfter": "active", "outcome": "succeeded",
            "compressedHighWaterBytes": 1024, "pcmHighWaterBytes": 2048,
        }
        return record
    return {
        "provider": "jellyfin", "serverVersion": "10.10.7", "representation": "original",
        "container": "wav", "codec": "pcm_s16le", "backend": "wasapi", "enabled": True,
        "mechanism": "ffmpeg-post-open-media-time-seek",
        "observations": [
            observation("seek-forward", 1_000, 4_000, 4_012),
            observation("seek-backward", 4_012, 2_000, 2_008),
        ],
    }


def seek_row_for(container, codec):
    row = seek_row()
    row.update(container=container, codec=codec)
    return row


def album_observation(cause):
    advancing = cause in {"naturalCompletion", "next", "pausedNext"}
    ordinal = 2 if cause == "finalCompletion" else 0
    sources = ["source-a", "source-b", "source-a"]
    occurrences = ["occurrence-a", "occurrence-b", "occurrence-c"]
    before = {"instanceId": "instance-a", "sessionId": "session-a",
              "generationId": "generation-a", "queueRevision": "1",
              "ordinal": ordinal, "occurrenceId": occurrences[ordinal],
              "sourceId": sources[ordinal], "positionMs": 123, "transport": "active"}
    if cause == "pausedNext":
        before["transport"] = "paused"
    if cause == "retry":
        before["transport"] = "error"
    after = dict(before)
    if advancing:
        after.update(ordinal=1, occurrenceId=occurrences[1], sourceId=sources[1], positionMs=0)
    if advancing or cause in {"retry", "offlineRestore"}:
        after["generationId"] = "generation-b"
    if cause == "retry":
        after["transport"] = "active"
    if cause == "technicalFailure":
        after["transport"] = "error"
    if cause == "finalCompletion":
        after.update(transport="completed", positionMs=456)
    if cause == "offlineRestore":
        after.update(instanceId="instance-b", transport="paused")
    return {"cause": cause, "ordinalSequence": [0, 1, 2], "totalCount": 3,
            "expectedSourceSequence": sources[:], "sourceSequence": sources[:],
            "occurrenceSequence": occurrences[:], "before": before, "after": after,
            "duplicateTerminalDeliveries": 1, "advanceCount": int(advancing),
            "audioActivated": cause in {"naturalCompletion", "next", "retry"},
            "disposition": {"naturalCompletion": "naturalCompletion", "next": "explicitSkip",
                            "pausedNext": "explicitSkip", "technicalFailure": "technicalFailure",
                            "finalCompletion": "naturalCompletion"}.get(cause),
            "terminalPositionMs": 456, "offline": True,
            "outcomeSequenceBefore": [None, "explicitSkip", "technicalFailure"],
            "outcomeSequenceAfter": [None, "explicitSkip", "technicalFailure"],
            "occurrenceSequenceAfter": occurrences[:], "sourceSequenceAfter": sources[:]}


def continuity_row():
    return {
        "backend": "wasapi", "architecture": "x86_64",
        "runtime": "cpal-0.18.2/ffmpeg-9.0.2", "endpointFormat": "48000Hz/stereo/f32",
        "captureMethod": "loopback-capture", "measurementKind": "physical-capture",
        "fixtureSha256": "d" * 64, "captureSha256": "e" * 64,
        "rawCapturePath": "captures/windows-x64/album-boundaries.wav",
        "alignmentMethod": "single-global-offset", "alignmentOffsetFrames": 240,
        "timingResolutionFrames": 1, "timingToleranceFrames": 0,
        "clockDriftPpm": -2.5,
        "driftAccountingMethod": "Linear clock fit over the full capture; no boundary realignment.",
        "listeningResult": "passed",
        "occurrenceSequence": ["occurrence-a", "occurrence-b", "occurrence-c"],
        "boundaryOffsetsFrames": [96017, 168028], "preparationStatus": "ready-before-boundary",
        "openCount": 1, "closeCount": 1, "underruns": 0, "maxGapFrames": 0,
        "missingFrames": 0, "duplicateFrames": 0, "outcome": "passed",
    }



class InstalledEvidenceTests(unittest.TestCase):
    def release_record(self, target="windows-x64", package_format="msi"):
        record = {
            "schemaVersion": 2,
            "releaseEvidenceVersion": 1,
            "evidenceId": f"{target}:{package_format}:{'a' * 64}:providers-v1:clean-upgrade",
            "target": target,
            "packageFormat": package_format,
            "artifact": {
                "fileName": f"HifiMule.{package_format}", "sha256": "a" * 64,
                "sourceRevision": "b" * 40,
                "signing": {"status": "passed", "identity": "distribution identity verified"},
                "licenses": {"noticePresent": True, "ffmpegSourceOffer": True},
            },
            "runtime": {
                "manifestSha256": "c" * 64,
                "loadedVersions": {"avcodec": "63.1.102", "avformat": "63.1.102",
                                   "avutil": "61.1.102", "swresample": "7.1.102"},
                "loadedPaths": ["%LOCALAPPDATA%/HifiMule/avcodec-63.dll"],
            },
            "providers": [
                {"kind": "jellyfin", "version": "10.10.7", "capabilities": ["stream", "album", "track"]},
                {"kind": "subsonic", "implementation": "navidrome", "version": "0.58.0",
                 "capabilities": ["stream", "album", "track"]},
            ],
            "environment": {
                "osVersion": "Windows 11", "architecture": "x86_64",
                "cleanInstall": "passed", "upgradeFrom": "0.14.0", "upgrade": "passed",
                "interruptedMigration": "passed", "noBuildTools": True,
                "noSystemFfmpeg": True, "elevationRequired": False,
            },
            "permissions": {"outcome": "passed", "policy": "per-user desktop install"},
            "scenarios": {name: "passed" for name in evidence.RELEASE_SCENARIOS},
            "resourceWorkload": {
                "outcome": "passed", "durationMinutes": 30, "warmupMinutes": 5,
                "sampleIntervalSeconds": 5, "rssDeltaP95MiB": 64,
                "retainedRssGrowthMiB": 8, "normalizedCpuP95": 20,
            },
            "materialEvidence": [{"kind": "physical-continuity", "sha256": "d" * 64,
                                  "uri": "ci-artifact://playback/windows/capture.wav",
                                  "retention": "immutable release archive"}],
            "limitations": [],
            "decision": "pass",
        }
        if package_format == "appimage":
            record["environment"]["cleanLaunch"] = record["environment"].pop("cleanInstall")
        return record

    def test_release_schema_requires_artifact_install_upgrade_lifecycle_ui_and_resources(self):
        record = self.release_record()
        self.assertEqual(evidence.validate_release_record(record), [])
        for mutation in (
            lambda value: value["artifact"].pop("sha256"),
            lambda value: value["environment"].update(upgrade="unverified"),
            lambda value: value["scenarios"].update({"safe-quit-real-sync": "failed"}),
            lambda value: value["resourceWorkload"].update(durationMinutes=29),
            lambda value: value["materialEvidence"][0].update(uri="/Users/alexis/capture.wav"),
        ):
            changed = self.release_record()
            mutation(changed)
            self.assertTrue(evidence.validate_release_record(changed))

    def test_release_schema_accepts_truthful_blocker_but_never_aggregates_it_as_pass(self):
        blocker = {
            "schemaVersion": 2, "releaseEvidenceVersion": 1,
            "evidenceId": "linux-x64:deb:artifact-unavailable:providers-v1:clean-upgrade",
            "target": "linux-x64", "packageFormat": "deb", "decision": "blocker",
            "blocker": {"owner": "release-manager", "rationale": "No Ubuntu host was available.",
                        "requiredAction": "Build, install and execute the release matrix."},
        }
        self.assertEqual(evidence.validate_release_record(blocker), [])

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "linux-deb.json").write_text(json.dumps(blocker))
            manifest = {
                "schemaVersion": 2, "releaseVersion": "0.15.0", "sourceRevision": "b" * 40,
                "rows": [{"target": "linux-x64", "packageFormat": "deb", "record": "linux-deb.json"}],
                "aggregateDecision": "pass",
            }
            errors = evidence.validate_release_manifest(manifest, root)
        self.assertTrue(any("aggregate" in error for error in errors))

    def test_release_manifest_requires_every_installer_row_and_matching_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = []
            for target, package_format in evidence.REQUIRED_RELEASE_ROWS:
                record = self.release_record(target, package_format)
                if target.startswith("linux"):
                    record["environment"].update(osVersion="Ubuntu 22.04", architecture="x86_64")
                    record["runtime"]["loadedPaths"] = ["/opt/hifimule/lib/libavcodec.so.63"]
                elif target == "macos-x64":
                    record["environment"].update(osVersion="macOS 10.15", architecture="x86_64")
                    record["runtime"]["loadedPaths"] = ["/Applications/HifiMule.app/Contents/Frameworks/libavcodec.63.dylib"]
                elif target == "macos-arm64":
                    record["environment"].update(osVersion="macOS 11", architecture="aarch64")
                    record["runtime"]["loadedPaths"] = ["/Applications/HifiMule.app/Contents/Frameworks/libavcodec.63.dylib"]
                name = f"{target}-{package_format}.json"
                (root / name).write_text(json.dumps(record))
                rows.append({"target": target, "packageFormat": package_format, "record": name})
            manifest = {"schemaVersion": 2, "releaseVersion": "0.15.0", "sourceRevision": "b" * 40,
                        "rows": rows, "aggregateDecision": "pass"}
            self.assertEqual(evidence.validate_release_manifest(manifest, root), [])
            manifest["rows"].pop()
            self.assertTrue(any("missing" in error for error in evidence.validate_release_manifest(manifest, root)))

    def test_release_sanitizer_rejects_raw_identifiers_authenticated_urls_and_home_paths(self):
        for value in (
            {"serverId": "private"},
            {"notes": "https://music.example/stream?token=secret"},
            {"path": "/home/alexis/.local/share/HifiMule"},
            {"path": "C:\\Users\\alexis\\AppData\\HifiMule"},
        ):
            with self.subTest(value=value):
                self.assertTrue(evidence.release_privacy_errors(value))

    def test_rpc_keeps_owner_token_out_of_returned_data(self):
        descriptor = {"port": 32123, "token": "top-secret", "instanceId": "owner"}

        class Response:
            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return None

            def read(self):
                return json.dumps({
                    "jsonrpc": "2.0",
                    "result": {"data": {"status": "ok"}},
                    "error": None,
                }).encode()

        with patch.object(evidence.urllib.request, "urlopen", return_value=Response()) as opened:
            result = evidence.rpc(descriptor, "daemon.health")

        self.assertEqual(result, {"data": {"status": "ok"}})
        request = opened.call_args.args[0]
        self.assertEqual(request.headers["Authorization"], "Bearer top-secret")
        self.assertNotIn("top-secret", json.dumps(result))

    def test_sanitize_rejects_secret_bearing_keys_and_urls(self):
        with self.assertRaises(ValueError):
            evidence.sanitize({"ownerToken": "secret"})
        with self.assertRaises(ValueError):
            evidence.sanitize({"notes": "https://server.example/audio?api_key=secret"})
        with self.assertRaises(ValueError):
            evidence.sanitize({"notes": "Bearer top-secret"})

    def test_runtime_projection_drops_urls_and_unknown_fields(self):
        projected = evidence.safe_audio_runtime({
            "avcodec": "63.1.102", "sharedEndpoint": "Speakers",
            "unexpected": "private", "manifest": {
                "sourceUrl": "https://ffmpeg.org/source.tar.xz",
                "sourceSha256": "a" * 64,
                "configureFlags": ["--disable-network"],
                "bufferPolicy": {"compressedCapacityBytes": 8388608},
            },
        })
        self.assertNotIn("unexpected", projected)
        self.assertNotIn("sourceUrl", projected["manifest"])
        self.assertEqual(projected["manifest"]["sourceSha256"], "a" * 64)

    def test_output_projection_omits_endpoint_ids_names_and_source_data(self):
        result = evidence.safe_output_snapshot({
            "positionMs": 123, "state": "paused", "queueRevision": "3",
            "current": {"source": {"trackId": "private-track"}},
            "output": {"revision": "2", "status": "unavailable", "selected": {
                "outputId": "private-endpoint", "displayName": "Personal headphones",
                "backend": "coreaudio", "available": False, "isVirtual": False,
            }},
        })
        encoded = json.dumps(result)
        self.assertNotIn("private-endpoint", encoded)
        self.assertNotIn("Personal headphones", encoded)
        self.assertNotIn("private-track", encoded)
        self.assertEqual(result["positionMs"], 123)
        self.assertEqual(len(result["output"]["selected"]["identityHash"]), 64)
        self.assertTrue(any("physical-output" in error for error in evidence.validate_record({"outputDeviceKind": "virtual"})))

    def test_local_paths_are_redacted_without_losing_containment(self):
        with patch.dict(evidence.os.environ, {
            "LOCALAPPDATA": "C:\\Users\\alexis\\AppData\\Local",
            "USERPROFILE": "C:\\Users\\alexis",
        }, clear=False):
            root = evidence.redact_path("C:\\Users\\alexis\\AppData\\Local\\HifiMule")
            library = evidence.redact_path("C:\\Users\\alexis\\AppData\\Local\\HifiMule\\avcodec-63.dll")
        self.assertEqual(root, "%LOCALAPPDATA%\\HifiMule")
        self.assertTrue(evidence.is_within(library, root, "windows-x64"))

    def complete_record(self):
        record = evidence.empty_record("windows-x64")
        record.update({
            "outputDeviceKind": "physical",
            "os": {"name": "Windows", "release": "11", "architecture": "AMD64"},
            "package": {"path": "HifiMule.exe", "sha256": "a" * 64},
            "sourceRevision": "b" * 40,
            "installRoot": "C:/Users/test/AppData/Local/HifiMule",
            "provider": {"kind": "jellyfin", "version": "10.10.7"},
            "audioRuntime": {
                "sharedBackend": "wasapi", "cpalVersion": "0.18.2",
                "avcodec": "63.1.102", "avformat": "63.1.102",
                "avutil": "61.1.102", "swresample": "7.1.102",
                "sharedEndpoint": "Speakers", "compressedHighWaterBytes": 100,
                "compressedAggregateHighWaterBytes": 200,
                "pcmHighWaterSamples": 100,
                "manifest": {"sourceSha256": "c" * 64,
                             "configureFlags": ["--disable-network"],
                             "bufferPolicy": {"compressedCapacityBytes": 8388608,
                                              "compressedAggregateCapacityBytes": 16777216,
                                              "pcmCapacityMaxBytes": 1048576}},
            },
            "loadedLibraries": [
                "C:/Users/test/AppData/Local/HifiMule/avcodec-63.dll",
                "C:/Users/test/AppData/Local/HifiMule/avformat-63.dll",
                "C:/Users/test/AppData/Local/HifiMule/avutil-61.dll",
                "C:/Users/test/AppData/Local/HifiMule/swresample-7.dll",
            ],
            "fixtures": {name: "passed" for name in evidence.FIXTURES},
            "scenarios": {name: {"outcome": "passed", "notes": "",
                                  "before": output_snapshot(),
                                  "after": output_snapshot("b" if name == "output-switch-playing-paused" else "a", name != "output-absent-startup"),
                                  "observedLatencyMs": 20, "audibleDestination": "A"}
                          for name in evidence.SCENARIOS},
            "native": {"desktopSession": "Windows 11 Explorer", "observations": {
                name: native_observation(name, path, ui_state)
                for name, path, ui_state in evidence.NATIVE_OBSERVATIONS
            }},
            "seek": {"rows": [seek_row()]},
            "albumPlayback": {"observations": [
                album_observation(cause)
                for cause in ("naturalCompletion", "next", "pausedNext", "technicalFailure",
                              "retry", "finalCompletion", "offlineRestore")
            ]},
            "continuity": {"rows": [continuity_row()]},
        })
        record["outcome"] = "passed"
        return record

    def test_continuity_evidence_rejects_missing_runtime_capture_and_contradictions(self):
        record = self.complete_record()
        self.assertEqual(evidence.validate_continuity_evidence(record), [])
        for mutation in (
            lambda row: row.update(runtime=""),
            lambda row: row.update(captureSha256="bad"),
            lambda row: row.update(occurrenceSequence=["same", "same", "same"]),
            lambda row: row.update(boundaryOffsetsFrames=[20, 10]),
            lambda row: row.update(measurementKind="callback-counter"),
            lambda row: row.update(openCount=2),
            lambda row: row.update(maxGapFrames=1),
            lambda row: row.update(preparationStatus="late"),
        ):
            changed = self.complete_record()
            mutation(changed["continuity"]["rows"][0])
            self.assertTrue(evidence.validate_continuity_evidence(changed))

    def test_continuity_evidence_rejects_malformed_types_without_exceptions(self):
        malformed = (None, True, False, {}, "1", 1.5, [], [None], [True], [{}],
                     [1, "2"], [2, 1], [1, 1], [-1, 2])
        for field in ("boundaryOffsetsFrames", "occurrenceSequence", "underruns", "outcome"):
            for value in malformed:
                with self.subTest(field=field, value=value):
                    record = self.complete_record()
                    record["continuity"]["rows"][0][field] = value
                    self.assertTrue(evidence.validate_continuity_evidence(record))
        for version in (None, True, "1", 1.0, [], {}):
            record = self.complete_record()
            record["continuityEvidenceVersion"] = version
            self.assertTrue(evidence.validate_continuity_evidence(record))

    def test_continuity_requires_reproducible_capture_fields(self):
        fields = ("rawCapturePath", "alignmentMethod", "alignmentOffsetFrames",
                  "timingResolutionFrames", "timingToleranceFrames", "clockDriftPpm",
                  "driftAccountingMethod", "listeningResult")
        for field in fields:
            with self.subTest(missing=field):
                record = self.complete_record()
                del record["continuity"]["rows"][0][field]
                self.assertTrue(evidence.validate_continuity_evidence(record))
        for field in fields:
            for value in (None, True, [], {}, "", "unverified"):
                with self.subTest(field=field, value=value):
                    record = self.complete_record()
                    record["continuity"]["rows"][0][field] = value
                    self.assertTrue(evidence.validate_continuity_evidence(record))

    def test_continuity_rejects_nonreproducible_alignment_and_measurements(self):
        changes = ({"alignmentMethod": "independent-boundary-alignment"},
                   {"alignmentOffsetFrames": 0.5}, {"timingResolutionFrames": 0},
                   {"timingToleranceFrames": -1}, {"listeningResult": "failed"},
                   {"rawCapturePath": "https://example.invalid/capture.wav"})
        for change in changes:
            with self.subTest(change=change):
                record = self.complete_record()
                record["continuity"]["rows"][0].update(change)
                self.assertTrue(evidence.validate_continuity_evidence(record))
        for field in ("timingResolutionFrames", "timingToleranceFrames", "clockDriftPpm"):
            for value in (float("nan"), float("inf"), -float("inf")):
                record = self.complete_record()
                record["continuity"]["rows"][0][field] = value
                self.assertTrue(evidence.validate_continuity_evidence(record))

    def test_continuity_accepts_documented_signed_alignment_and_clock_drift(self):
        for drift in (-12.5, 0, 12.5):
            record = self.complete_record()
            record["continuity"]["rows"][0].update(
                alignmentOffsetFrames=-240, clockDriftPpm=drift,
                timingResolutionFrames=0.5, timingToleranceFrames=0.5)
            self.assertEqual(evidence.validate_continuity_evidence(record), [])

    def test_album_evidence_rejects_incorrect_order_identity_and_transport(self):
        def invalid(cause, change):
            record = self.complete_record()
            item = next(item for item in record["albumPlayback"]["observations"] if item["cause"] == cause)
            change(item)
            self.assertTrue(evidence.validate_album_playback_evidence(record), (cause, item))

        cases = [
            ("naturalCompletion", lambda row: row.update(ordinalSequence=[1, 0, 2])),
            ("naturalCompletion", lambda row: row["after"].update(ordinal=9)),
            ("naturalCompletion", lambda row: row["after"].update(ordinal=2, occurrenceId="occurrence-c", sourceId="source-a")),
            ("naturalCompletion", lambda row: row["after"].update(sessionId="different")),
            ("naturalCompletion", lambda row: row["after"].update(occurrenceId="occurrence-a")),
            ("naturalCompletion", lambda row: row.update(sourceSequence=["source-a", "source-a", "source-b"])),
            ("naturalCompletion", lambda row: row.update(occurrenceSequence=["occurrence-a", "occurrence-b", "occurrence-a"])),
            ("naturalCompletion", lambda row: row.update(advanceCount=2)),
            ("next", lambda row: row.update(disposition="naturalCompletion")),
            ("pausedNext", lambda row: row.update(audioActivated=True)),
            ("retry", lambda row: row["after"].update(positionMs=0)),
            ("retry", lambda row: row["after"].update(generationId="generation-a")),
            ("retry", lambda row: row.update(audioActivated=False)),
            ("technicalFailure", lambda row: row["after"].update(transport="active")),
            ("finalCompletion", lambda row: row.update(terminalPositionMs=999)),
            ("finalCompletion", lambda row: row["after"].update(transport="active")),
            ("offlineRestore", lambda row: row.update(offline=False)),
            ("offlineRestore", lambda row: row.update(outcomeSequenceAfter=[None, None, None])),
            ("offlineRestore", lambda row: row.update(occurrenceSequenceAfter=["occurrence-a"])),
            ("offlineRestore", lambda row: row["after"].update(instanceId="instance-a")),
        ]
        for cause, change in cases:
            with self.subTest(cause=cause, change=change):
                invalid(cause, change)

    def test_album_malformed_json_values_return_errors_without_exceptions(self):
        malformed = [None, True, False, 7, -1, 1.5, "invalid", [], {}, [1], {"x": 1}]
        fields = ["cause", "ordinalSequence", "totalCount", "sourceSequence", "expectedSourceSequence",
                  "occurrenceSequence", "before", "after", "duplicateTerminalDeliveries", "advanceCount",
                  "audioActivated", "disposition"]
        for field in fields:
            for value in malformed:
                if (field == "duplicateTerminalDeliveries" and type(value) is int and value >= 0) or (field == "audioActivated" and value is True):
                    continue
                with self.subTest(field=field, value=value):
                    record = self.complete_record()
                    record["albumPlayback"]["observations"][0][field] = value
                    self.assertTrue(evidence.validate_album_playback_evidence(record))
        for version in (None, True, "1", 2, [], {}):
            record = self.complete_record()
            record["albumPlaybackEvidenceVersion"] = version
            self.assertTrue(evidence.validate_album_playback_evidence(record))

    def test_album_malformed_nested_states_return_errors(self):
        for key in ("instanceId", "sessionId", "generationId", "occurrenceId", "sourceId", "queueRevision", "ordinal", "positionMs", "transport"):
            for value in (None, True, [], {}, 7.5):
                with self.subTest(key=key, value=value):
                    record = self.complete_record()
                    record["albumPlayback"]["observations"][0]["after"][key] = value
                    self.assertTrue(evidence.validate_album_playback_evidence(record))

    def test_album_evidence_requires_complete_observations_and_repeated_fixture(self):
        for missing in ("before", "after", "expectedSourceSequence", "occurrenceSequence", "disposition", "audioActivated"):
            record = self.complete_record()
            del record["albumPlayback"]["observations"][0][missing]
            self.assertTrue(evidence.validate_album_playback_evidence(record))
        record = self.complete_record()
        for row in record["albumPlayback"]["observations"]:
            row["expectedSourceSequence"][2] = "source-c"
            row["sourceSequence"][2] = "source-c"
            row["sourceSequenceAfter"][2] = "source-c"
            for state in (row["before"], row["after"]):
                if state["ordinal"] == 2:
                    state["sourceId"] = "source-c"
        self.assertTrue(any("repeated source" in error for error in evidence.validate_album_playback_evidence(record)))

    def test_album_offline_and_final_malformed_values_are_rejected(self):
        for cause, fields in (("offlineRestore", ("offline", "outcomeSequenceBefore", "outcomeSequenceAfter", "occurrenceSequenceAfter", "sourceSequenceAfter")),
                              ("finalCompletion", ("terminalPositionMs",))):
            for field in fields:
                for value in (None, [], {}, "invalid", -1, False):
                    with self.subTest(cause=cause, field=field, value=value):
                        record = self.complete_record()
                        row = next(row for row in record["albumPlayback"]["observations"] if row["cause"] == cause)
                        row[field] = value
                        self.assertTrue(evidence.validate_album_playback_evidence(record))

    def test_validate_record_accepts_complete_bounded_installed_result(self):
        record = self.complete_record()
        self.assertEqual(evidence.validate_record(record), [])

        record["os"]["architecture"] = "arm64"
        self.assertTrue(any("does not match" in error for error in evidence.validate_record(record)))
        record["os"]["architecture"] = "AMD64"

        record["loadedLibraries"] = record["loadedLibraries"][:-1] + [
            "C:/Users/test/AppData/Local/HifiMule/avcodec-copy.dll"
        ]
        self.assertTrue(any("four required" in error for error in evidence.validate_record(record)))

    def test_seek_evidence_cannot_certify_a_fully_disabled_feature(self):
        row = seek_row()
        row.update(enabled=False, disabledReason="unverified installed target", ordinaryPlayback="passed")
        errors = evidence.validate_seek_evidence({"seekEvidenceVersion": 1, "seek": {"rows": [row]}})
        self.assertTrue(any("usable seek combination" in error for error in errors))

    def test_seek_evidence_accepts_the_first_compressed_jellyfin_batch(self):
        for container, codec in (("m4a", "aac"), ("m4a", "alac"),
                                 ("ogg", "opus"), ("oga", "opus"), ("opus", "opus"),
                                 ("mp3", "mp3"), ("flac", "flac")):
            with self.subTest(container=container, codec=codec):
                self.assertEqual(evidence.validate_seek_evidence({
                    "seekEvidenceVersion": 1,
                    "seek": {"rows": [seek_row_for(container, codec)]},
                }), [])
        errors = evidence.validate_seek_evidence({
            "seekEvidenceVersion": 1,
            "seek": {"rows": [seek_row_for("ogg", "vorbis")]},
        })
        self.assertTrue(any("unqualified provider" in error for error in errors))

    def test_seek_evidence_accepts_navidrome_raw_original_formats(self):
        for container, codec in (("wav", "pcm_s16le"), ("m4a", "aac"),
                                 ("m4a", "alac"), ("ogg", "opus"),
                                 ("mp3", "mp3"), ("flac", "flac")):
            with self.subTest(container=container, codec=codec):
                row = seek_row_for(container, codec)
                row["provider"] = "navidrome"
                self.assertEqual(evidence.validate_seek_evidence({
                    "seekEvidenceVersion": 1,
                    "seek": {"rows": [row]},
                }), [])

    def test_seek_evidence_rejects_unchanged_landing_despite_changed_requests(self):
        row = seek_row()
        for item in row["observations"]:
            item.update(priorCommittedPositionMs=3000, landedPositionMs=3000,
                        committedPositionMs=3000, fixtureOraclePositionMs=3000, absoluteErrorMs=0)
        errors = evidence.validate_seek_evidence({"seekEvidenceVersion": 1, "seek": {"rows": [row]}})
        self.assertTrue(any("no-op" in error for error in errors))
        self.assertTrue(any("requested target" in error for error in errors))

    def test_seek_evidence_requires_transport_intent_and_strict_numbers(self):
        for changes in ({"transportBefore": "paused", "transportAfter": "active"},
                        {"transportAfter": "completed"}, {"committedPositionMs": True},
                        {"durationMs": 0}):
            with self.subTest(changes=changes):
                row = seek_row()
                row["observations"][0].update(changes)
                self.assertTrue(evidence.validate_seek_evidence(
                    {"seekEvidenceVersion": 1, "seek": {"rows": [row]}}))
        self.assertTrue(evidence.validate_seek_evidence(
            {"seekEvidenceVersion": True, "seek": {"rows": [seek_row()]}}))

    def test_seek_evidence_rejects_noop_false_landing_and_identity_changes(self):
        record = self.complete_record()
        row = record["seek"]["rows"][0]
        row["observations"][0]["requestedPositionMs"] = 1_000
        self.assertTrue(any("forward/backward" in error for error in evidence.validate_record(record)))
        record = self.complete_record()
        item = record["seek"]["rows"][0]["observations"][0]
        item["fixtureOraclePositionMs"] = item["landedPositionMs"] + 51
        item["absoluteErrorMs"] = 51
        self.assertTrue(any("within 50 ms" in error for error in evidence.validate_record(record)))
        record = self.complete_record()
        record["seek"]["rows"][0]["observations"][0]["queueRevisionAfter"] = "2"
        self.assertTrue(any("queue or occurrence" in error for error in evidence.validate_record(record)))

    def test_output_evidence_rejects_empty_incomplete_and_virtual_descriptors(self):
        for output in ({}, {"revision": "1", "status": "available"},
                       {**output_snapshot()["output"], "selected": {}},
                       {**output_snapshot()["output"], "selected": {**output_snapshot()["output"]["selected"], "identityHash": ""}}):
            record = self.complete_record()
            record["scenarios"]["output-loss"]["before"]["output"] = output
            self.assertTrue(any("before output/position evidence" in error for error in evidence.validate_record(record)))
        record = self.complete_record()
        record["scenarios"]["output-loss"]["before"]["output"]["selected"]["isVirtual"] = True
        self.assertTrue(any("virtual" in error for error in evidence.validate_record(record)))
        missing_id = evidence.safe_output_snapshot({"output": {"selected": {}}})
        self.assertIsNone(missing_id["output"]["selected"]["identityHash"])

    def test_output_evidence_checks_switch_identity_and_absent_startup(self):
        record = self.complete_record()
        record["scenarios"]["output-switch-playing-paused"]["after"] = output_snapshot()
        self.assertTrue(any("two distinct" in error for error in evidence.validate_record(record)))
        record = self.complete_record()
        record["scenarios"]["output-absent-startup"]["after"] = output_snapshot()
        self.assertTrue(any("unavailable restoration" in error for error in evidence.validate_record(record)))
        snapshot = output_snapshot(available=False)
        self.assertTrue(evidence.valid_output_snapshot(snapshot)) # Explicit null active is valid.
        snapshot["positionMs"] = True
        self.assertFalse(evidence.valid_output_snapshot(snapshot))

    def test_linux_negotiated_buffer_accepts_100_ms_and_rejects_overflow(self):
        record = self.complete_record()
        record.update({"target": "linux-x64", "os": {"name": "Linux", "architecture": "x86_64"},
                       "installRoot": "/opt/hifimule", "loadedLibraries": [
                           "/opt/hifimule/lib" + library + ".so" for library in
                           ("avcodec", "avformat", "avutil", "swresample", "pulse")]})
        record["audioRuntime"].update({"sharedBackend": "pulse", "pulseVersion": "17.0", "pulseServerBufferMaxBytes": 38400})
        self.assertEqual(evidence.validate_record(record), [])
        for value in (38401, 0, None, True):
            record["audioRuntime"]["pulseServerBufferMaxBytes"] = value
            self.assertTrue(any("Pulse buffer" in error for error in evidence.validate_record(record)))

    def test_matrix_requires_every_native_target_and_rejects_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for target in evidence.REQUIRED_TARGETS[:-1]:
                (root / f"{target}.json").write_text(json.dumps({"target": target, "outcome": "passed"}))
            errors = evidence.validate_matrix(root)
        self.assertTrue(any(evidence.REQUIRED_TARGETS[-1] in error for error in errors))
        self.assertTrue(any("invalid" in error for error in errors))

    def test_native_evidence_rejects_output_only_and_mislabeled_physical_success(self):
        record = self.complete_record()
        record.pop("nativeEvidenceVersion")
        self.assertTrue(any("native evidence is missing" in error
                            for error in evidence.validate_record(record)))
        record = self.complete_record()
        item = record["native"]["observations"]["physical-keys-ui-closed"]
        item["delivery"] = "api-observed"
        self.assertTrue(any("physical-key success" in error
                            for error in evidence.validate_record(record)))

    def test_native_physical_routing_limitation_is_explicit_and_accepted(self):
        record = self.complete_record()
        item = record["native"]["observations"]["physical-keys-ui-closed"]
        item.update({"outcome": "limitation", "delivery": "not-delivered",
                     "limitation": "Desktop routed the key to another player."})
        self.assertEqual(evidence.validate_record(record), [])

    def test_native_api_observations_prove_each_transport_transition(self):
        for name in ("api-play-ui-open", "api-pause-ui-closed",
                     "api-toggle-ui-open", "api-stop-ui-closed"):
            record = self.complete_record()
            record["native"]["observations"][name]["after"]["playbackStatus"] = "error"
            self.assertTrue(any(f"{name} does not prove" in error
                                for error in evidence.validate_record(record)))

    def test_native_position_and_action_facts_are_required(self):
        for name, _, _ in evidence.NATIVE_OBSERVATIONS:
            with self.subTest(name=name):
                record = self.complete_record()
                del record["native"]["observations"][name]["after"]["positionMs"]
                self.assertTrue(evidence.validate_native_evidence(record))
        for name, fields in evidence.NATIVE_FACTS.items():
            for key in fields:
                with self.subTest(name=name, key=key):
                    record = self.complete_record()
                    del record["native"]["observations"][name]["facts"][key]
                    self.assertTrue(evidence.validate_native_evidence(record))

    def test_native_outcomes_reject_false_success(self):
        cases = [
            ("api-stop-ui-open", "after", "positionMs", 123),
            ("api-stop-ui-open", "after", "generationId", native_state()["generationId"]),
            ("api-play-ui-open", "after", "stateSequence", "7"),
            ("menu-resume-ui-closed", "after", "playbackStatus", "paused"),
            ("menu-resume-ui-closed", "after", "positionMs", 0),
            ("menu-resume-ui-closed", "after", "occurrenceId", "replacement"),
            ("ui-reopen-authoritative", "after", "positionMs", 101),
            ("ui-reopen-authoritative", "facts", "uiPositionMs", 101),
            ("ui-reopen-authoritative", "facts", "commandReplayed", True),
            ("metadata-cleared", "facts", "sparseMissingFieldsCleared", False),
            ("output-loss-rejected", "after", "playbackStatus", "active"),
            ("output-loss-rejected", "facts", "selectedOutputAfter", "b" * 64),
            ("quit-deregistered-before-relaunch", "facts", "observedBeforeRelaunch", False),
            ("new-instance-reregistered", "after", "instanceId", "instance-a"),
            ("new-instance-reregistered", "facts", "registrationCount", True),
        ]
        for name, section, field, value in cases:
            with self.subTest(name=name, field=field):
                record = self.complete_record()
                record["native"]["observations"][name][section][field] = value
                self.assertTrue(evidence.validate_native_evidence(record))

    def test_json_entry_retries_without_losing_previous_observation(self):
        previous = {"before": native_state()}
        with patch("builtins.input", side_effect=["{bad", "[]", '{"token":"private"}', '{}', json.dumps(native_state())]), patch("builtins.print") as printed:
            previous["after"] = evidence.ask_json_object("state: ", evidence.valid_native_state)
        self.assertEqual(previous, {"before": native_state(), "after": native_state()})
        self.assertEqual(printed.call_count, 4)

    def test_native_empty_session_state_allows_null_occurrence(self):
        state = native_state(status="idle")
        state["occurrenceId"] = None
        self.assertTrue(evidence.valid_native_state(state))
        state["positionMs"] = True
        self.assertFalse(evidence.valid_native_state(state))


if __name__ == "__main__":
    unittest.main()
