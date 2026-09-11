#!/usr/bin/env python3
"""Isolated FFmpeg PCM comparison adapter; no resampling or fixture-aware trimming."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

TIMEOUT_SECONDS = 30
MAX_PCM_BYTES = 64 * 1024 * 1024


def run(command, deadline=None, **kwargs):
    # subprocess.run kills and waits for the child on timeout.
    timeout = TIMEOUT_SECONDS if deadline is None else min(TIMEOUT_SECONDS, deadline - time.monotonic())
    if timeout <= 0:
        raise subprocess.TimeoutExpired(command, 45)
    return subprocess.run(command, check=True, timeout=timeout, **kwargs)


def probe(path, ffprobe, deadline=None):
    result = run([ffprobe, '-v', 'error', '-protocol_whitelist', 'file,pipe',
                  '-select_streams', 'a:0', '-show_entries',
                  'stream=sample_rate,channels,channel_layout,codec_name',
                  '-of', 'json', str(path)], capture_output=True, text=True, deadline=deadline)
    data = json.loads(result.stdout)
    streams = data.get('streams') if isinstance(data, dict) else None
    if not isinstance(streams, list) or len(streams) != 1 or not isinstance(streams[0], dict):
        raise ValueError('expected exactly one selected audio stream')
    stream = streams[0]
    raw_rate = stream.get('sample_rate')
    if not isinstance(raw_rate, str) or not raw_rate.isdecimal():
        raise ValueError('missing or invalid sample rate')
    rate = int(raw_rate)
    if not 8000 <= rate <= 384000 or type(stream.get('channels')) is not int or stream['channels'] != 2:
        raise ValueError('only stereo audio at 8–384 kHz is supported')
    # Untagged two-channel WAV has no layout; do not force a layout or remix it.
    layout = stream.get('channel_layout') or 'unspecified-stereo'
    if layout not in ('stereo', 'unspecified-stereo'):
        raise ValueError(f'unsupported channel layout: {layout}')
    if not isinstance(stream.get('codec_name'), str) or not stream['codec_name']:
        raise ValueError('missing codec identity')
    return rate, 2, layout


def decode(output, files):
    deadline = time.monotonic() + 45
    ffmpeg, ffprobe = shutil.which('ffmpeg'), shutil.which('ffprobe')
    if not ffmpeg or not ffprobe:
        raise ValueError('ffmpeg and ffprobe must be installed on PATH')
    paths = [Path(filename).resolve(strict=True) for filename in files]
    if not paths or any(not path.is_file() or path.stat().st_size == 0 for path in paths):
        raise ValueError('inputs must be nonempty regular local files')
    metadata = [probe(path, ffprobe, deadline) for path in paths]
    if any(item != metadata[0] for item in metadata):
        raise ValueError('sample rate or channel layout changes are unsupported')
    rate, channels, _ = metadata[0]
    records = []
    total = 0
    # Exclusive creation refuses existing files, symlinks and hard links alike.
    with open(output, 'xb') as destination:
        try:
            for path in paths:
                with tempfile.TemporaryFile() as pcm:
                    run([ffmpeg, '-nostdin', '-v', 'error', '-xerror',
                         '-err_detect', 'explode', '-protocol_whitelist', 'file,pipe',
                         '-i', str(path), '-map', '0:a:0', '-vn', '-sn', '-dn',
                         '-c:a', 'pcm_f32le', '-f', 'f32le', '-fs', str(MAX_PCM_BYTES),
                         'pipe:1'], stdout=pcm, stderr=subprocess.PIPE, deadline=deadline)
                    size = pcm.tell()
                    if size >= MAX_PCM_BYTES - 65536:
                        raise ValueError('decoded track exceeds probe size limit')
                    if size == 0 or size % (channels * 4):
                        raise ValueError('decoder produced zero or incomplete PCM frames')
                    pcm.seek(0)
                    shutil.copyfileobj(pcm, destination, length=64 * 1024)
                frames = size // (channels * 4)
                total += frames
                records.append({'file': str(path), 'frames': frames,
                                'channels': channels, 'sample_rate': rate})
        except BaseException:
            destination.close()
            Path(output).unlink(missing_ok=True)
            raise
    return records + [{'total_frames': total, 'channels': channels, 'sample_rate': rate}]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['decode'])
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('files', nargs='+')
    args = parser.parse_args()
    try:
        records = decode(args.output, args.files)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'FFmpeg decode failed: {error}', file=sys.stderr)
        return 1
    for record in records:
        print(json.dumps(record))
    return 0


if __name__ == '__main__':
    sys.exit(main())
