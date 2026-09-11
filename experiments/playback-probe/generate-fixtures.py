#!/usr/bin/env python3
"""Generate original, deterministic continuity fixtures; FFmpeg is optional."""
import argparse
import array
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys
import wave


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(__file__).parent / "fixtures")
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    rate = 48000
    lengths = [96017, 72011, 120013]
    pcm = array.array("h")
    for frame in range(sum(lengths)):
        t = frame / rate
        # Continuous, quiet stereo test signal. Non-round boundaries expose padding.
        for frequency in (317, 503):
            sample = 0.12 * math.sin(2 * math.pi * frequency * t)
            sample += 0.035 * math.sin(2 * math.pi * (frequency * 2.31) * t)
            pcm.append(round(sample * 32767))

    reference = array.array("f", (value / 32768 for value in pcm))
    if sys.byteorder != "little":
        reference.byteswap()
        pcm.byteswap()
    (root / "reference.f32le").write_bytes(reference.tobytes())
    tracks = []
    offset = 0
    for index, count in enumerate(lengths, 1):
        name = f"track-{index}.wav"
        with wave.open(str(root / name), "wb") as output:
            output.setnchannels(2)
            output.setsampwidth(2)
            output.setframerate(rate)
            output.writeframes(pcm[offset * 2:(offset + count) * 2].tobytes())
        offset += count
        tracks.append(name)

    manifest = {"sample_rate": rate, "channels": 2, "track_frames": lengths,
                "total_frames": sum(lengths), "reference": "reference.f32le",
                "variants": {"wav": {"files": tracks, "lossless": True}},
                "generation_failures": {}}
    encoder = shutil.which("ffmpeg")
    codecs = {
        "flac": ("flac", ["-c:a", "flac"], True),
        "alac": ("m4a", ["-c:a", "alac"], True),
        "mp3": ("mp3", ["-c:a", "libmp3lame", "-b:a", "192k"], False),
        "aac": ("aac.m4a", ["-c:a", "aac", "-b:a", "192k"], False),
        "vorbis": ("ogg", ["-c:a", "libvorbis", "-q:a", "6"], False),
        "opus": ("opus", ["-c:a", "libopus", "-b:a", "160k"], False),
    }
    for name, (extension, options, lossless) in codecs.items():
        if encoder is None:
            manifest["generation_failures"][name] = "ffmpeg unavailable"
            continue
        files = []
        for index, source in enumerate(tracks, 1):
            target = f"track-{index}.{extension}"
            try:
                result = subprocess.run(
                    [encoder, "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
                     "-i", str(root / source), *options, str(root / target)],
                    capture_output=True, text=True, timeout=60)
            except (OSError, subprocess.TimeoutExpired) as error:
                manifest["generation_failures"][name] = str(error)
                break
            if result.returncode:
                manifest["generation_failures"][name] = result.stderr.strip()
                break
            files.append(target)
        else:
            manifest["variants"][name] = {"files": files, "lossless": lossless}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"manifest": str(root / "manifest.json"),
                      "variants": list(manifest["variants"]),
                      "generation_failures": manifest["generation_failures"]}, indent=2))


if __name__ == "__main__":
    main()
