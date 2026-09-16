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
            "stateSequence": "7", "playbackStatus": status}


def native_observation(name, path, ui_state):
    before_status, after_status = "paused", "paused"
    if name.startswith("api-play-"):
        before_status, after_status = "paused", "active"
    elif name.startswith("api-pause-"):
        before_status, after_status = "active", "paused"
    elif name.startswith("api-toggle-"):
        before_status, after_status = "paused", "active"
    elif name.startswith("api-stop-"):
        before_status, after_status = "active", "stopped"
    return {"commandPath": path, "uiState": ui_state,
            "outcome": "passed", "delivery": "observed", "limitation": "",
            "before": native_state(status=before_status),
            "after": native_state(status=after_status)}


class InstalledEvidenceTests(unittest.TestCase):
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
            "avcodec": "63.1.101", "sharedEndpoint": "Speakers",
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
                "avcodec": "63.1.101", "avformat": "63.1.101",
                "avutil": "61.1.101", "swresample": "7.1.101",
                "sharedEndpoint": "Speakers", "compressedHighWaterBytes": 100,
                "pcmHighWaterSamples": 100,
                "manifest": {"sourceSha256": "c" * 64,
                             "configureFlags": ["--disable-network"],
                             "bufferPolicy": {"compressedCapacityBytes": 8388608,
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
        })
        record["outcome"] = "passed"
        return record

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


if __name__ == "__main__":
    unittest.main()
