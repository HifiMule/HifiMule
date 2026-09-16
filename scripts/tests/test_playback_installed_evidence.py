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

    def test_validate_record_accepts_complete_bounded_installed_result(self):
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
                                  "before": {"positionMs": 100, "output": {}},
                                  "after": {"positionMs": 100, "output": {}},
                                  "observedLatencyMs": 20, "audibleDestination": "A"}
                          for name in evidence.SCENARIOS},
        })
        record["outcome"] = "passed"
        self.assertEqual(evidence.validate_record(record), [])

        record["os"]["architecture"] = "arm64"
        self.assertTrue(any("does not match" in error for error in evidence.validate_record(record)))
        record["os"]["architecture"] = "AMD64"

        record["loadedLibraries"] = record["loadedLibraries"][:-1] + [
            "C:/Users/test/AppData/Local/HifiMule/avcodec-copy.dll"
        ]
        self.assertTrue(any("four required" in error for error in evidence.validate_record(record)))

    def test_matrix_requires_every_native_target_and_rejects_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for target in evidence.REQUIRED_TARGETS[:-1]:
                (root / f"{target}.json").write_text(json.dumps({"target": target, "outcome": "passed"}))
            errors = evidence.validate_matrix(root)
        self.assertTrue(any(evidence.REQUIRED_TARGETS[-1] in error for error in errors))
        self.assertTrue(any("invalid" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
