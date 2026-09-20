"""Execute the macOS harness with mocked OS commands; never launch or kill apps."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


MOCKS = r'''launches=0
ui_alive=0
second_alive=0
closing=0
ticks=0
daemon_alive=1
daemon_pid=901
log() { printf '%s\n' "$*" >> "$EVENTS"; }
hdiutil() { :; }
xattr() { :; }
cp() { :; }
find() { printf '%s\n' '/Volumes/HifiMule/Hifi [test].app'; }
shasum() { echo 'fakehash artifact.dmg'; }
new_ui_smoke_id() { UI_SMOKE_ID="marker-$((launches + 1))"; }
open() {
    log "open:$*"
    if [[ "$launches" -ge 2 && ( "$ui_alive" == 1 || "$second_alive" == 1 ) ]]; then
        log 'ERROR:launch-before-exit'
        return 1
    fi
    launches=$((launches + 1))
    ui_alive=1
    if [[ "$launches" == 2 ]]; then second_alive=1; fi
    closing=0
    if [[ "$daemon_alive" == 0 ]]; then daemon_alive=1; daemon_pid=902; fi
}
ps() {
    if [[ "$1" == '-axww' ]]; then
        # A helper executable and a regex lookalike must not be terminated.
        printf '888 %s/Contents/MacOS/hifimule-ui-helper\n' "$APP_PATH"
        printf '889 %s/Hifi t.app/Contents/MacOS/hifimule-ui\n' "${APP_PATH%/*}"
        if [[ "$ui_alive" == 1 ]]; then
            printf '801 %s/Contents/MacOS/hifimule-ui\n' "$APP_PATH"
        fi
        if [[ "$second_alive" == 1 ]]; then
            printf '802 %s/Contents/MacOS/hifimule-ui\n' "$APP_PATH"
        fi
    else
        log "diagnostic:$*"
        echo '801 1 S 00:01 hifimule-ui'
    fi
}
kill() {
    log "kill:$*"
    if [[ "$1" == '-0' ]]; then
        [[ "$daemon_alive" == 1 && "$2" == "$daemon_pid" ]]
    elif [[ "$1" == '-9' ]]; then
        daemon_alive=0
    elif [[ "$1" == 801 || "$1" == 802 ]]; then
        closing=1
        ticks=0
    elif [[ "$1" != "$daemon_pid" ]]; then
        log 'ERROR:unrelated-kill'
        return 1
    fi
}
sleep() {
    SECONDS=$((SECONDS + 1))
    if [[ "$closing" == 1 ]]; then
        ticks=$((ticks + 1))
        log "closing-tick:$launches:$ticks"
        if [[ "$SCENARIO" != stuck && "$ticks" -ge 2 ]]; then
            ui_alive=0
            if [[ "$second_alive" == 1 && "$ticks" -ge 3 ]]; then second_alive=0; fi
            if [[ "$second_alive" == 0 ]]; then log "ui-exited:$launches"; fi
        fi
    fi
}
poll_health() { log "health:$launches"; }
poll_ui_ready() {
    log "hydrate:$UI_SMOKE_ID"
    [[ "$SCENARIO" != missing || "$launches" != 3 ]]
}
lifecycle_identity() { printf '%s\tinstance-%s\n' "$daemon_pid" "$daemon_pid"; }
assert_unauthenticated_access_rejected() { log 'auth-rejected'; }
record_unverified_real_device_shutdown() { :; }
'''


class MacosLifecycleTests(unittest.TestCase):
    def run_smoke(self, scenario, aliased_root=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'smoke-macos.sh'
            script.write_text(Path(__file__).with_name('smoke-macos.sh').read_text())
            # Shared protocol gates have dedicated test-ui-evidence.py coverage.
            # Here they are stand-ins so the entire platform control flow runs.
            (root / 'smoke-common.sh').write_text('')
            mocks = root / 'mocks.sh'
            mocks.write_text(MOCKS)
            (root / 'artifact.dmg').touch()
            events = root / 'events'
            install_root = str(root / 'Apps [test]')
            if aliased_root:
                (root / 'real-apps').mkdir()
                (root / 'alias').symlink_to(root / 'real-apps', target_is_directory=True)
                install_root = 'alias'
            env = dict(os.environ, BASH_ENV=str(mocks), EVENTS=str(events),
                       SCENARIO=scenario, HIFIMULE_SMOKE_INSTALL_ROOT=install_root)
            result = subprocess.run(['bash', str(script)], cwd=root, env=env,
                                    text=True, capture_output=True, timeout=10)
            return result, events.read_text().splitlines()

    def test_delayed_exit_fresh_markers_and_recovery_order(self):
        result, events = self.run_smoke('delayed')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        launches = [event for event in events if event.startswith('open:')]
        self.assertEqual(len(launches), 4)
        for index, launch in enumerate(launches, 1):
            self.assertTrue(launch.startswith('open:-n '), launch)
            self.assertTrue(launch.endswith(f' --args --smoke-id marker-{index}'), launch)
        self.assertLess(events.index('ui-exited:2'), events.index(launches[2]))
        self.assertLess(events.index('kill:-9 901'), events.index('ui-exited:3'))
        self.assertLess(events.index('ui-exited:3'), events.index(launches[3]))
        self.assertIn('closing-tick:2:3', events)
        self.assertIn('kill:802', events)
        self.assertIn('hydrate:marker-4', events)
        self.assertIn('auth-rejected', events)
        self.assertFalse(any('ERROR:' in event for event in events), events)

    def test_relative_symlink_install_root_is_normalized(self):
        result, events = self.run_smoke('delayed', aliased_root=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        launches = [event for event in events if event.startswith('open:')]
        self.assertEqual(len(launches), 4)
        for launch in launches:
            self.assertTrue(launch.startswith('open:-n /'), launch)
            self.assertIn('/real-apps/Hifi [test].app --args ', launch)
            self.assertNotIn('/alias/', launch)

    def test_stuck_ui_fails_at_close_without_relaunch(self):
        result, events = self.run_smoke('stuck')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('[step=close-ui]', result.stdout)
        self.assertIn('stage=close-ui smokeId=marker-2', result.stdout)
        self.assertEqual(len([e for e in events if e.startswith('open:')]), 2)
        self.assertIn('diagnostic:-p 801 -o pid=,ppid=,stat=,etime=,comm=', events)
        self.assertFalse(any('ERROR:' in event for event in events), events)

    def test_missing_acknowledgment_preserves_hydration_failure(self):
        result, events = self.run_smoke('missing')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('[step=ui-hydration]', result.stdout)
        self.assertIn('stage=reopen-ui smokeId=marker-3', result.stdout)
        self.assertNotIn('kill:-9 901', events)
        self.assertNotIn('token', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
