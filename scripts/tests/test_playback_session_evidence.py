"""Exercise evidence orchestration without compiling or provisioning native dependencies."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "playback_session_evidence",
    Path(__file__).resolve().parents[1] / "playback-session-evidence.py",
)
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)


class PlaybackEvidenceTests(unittest.TestCase):
    def run_evidence(self, failed_index=None):
        calls = []

        def run(command, **kwargs):
            if command[:2] == ["git", "diff"]:
                return subprocess.CompletedProcess(command, 0, stdout=b"")
            calls.append(command)
            code = 101 if len(calls) - 1 == failed_index else 0
            return subprocess.CompletedProcess(command, code)

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "nested" / "evidence.json"
            with patch.dict(os.environ, {"HIFIMULE_EVIDENCE_PATH": str(output), "GITHUB_SHA": "test-revision"}), \
                    patch.object(evidence.subprocess, "run", side_effect=run), \
                    patch("builtins.print"):
                code = evidence.main()
            record = json.loads(output.read_text(encoding="utf-8"))
        return code, record, calls

    def test_every_fixture_uses_runtime_verifying_wrapper(self):
        code, record, calls = self.run_evidence()
        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 9)
        for command in calls:
            self.assertEqual(command[:5], ["node", "scripts/build-daemon.mjs", "test", "-p", "hifimule-daemon"])
            self.assertEqual(command[-2:], ["--", "--nocapture"])
        self.assertEqual(record["commands"], [" ".join(command) for command in calls])
        self.assertEqual(record["outcome"], "passed")
        self.assertEqual(record["commandExitCodes"], [0] * 9)

    def test_failure_is_preserved_and_remaining_fixtures_run(self):
        code, record, calls = self.run_evidence(failed_index=2)
        self.assertEqual(code, 101)
        self.assertEqual(len(calls), 9)
        self.assertEqual(record["outcome"], "failed")
        self.assertEqual(record["exitCode"], 101)
        self.assertEqual(record["commandExitCodes"], [0, 0, 101, 0, 0, 0, 0, 0, 0])


if __name__ == "__main__":
    unittest.main()
