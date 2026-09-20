"""Exercise Linux smoke helpers with shell mocks; never signal real processes."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name("smoke-linux.sh").read_text()


def function(name):
    match = re.search(r"^" + name + r"\(\) \{\n.*?^\}", SOURCE, re.M | re.S)
    if match is None:
        raise AssertionError(f"Missing harness function: {name}")
    return match.group()


HELPERS = "\n".join(function(name) for name in (
    "fail", "ui_diagnostics", "close_installed_ui", "require_ui_ready", "cleanup"))
MOCKS = r'''
PLATFORM=linux
APP_PID=801
SECOND_UI_PID=802
DAEMON_PID=901
XVFB_PID=902
UI_SMOKE_ID=marker-3
ticks=0
log() { echo "$*"; }
kill() {
    log "kill:$*"
    if [[ "$1" == -0 ]]; then
        if [[ "$SCENARIO" == stuck ]]; then return 0; fi
        if [[ "$2" == 801 ]]; then (( ticks < 2 ));
        elif [[ "$2" == 802 ]]; then (( ticks < 3 ));
        else return 1; fi
    fi
}
sleep() { ticks=$((ticks + 1)); SECONDS=$((SECONDS + 1)); log "tick:$ticks"; }
wait() { log "wait:$*"; return 143; }
ps() { log "diagnostic:$*"; }
poll_ui_ready() { log "poll:$*"; [[ "$SCENARIO" != missing ]]; }
'''


class LinuxLifecycleTests(unittest.TestCase):
    def run_shell(self, body, scenario="delayed"):
        return subprocess.run(["bash", "-c", "set -euo pipefail\n" + MOCKS + HELPERS + "\n" + body],
                              env=dict(os.environ, SCENARIO=scenario), text=True,
                              capture_output=True, timeout=5)

    def test_close_waits_for_both_owned_processes(self):
        result = self.run_shell('close_installed_ui close-ui; echo "closed:$APP_PID:$SECOND_UI_PID"')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        events = result.stdout.splitlines()
        self.assertLess(events.index("kill:802"), events.index("tick:1"))
        self.assertLess(events.index("tick:2"), events.index("wait:801"))
        self.assertLess(events.index("tick:3"), events.index("wait:802"))
        self.assertEqual(events[-1], "closed::")
        self.assertNotIn("kill:901", events)
        self.assertNotIn("kill:902", events)

    def test_stuck_close_fails_and_terminates_only_owned_ui_pids(self):
        result = self.run_shell("close_installed_ui close-ui; echo unexpected", "stuck")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("[step=close-ui]", result.stdout)
        self.assertIn("stage=close-ui smokeId=marker-3", result.stdout)
        self.assertIn("diagnostic:-p 802", result.stdout)
        self.assertIn("kill:-KILL 801", result.stdout)
        self.assertIn("kill:-KILL 802", result.stdout)
        self.assertNotIn("kill:-KILL 901", result.stdout)
        self.assertNotIn("unexpected", result.stdout)
        self.assertNotIn("wait:801", result.stdout)

    def test_missing_acknowledgment_keeps_failure_and_stage(self):
        result = self.run_shell("require_ui_ready reopen-ui; echo unexpected", "missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("poll:30", result.stdout)
        self.assertIn("[step=ui-hydration]", result.stdout)
        self.assertIn("stage=reopen-ui smokeId=marker-3", result.stdout)
        self.assertNotIn("unexpected", result.stdout)
        self.assertNotIn("args=", result.stdout)

    def test_valid_acknowledgment_does_not_emit_diagnostics(self):
        result = self.run_shell("require_ui_ready concurrent-launch")
        self.assertEqual(result.returncode, 0)
        self.assertNotIn("DIAGNOSTIC", result.stdout)

    def test_cleanup_signals_secondary_ui_even_after_failure(self):
        result = self.run_shell("cleanup")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.splitlines(), ["kill:801", "kill:802", "kill:901", "kill:902"])

    def test_private_bus_wrapper_reexecutes_once_and_forwards_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wrapper = root / "smoke-linux.sh"
            wrapper.write_text(SOURCE.split('PLATFORM="linux"', 1)[0] +
                               'printf "child:%s:%s\\n" "$HIFIMULE_SMOKE_DBUS_SESSION" "$1"\nexit 23\n')
            dbus = root / "dbus-run-session"
            dbus.write_text('#!/bin/bash\necho bus-start\n[[ "$1" == -- ]] || exit 99\nshift\nexec "$@"\n')
            dbus.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ["PATH"])
            env.pop("HIFIMULE_SMOKE_DBUS_SESSION", None)
            result = subprocess.run(["bash", str(wrapper), "argument with spaces"], env=env,
                                    text=True, capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
            self.assertEqual(result.stdout.splitlines(), ["bus-start", "child:1:argument with spaces"])

    def test_activation_receives_display_and_rendering_environment(self):
        fragment = SOURCE.split("# --- STEP 2: Launch ---", 1)[1].split('APP_BIN="hifimule-ui"', 1)[0]
        mocks = r'''
start_xvfb() { export DISPLAY=:117; }
dbus-update-activation-environment() {
    for name in "$@"; do echo "activation:$name=${!name}"; done
    [[ "$SCENARIO" != activation-failure ]]
}
'''
        result = self.run_shell(mocks + fragment)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for value in ("DISPLAY=:117", "GDK_BACKEND=x11", "LIBGL_ALWAYS_SOFTWARE=1", "WEBKIT_DISABLE_COMPOSITING_MODE=1"):
            self.assertIn("activation:" + value, result.stdout)
        self.assertLess(SOURCE.index("dbus-update-activation-environment DISPLAY"), SOURCE.index('"$APP_BIN" --smoke-id'))
        failure = self.run_shell(mocks + fragment + "echo unexpected", "activation-failure")
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn("[step=launch]", failure.stdout)
        self.assertNotIn("unexpected", failure.stdout)


if __name__ == "__main__":
    unittest.main()
