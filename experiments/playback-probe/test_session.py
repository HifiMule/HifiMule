#!/usr/bin/env python3
"""Bounded desktop-session proof. Native transport and physical keys are distinct."""
import argparse
import csv
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


SUPERVISOR = r"""
import json, os, pathlib, subprocess, sys
command = json.loads(sys.argv[1])
out = pathlib.Path(sys.argv[2])
def publish(name, value):
    temporary = out / (name + ".tmp")
    temporary.write_text(str(value))
    temporary.replace(out / name)
with open(out / "owner.jsonl", "w") as stdout, open(out / "owner.stderr", "w") as stderr:
    options = {"stdin": subprocess.DEVNULL, "stdout": stdout, "stderr": stderr}
    if os.name == "nt":
        options["creationflags"] = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    print("supervisor: spawning owner", flush=True)
    child = subprocess.Popen(command, **options)
    print("supervisor: owner PID " + str(child.pid), flush=True)
    publish("owner-pid.txt", child.pid)
    try:
        code = child.wait(timeout=185)
    except subprocess.TimeoutExpired:
        child.kill()
        code = child.wait(timeout=5)
    publish("owner-exit-code.txt", code)
"""

LAUNCHER = r"""
import os, pathlib, subprocess, sys
out = pathlib.Path(sys.argv[3])
with open(out / "supervisor.stdout", "w") as stdout, open(out / "supervisor.stderr", "w") as stderr:
    options = {"stdin": subprocess.DEVNULL, "stdout": stdout, "stderr": stderr}
    if os.name == "nt":
        options["creationflags"] = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    supervisor = subprocess.Popen([sys.executable, "-c", sys.argv[1], sys.argv[2], sys.argv[3]], **options)
    print(supervisor.pid, flush=True)
"""


def require(condition, message="check failed"):
    if not condition:
        raise RuntimeError(str(message))


def valid_endpoint(path):
    try:
        value = json.loads(path.read_text())
        if (isinstance(value.get("pid"), int) and isinstance(value.get("port"), int)
                and isinstance(value.get("token"), str) and value.get("session_id")):
            return value
    except (OSError, ValueError):
        pass
    return None


def alive(pid):
    if os.name == "nt":
        result = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
                                capture_output=True, text=True, timeout=5)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return any(len(row) > 1 and row[1] == str(pid)
                   for row in csv.reader(io.StringIO(result.stdout)))
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def wait_for(check, seconds, description):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.1)
    raise RuntimeError("timed out: " + description)


