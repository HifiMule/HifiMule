"""Contract tests for Story 16.14 sustained-session evidence."""

import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "playback_long_session_evidence",
    Path(__file__).resolve().parents[1] / "playback-long-session-evidence.py",
)
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)


class LongSessionEvidenceTests(unittest.TestCase):
    def passed_record(self):
        record = evidence.template("linux-x64", "deb")
        record["artifact"] = {"sha256": "a" * 64, "sourceRevision": "b" * 40,
                              "immutableUri": "ci-artifact://release/linux/hifimule.deb"}
        for profile in record["profiles"].values():
            profile.update(outcome="PASS", rawEvidenceUri="ci-artifact://evidence/run.json",
                           durationSeconds=28_800, warmupSeconds=300,
                           sampleIntervalSeconds=5, repetitions=3,
                           budgets={
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
                           })
        record["faults"] = {key: "PASS" for key in record["faults"]}
        record["epic16Matrix"] = {key: "PASS" for key in record["epic16Matrix"]}
        record["decision"] = "PASS"
        return record

    def test_complete_immutable_record_passes(self):
        self.assertEqual(evidence.validate(self.passed_record()), [])

    def test_missing_or_short_profile_cannot_be_promoted(self):
        record = self.passed_record()
        record["profiles"]["radio"]["durationSeconds"] = 1_800
        record["profiles"]["combined"]["outcome"] = "BLOCKED"
        errors = evidence.validate(record)
        self.assertTrue(any("shorter" in error for error in errors))
        self.assertTrue(any("every profile" in error for error in errors))

    def test_secret_urls_and_profile_paths_are_rejected(self):
        record = self.passed_record()
        record["notes"] = "https://user:secret@example.test/audio"
        record["debug"] = "/Users/alexis/private/run.json"
        record["ownerToken"] = "secret"
        errors = evidence.validate(record)
        self.assertGreaterEqual(len([error for error in errors if "unsafe" in error or "secret" in error]), 3)

    def test_every_shipping_row_is_frozen(self):
        self.assertEqual(len(evidence.ROWS), 6)
        for target, package_format in evidence.ROWS:
            record = evidence.template(target, package_format)
            self.assertFalse(any("artifact row" in error for error in evidence.validate(record)))
        invalid = evidence.template("linux-arm64", "appimage")
        self.assertTrue(any("artifact row" in error for error in evidence.validate(invalid)))

    def test_each_independent_owner_budget_is_enforced(self):
        limits = {
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
        for key, limit in limits.items():
            with self.subTest(key=key):
                record = self.passed_record()
                record["profiles"]["radio"]["budgets"][key] = limit + 1
                self.assertTrue(any(key in error for error in evidence.validate(record)))

    def test_history_continuity_requires_exact_large_fixture_accounting(self):
        record = self.passed_record()
        record["history"] = {"fixtureRows": 10_000, "visitedRows": 10_000,
                             "uniqueRows": 10_000, "stableOrder": True,
                             "activeExclusionsPreserved": True,
                             "ambiguousOperationsPreserved": True}
        self.assertEqual(evidence.validate(record), [])
        broken = copy.deepcopy(record)
        broken["history"]["uniqueRows"] = 9_999
        self.assertTrue(any("history continuity" in error for error in evidence.validate(broken)))


if __name__ == "__main__":
    unittest.main()
