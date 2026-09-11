#!/usr/bin/env python3
"""Measure decoded continuity. This does not certify gapless hardware output."""
import argparse
import array
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile


def floats(path):
    data = array.array("f")
    data.frombytes(path.read_bytes())
    if sys.byteorder != "little":
        data.byteswap()
    return data


def errors(actual, reference, start=0, end=None):
    end = min(len(actual), len(reference), end if end is not None else len(reference))
    if end <= start:
        return {"samples": 0, "rms": None, "max_abs": None}
    total = 0.0
    maximum = 0.0
    for index in range(start, end):
        difference = abs(actual[index] - reference[index])
        if not math.isfinite(difference):
            return {"samples": end - start, "rms": None, "max_abs": None,
                    "error": "non-finite decoded sample"}
        total += difference * difference
        maximum = max(maximum, difference)
    return {"samples": end - start, "rms": math.sqrt(total / (end - start)),
            "max_abs": maximum}


def decoder_command(name, probe, root):
    if name == "ffmpeg":
        return [sys.executable, str(root / "ffmpeg_decode.py")]
    return [str(probe.resolve())]


def tool_version(name):
    try:
        result = subprocess.run([name, "-version"], capture_output=True, text=True, timeout=5)
        if result.returncode:
            return {"error": result.stderr.strip(), "exit_code": result.returncode}
        lines = result.stdout.splitlines()
        return {"version": lines[0] if lines else "unreported",
                "configuration": next((line for line in lines if line.startswith("configuration:")), None)}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": str(error)}


def main():
    root = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, default=root / "fixtures")
    parser.add_argument("--probe", type=Path, default=root / "target/debug/playback-probe")
    parser.add_argument("--decoder", choices=("symphonia", "ffmpeg"), default="symphonia")
    parser.add_argument("--output", type=Path, default=root / "results/decode.json")
    parser.add_argument("--require", default="wav,flac",
                        help="comma-separated variants required to pass; others are exploratory")
    args = parser.parse_args()
    manifest = json.loads((args.fixtures / "manifest.json").read_text())
    reference = floats(args.fixtures / manifest["reference"])
    required = set(filter(None, args.require.split(",")))
    report = {"scope": "decoded PCM only; native output and streaming not tested",
              "decoder_backend": args.decoder,
              "sample_rate": manifest["sample_rate"], "channels": manifest["channels"],
              "expected_frames": manifest["total_frames"],
              "generation_failures": manifest.get("generation_failures", {}), "variants": {}}
    prefix = decoder_command(args.decoder, args.probe, root)
    report["decoder_command"] = prefix
    if args.decoder == "ffmpeg":
        report["tool_versions"] = {name: tool_version(name) for name in ("ffmpeg", "ffprobe")}
    with tempfile.TemporaryDirectory(prefix="hifimule-decode-") as temporary:
        for name, variant in manifest["variants"].items():
            output = Path(temporary) / f"{name}.f32le"
            command = [*prefix, "decode", "--output", str(output)]
            command.extend(str((args.fixtures / filename).resolve()) for filename in variant["files"])
            try:
                result = subprocess.run(command, capture_output=True, text=True, timeout=60)
            except (OSError, subprocess.TimeoutExpired) as error:
                report["variants"][name] = {"pass": False, "error": str(error)}
                continue
            if result.returncode:
                report["variants"][name] = {"pass": False, "error": result.stderr.strip(),
                                            "exit_code": result.returncode}
                continue
            try:
                metrics = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
                actual = floats(output)
            except (ValueError, OSError) as error:
                report["variants"][name] = {"pass": False, "error": str(error)}
                continue
            difference = errors(actual, reference)
            exact_length = len(actual) == len(reference)
            expected_tracks = manifest["track_frames"]
            metadata_ok = (
                len(metrics) == len(expected_tracks) + 1
                and all(isinstance(item, dict) for item in metrics)
                and all(item.get("channels") == manifest["channels"]
                        and item.get("sample_rate") == manifest["sample_rate"] for item in metrics)
                and [item.get("frames") for item in metrics[:-1]] == expected_tracks
                and metrics[-1].get("total_frames") == manifest["total_frames"])
            # Lossy tolerance is a diagnostic for these fixtures, not a universal quality threshold.
            tolerance = 0.000001 if variant["lossless"] else 0.02
            pass_signal = difference["rms"] is not None and difference["rms"] <= tolerance
            boundaries = []
            frame = 0
            for length in manifest["track_frames"][:-1]:
                frame += length
                sample = frame * manifest["channels"]
                window = 256 * manifest["channels"]
                boundaries.append({"frame": frame, **errors(actual, reference,
                                                            max(0, sample - window), sample + window)})
            pass_boundaries = all(b["rms"] is not None and b["rms"] <= tolerance for b in boundaries)
            report["variants"][name] = {
                "pass": exact_length and metadata_ok and pass_signal and pass_boundaries,
                "metadata_matches": metadata_ok,
                "decoded_frames": len(actual) / manifest["channels"],
                "exact_length": exact_length, "rms_tolerance": tolerance,
                "difference": difference, "boundaries": boundaries, "decoder": metrics}
    missing = required - report["variants"].keys()
    failed = sorted(name for name in required if not report["variants"].get(name, {}).get("pass"))
    report["required_missing"] = sorted(missing)
    report["required_failed"] = failed
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(json.dumps(report, indent=2, allow_nan=False))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