def main():
    root = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, default=root / "target/debug/playback-probe")
    parser.add_argument("--device", required=True)
    parser.add_argument("--decoder", default="native-ffmpeg")
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--mpris", action="store_true", help="also issue real session-bus commands")
    parser.add_argument("--windows-native-remote", type=Path, help="external SMTC test client")
    parser.add_argument("--no-window", action="store_true", help="only disposable CLI controllers")
    parser.add_argument("files", nargs="*", type=Path)
    args = parser.parse_args()
    probe = str(args.probe.resolve())
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    info = out / "session-info.json"
    files = args.files or [root / "fixtures" / f"track-{n}.wav" for n in (1, 2, 3)]
    command = [probe, "session", "--info", str(info), "--seconds", "120", "--device",
               args.device, "--decoder", args.decoder, *[str(f.resolve()) for f in files]]
    report = {"scope": "isolated session and disposable controller; not production UI lifetime",
              "command": command, "native_transport": "not run", "physical_keys": "not run",
              "controller_windows": not args.no_window, "checks": [], "passed": False}
    pid = None
    supervisor_pid = None
    session_id = None

    def control(action, path=info, required=True):
        r = subprocess.run([probe, "session-control", "--info", str(path), action],
                           capture_output=True, text=True, timeout=5)
        if required and r.returncode:
            raise RuntimeError(f"{action}: {r.stderr.strip()}")
        if not required:
            return r
        rows = [json.loads(line) for line in r.stdout.splitlines() if line.strip()]
        return rows[-1]

    def snapshot(label):
        value = control("status")
        if session_id is not None:
            require(value["session_id"] == session_id and value["pid"] == pid, value)
        report["checks"].append({"label": label, "status": value})
        return value

    try:
        launched = subprocess.run([sys.executable, "-c", LAUNCHER, SUPERVISOR,
                                   json.dumps(command), str(out)],
                                  capture_output=True, text=True, timeout=10, check=True)
        supervisor_pid = int(launched.stdout.strip())
        report["supervisor_pid"] = supervisor_pid
        report["supervision"] = "detached test supervisor records owner exit; controller owns neither process"
        wait_for(lambda: (out / "owner-pid.txt").exists(), 15, "supervisor owner PID")
        pid = int((out / "owner-pid.txt").read_text())
        report["owner_pid"] = pid
        wait_for(lambda: valid_endpoint(info), 15, "complete session endpoint JSON")
        first = snapshot("launcher exited")
        session_id = first["session_id"]
        require(first["pid"] == pid)
        time.sleep(0.5)
        second = snapshot("independent owner advances")
        require(second["consumed_frames"] > first["consumed_frames"])

        if not args.no_window:
            for n in (1, 2):
                before = snapshot(f"before controller {n}")
                ui = subprocess.run([probe, "session-ui", "--info", str(info), "--close-after", "1"],
                                    capture_output=True, text=True, timeout=15)
                (out / f"controller-{n}.stdout").write_text(ui.stdout)
                (out / f"controller-{n}.stderr").write_text(ui.stderr)
                require(ui.returncode == 0, ui.stderr)
                after = snapshot(f"controller {n} closed")
                require(after["consumed_frames"] > before["consumed_frames"])

        control("pause")
        time.sleep(0.3)
        paused = snapshot("paused")
        time.sleep(0.4)
        held = snapshot("pause retained position")
        require(paused["paused"] and held["paused"])
        require(paused["consumed_frames"] == held["consumed_frames"])
        control("resume")
        time.sleep(0.4)
        resumed = snapshot("resumed")
        require(not resumed["paused"] and resumed["consumed_frames"] > held["consumed_frames"])

        bad = json.loads(info.read_text())
        bad["token"] = "incorrect-test-token"
        wrong = out / "wrong-info.json"
        fd = os.open(wrong, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            json.dump(bad, f)
        require(control("pause", wrong, required=False).returncode != 0)
        require(not snapshot("wrong token rejected")["paused"])

        if args.mpris:
            config = json.loads(info.read_text())
            name = config.get("dbus_name") or resumed.get("dbus_name")
            if not name:
                raise RuntimeError("session did not expose its MPRIS name")
            if not name.startswith("org.mpris.MediaPlayer2."):
                name = "org.mpris.MediaPlayer2." + name
            for method, paused_expected in (("Pause", True), ("Play", False)):
                r = subprocess.run(["gdbus", "call", "--session", "--dest", name,
                                    "--object-path", "/org/mpris/MediaPlayer2", "--method",
                                    "org.mpris.MediaPlayer2.Player." + method],
                                   capture_output=True, text=True, timeout=5)
                (out / ("mpris-" + method + ".stdout")).write_text(r.stdout)
                (out / ("mpris-" + method + ".stderr")).write_text(r.stderr)
                require(r.returncode == 0, r.stderr)
                wait_for(lambda: control("status")["paused"] == paused_expected, 3, method)
                time.sleep(0.1)  # Allow the currently executing audio callback to finish.
                before_native = snapshot("native MPRIS " + method)
                time.sleep(0.3)
                after_native = snapshot("native MPRIS progression " + method)
                require((after_native["consumed_frames"] == before_native["consumed_frames"]) if paused_expected
                        else (after_native["consumed_frames"] > before_native["consumed_frames"]), "native MPRIS consumption")
            report["native_transport"] = "MPRIS Pause and Play delivered through session bus"

        if args.windows_native_remote:
            for method, paused_expected in (("pause", True), ("play", False)):
                r = subprocess.run([str(args.windows_native_remote.resolve()), method],
                                   capture_output=True, text=True, timeout=10)
                (out / ("smtc-" + method + ".stdout")).write_text(r.stdout)
                (out / ("smtc-" + method + ".stderr")).write_text(r.stderr)
                require(r.returncode == 0, r.stderr)
                wait_for(lambda: control("status")["paused"] == paused_expected, 3, method)
                time.sleep(0.1)  # Allow the currently executing audio callback to finish.
                before_native = snapshot("native SMTC " + method)
                time.sleep(0.3)
                after_native = snapshot("native SMTC progression " + method)
                require((after_native["consumed_frames"] == before_native["consumed_frames"]) if paused_expected
                        else (after_native["consumed_frames"] > before_native["consumed_frames"]), "native SMTC consumption")
            report["native_transport"] = "SMTC Pause and Play delivered through Windows session API"

        final = snapshot("before quit")
        require(final["underrun_frames"] == 0, final)
        control("quit")
        wait_for(lambda: not info.exists(), 5, "endpoint cleanup")
        wait_for(lambda: not alive(pid), 5, "owner exit")
        wait_for(lambda: (out / "owner-exit-code.txt").exists(), 5, "recorded owner exit code")
        report["owner_exit_code"] = int((out / "owner-exit-code.txt").read_text())
        require(report["owner_exit_code"] == 0, "owner exited nonzero")
        wait_for(lambda: not alive(supervisor_pid), 5, "supervisor exit")
        report["clean_exit"] = True
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
    finally:
        try:
            if pid is None and (out / "owner-pid.txt").exists():
                pid = int((out / "owner-pid.txt").read_text())
            if pid is not None and alive(pid):
                try:
                    control("quit", required=False)
                    wait_for(lambda: not alive(pid), 3, "cleanup quit")
                except Exception:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True, timeout=5, check=True)
                    else:
                        os.kill(pid, signal.SIGKILL)
                    report["forced_cleanup"] = True
                    wait_for(lambda: not alive(pid), 5, "forced owner cleanup")
            if supervisor_pid is not None and alive(supervisor_pid):
                wait_for(lambda: not alive(supervisor_pid), 5, "cleanup supervisor exit")
        except Exception as error:
            report["cleanup_error"] = str(error)
            report["passed"] = False
            if supervisor_pid is not None:
                try:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(supervisor_pid), "/T", "/F"], capture_output=True, timeout=5)
                    else:
                        os.killpg(supervisor_pid, signal.SIGKILL)
                    wait_for(lambda: not alive(supervisor_pid), 5, "forced supervisor cleanup")
                except Exception as forced_error:
                    report["forced_supervisor_cleanup_error"] = str(forced_error)
        finally:
            report["endpoint_removed"] = not info.exists()
            (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
