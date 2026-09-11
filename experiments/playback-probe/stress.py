#!/usr/bin/env python3
"""Run a silent native probe alongside bounded temporary-file writes (not real sync)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time


def main():
    root = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, default=root / "target/debug/playback-probe")
    parser.add_argument("--device", required=True)
    parser.add_argument("--output", type=Path, default=root / "results/synthetic-io.json")
    args = parser.parse_args()
    stop = threading.Event()
    stats = {"bytes_written": 0, "error": None}

    def writer(path):
        try:
            block = bytes(1024 * 1024)
            with path.open("wb") as target:
                while not stop.is_set():
                    target.seek(0)
                    for _ in range(16):
                        if stop.is_set():
                            break
                        target.write(block)
                        stats["bytes_written"] += len(block)
                    target.flush()
                    os.fsync(target.fileno())
                    # Bounded 16 MiB scratch file and intentional pacing.
                    stop.wait(0.01)
        except Exception as error:
            stats["error"] = str(error)

    with tempfile.TemporaryDirectory(prefix="hifimule-io-probe-") as temporary:
        worker = threading.Thread(target=writer, args=(Path(temporary) / "scratch",))
        worker.start()
        started = time.monotonic()
        try:
            result = subprocess.run(
                [str(args.probe.resolve()), "play", "--volume", "0", "--device", args.device,
                 *(str(root / "fixtures" / f"track-{index}.wav") for index in (1, 2, 3))],
                capture_output=True, text=True, timeout=60)
            exit_code, stdout, stderr = result.returncode, result.stdout, result.stderr
        except (OSError, subprocess.TimeoutExpired) as error:
            exit_code, stdout, stderr = 1, "", str(error)
            if isinstance(error, subprocess.TimeoutExpired):
                raw = error.stdout or b""
                stdout = raw.decode(errors="replace") if isinstance(raw, bytes) else raw
        finally:
            stop.set()
            worker.join()
        elapsed = time.monotonic() - started
    report = {"scope": "silent native callback with synthetic local I/O, not real device sync",
              "seconds": elapsed, **stats, "exit_code": exit_code,
              "probe_stdout": stdout, "probe_stderr": stderr,
              "physical_audio_verified": False}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return exit_code or (1 if stats["error"] else 0)


if __name__ == "__main__":
    raise SystemExit(main())
