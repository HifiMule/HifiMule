"""Regression checks for failed experiments producing fresh, explicit reports."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parent / filename)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


generator = module("fixture_generator", "generate-fixtures.py")
stress = module("stress_probe", "stress.py")
verifier = module("decoder_verifier", "verify.py")


class FailureReporting(unittest.TestCase):
    def test_decoder_selection_preserves_argument_boundaries(self):
        root = Path("directory with spaces")
        self.assertEqual(verifier.decoder_command("ffmpeg", Path("unused"), root)[1],
                         str(root / "ffmpeg_decode.py"))
        self.assertEqual(verifier.decoder_command("symphonia", Path("probe path"), root),
                         [str(Path("probe path").resolve())])

    def test_native_selector_is_forwarded_after_subcommand(self):
        prefix = ["probe with spaces"]
        self.assertEqual(verifier.decode_command("native-ffmpeg", prefix, Path("output pcm")),
                         ["probe with spaces", "decode", "--decoder", "native-ffmpeg", "--output", "output pcm"])
        self.assertEqual(verifier.decode_command("symphonia", prefix, Path("output")),
                         ["probe with spaces", "decode", "--output", "output"])

    def test_native_identity_rejects_missing_feature_or_malformed_json(self):
        for result in (subprocess.CompletedProcess([], 1, "", "feature disabled"),
                       subprocess.CompletedProcess([], 0, "[]", ""),
                       subprocess.CompletedProcess([], 0, '{"decoder":"symphonia"}', ""),
                       subprocess.CompletedProcess([], 0, "not json", "")):
            with patch.object(verifier.subprocess, "run", return_value=result):
                self.assertIn("error", verifier.linked_backend_info(Path("probe")))
        with patch.object(verifier.subprocess, "run", side_effect=FileNotFoundError("missing")):
            self.assertIn("error", verifier.linked_backend_info(Path("probe")))

    def test_missing_version_is_an_explicit_error(self):
        with patch.object(verifier.subprocess, "run", side_effect=FileNotFoundError("not found")):
            self.assertIn("error", verifier.tool_version("ffmpeg"))

    def test_failed_alternative_writes_fresh_required_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "reference").write_bytes(b"\0" * 8)
            (root / "manifest.json").write_text(json.dumps({
                "sample_rate": 48000, "channels": 2, "track_frames": [1],
                "total_frames": 1, "reference": "reference",
                "variants": {"aac": {"files": ["input.m4a"], "lossless": False}}}))
            output = root / "report.json"
            output.write_text('{"required_failed": []}')
            argv = ["verify.py", "--decoder", "ffmpeg", "--fixtures", str(root),
                    "--require", "aac", "--output", str(output)]
            with patch("sys.argv", argv), patch.object(verifier, "tool_version", return_value={"version": "test"}), \
                 patch.object(verifier.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, "", "decoder failed")), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(verifier.main(), 1)
            report = json.loads(output.read_text())
            self.assertEqual(report["required_failed"], ["aac"])
            self.assertEqual(report["decoder_backend"], "ffmpeg")
            self.assertEqual(report["variants"]["aac"]["error"], "decoder failed")

    def test_missing_native_identity_cannot_report_overall_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "reference").write_bytes(b"\0" * 8)
            (root / "manifest.json").write_text(json.dumps({
                "sample_rate": 48000, "channels": 2, "track_frames": [1],
                "total_frames": 1, "reference": "reference",
                "variants": {"wav": {"files": ["input.wav"], "lossless": True}}}))
            output = root / "report.json"
            argv = ["verify.py", "--decoder", "native-ffmpeg", "--fixtures", str(root),
                    "--require", "wav", "--output", str(output)]
            def successful_decode(command, **kwargs):
                Path(command[command.index("--output") + 1]).write_bytes(b"\0" * 8)
                metrics = [{"frames": 1, "channels": 2, "sample_rate": 48000},
                           {"total_frames": 1, "channels": 2, "sample_rate": 48000}]
                return subprocess.CompletedProcess(command, 0, "\n".join(map(json.dumps, metrics)), "")
            with patch("sys.argv", argv), \
                 patch.object(verifier, "linked_backend_info", return_value={"error": "identity unavailable"}), \
                 patch.object(verifier.subprocess, "run", side_effect=successful_decode), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(verifier.main(), 1)
            report = json.loads(output.read_text())
            self.assertTrue(report["variants"]["wav"]["pass"])
            self.assertIn("error", report["linked_backend"])

    def test_encoder_timeout_is_recorded_for_each_variant(self):
        with tempfile.TemporaryDirectory() as temporary:
            argv = ["generate-fixtures.py", "--output", temporary]
            with patch("sys.argv", argv), patch.object(generator.shutil, "which", return_value="ffmpeg"), \
                 patch.object(generator.subprocess, "run", side_effect=subprocess.TimeoutExpired("ffmpeg", 60)), \
                 contextlib.redirect_stdout(io.StringIO()):
                generator.main()
            manifest = json.loads((Path(temporary) / "manifest.json").read_text())
            self.assertEqual(list(manifest["variants"]), ["wav"])
            self.assertEqual(len(manifest["generation_failures"]), 6)

    def check_stress_error(self, error):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"
            output.write_text('{"exit_code": 0}')
            argv = ["stress.py", "--device", "test", "--output", str(output)]
            with patch("sys.argv", argv), patch.object(stress.subprocess, "run", side_effect=error), \
                 contextlib.redirect_stdout(io.StringIO()):
                result = stress.main()
            self.assertNotEqual(result, 0)
            report = json.loads(output.read_text())
            self.assertNotEqual(report["exit_code"], 0)
            self.assertTrue(report["probe_stderr"])
            return report

    def test_missing_probe_replaces_old_success_report(self):
        self.check_stress_error(FileNotFoundError("missing probe"))

    def test_timeout_preserves_partial_output(self):
        report = self.check_stress_error(subprocess.TimeoutExpired("probe", 60, output=b"partial"))
        self.assertEqual(report["probe_stdout"], "partial")


if __name__ == "__main__":
    unittest.main()
