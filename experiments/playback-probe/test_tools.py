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


class FailureReporting(unittest.TestCase):
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
