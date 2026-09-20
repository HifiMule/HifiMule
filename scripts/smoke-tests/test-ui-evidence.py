"""Regression checks for the installed UI acknowledgment gate (no app launch)."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import uuid


class UiEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.profile = tempfile.TemporaryDirectory()
        self.addCleanup(self.profile.cleanup)
        self.runtime = Path(self.profile.name) / 'runtime'
        self.runtime.mkdir()
        self.marker = str(uuid.uuid4())
        self.owner = {'pid': os.getpid(), 'instanceId': 'expected-owner', 'token': 'must-not-be-logged'}
        (self.runtime / 'owner.json').write_text(json.dumps(self.owner))

    def poll(self, timeout, expected_pid=''):
        helper = Path(__file__).resolve().with_name('smoke-common.sh')
        environment = dict(os.environ, HIFIMULE_APP_DATA_DIR=self.profile.name,
                           UI_SMOKE_ID=self.marker, HELPER=str(helper), TIMEOUT=str(timeout),
                           EXPECTED_PID=str(expected_pid))
        return subprocess.run(['bash', '-c', 'source "$HELPER"; poll_ui_ready "$TIMEOUT" "$EXPECTED_PID"'],
                              env=environment, capture_output=True, text=True, timeout=5)

    def poll_shutdown(self, timeout, shutdown_id):
        helper = Path(__file__).resolve().with_name('smoke-common.sh')
        environment = dict(os.environ, HIFIMULE_APP_DATA_DIR=self.profile.name,
                           UI_SMOKE_ID=self.marker, HELPER=str(helper), TIMEOUT=str(timeout),
                           SHUTDOWN_ID=shutdown_id)
        return subprocess.run(
            ['bash', '-c', 'source "$HELPER"; poll_shutdown_rendered "$TIMEOUT" "$SHUTDOWN_ID"'],
            env=environment, capture_output=True, text=True, timeout=5)

    def write_ready(self, instance):
        (self.runtime / f'ui-ready-{self.marker}.json').write_text(json.dumps({
            'smokeId': self.marker, 'state': 'hydrated', 'uiPid': os.getpid(),
            'daemonPid': self.owner['pid'], 'instanceId': instance,
        }))

    def test_daemon_metadata_alone_is_not_ui_attachment(self):
        self.assertNotEqual(self.poll(0).returncode, 0)

    def test_exited_expected_ui_fails_without_waiting_for_timeout(self):
        child = subprocess.Popen(['bash', '-c', 'exit 0'])
        child.wait()
        result = self.poll(30, child.pid)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('exited before confirming hydration', result.stderr)
        self.assertNotIn(self.owner['token'], result.stdout + result.stderr)

    def test_live_acknowledgment_must_belong_to_expected_ui(self):
        child = subprocess.Popen(['sleep', '10'])
        try:
            self.write_ready('expected-owner')
            self.assertNotEqual(self.poll(1, child.pid).returncode, 0)
            self.assertEqual(self.poll(1, os.getpid()).returncode, 0)
        finally:
            child.terminate()
            child.wait()

    def test_only_matching_live_ui_acknowledgment_passes(self):
        self.write_ready('wrong-owner')
        self.assertNotEqual(self.poll(1).returncode, 0)
        self.write_ready('expected-owner')
        result = self.poll(1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('UI_ATTACHMENT_EVIDENCE', result.stdout)
        self.assertNotIn(self.owner['token'], result.stdout + result.stderr)
        self.assertFalse((self.runtime / f'ui-ready-{self.marker}.json').exists())

    def test_shutdown_render_requires_matching_operation_and_owner(self):
        shutdown_id = str(uuid.uuid4())
        path = self.runtime / f'shutdown-rendered-{self.marker}.json'
        path.write_text(json.dumps({
            'smokeId': self.marker, 'state': 'shutdownRendered', 'uiPid': os.getpid(),
            'daemonPid': self.owner['pid'], 'instanceId': self.owner['instanceId'],
            'shutdownId': str(uuid.uuid4()),
        }))
        self.assertNotEqual(self.poll_shutdown(1, shutdown_id).returncode, 0)
        path.write_text(json.dumps({
            'smokeId': self.marker, 'state': 'shutdownRendered', 'uiPid': os.getpid(),
            'daemonPid': self.owner['pid'], 'instanceId': self.owner['instanceId'],
            'shutdownId': shutdown_id,
        }))
        result = self.poll_shutdown(1, shutdown_id)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('SHUTDOWN_UI_EVIDENCE', result.stdout)
        self.assertNotIn(self.owner['token'], result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
