#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import ffmpeg_decode as adapter


class DecoderTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'input.wav'
        self.source.write_bytes(b'original audio')
        self.output = self.root / 'output.pcm'

    def decode(self, effect):
        with patch.object(adapter.shutil, 'which', side_effect=lambda name: name), \
             patch.object(adapter, 'probe', return_value=(48000, 2, 'stereo')), \
             patch.object(adapter, 'run', side_effect=effect):
            return adapter.decode(self.output, [self.source])

    def test_existing_files_and_aliases_unchanged(self):
        for alias in ('plain', 'hard', 'symbolic'):
            if alias == 'plain':
                self.output.write_bytes(b'keep me')
            elif alias == 'hard':
                self.output.hardlink_to(self.source)
            else:
                self.output.symlink_to(self.source)
            before = self.output.read_bytes()
            with self.assertRaises(FileExistsError):
                self.decode(lambda *args, **kwargs: self.fail('decoder must not run'))
            self.assertEqual(before, self.output.read_bytes())
            self.assertEqual(b'original audio', self.source.read_bytes())
            self.output.unlink()

    def test_exact_pcm_and_metadata(self):
        def write(*args, **kwargs):
            kwargs['stdout'].write(b'\0' * 24)
        records = self.decode(write)
        self.assertEqual(self.output.read_bytes(), b'\0' * 24)
        self.assertEqual(records[0]['frames'], 3)
        self.assertEqual(records[-1]['total_frames'], 3)

    def test_empty_fractional_and_oversize_output_removed(self):
        for size in (0, 7, adapter.MAX_PCM_BYTES):
            def write(*args, **kwargs):
                if size:
                    kwargs['stdout'].seek(size - 1)
                    kwargs['stdout'].write(b'\0')
            with self.assertRaises(ValueError):
                self.decode(write)
            self.assertFalse(self.output.exists())

    def test_errors_and_timeouts_remove_partial_output(self):
        for error in (subprocess.CalledProcessError(1, ['ffmpeg']),
                      subprocess.TimeoutExpired(['ffmpeg'], 30)):
            with self.assertRaises(type(error)):
                self.decode(lambda *args, **kwargs: (_ for _ in ()).throw(error))
            self.assertFalse(self.output.exists())

    def test_metadata_validation(self):
        valid = {'sample_rate': '48000', 'channels': 2, 'codec_name': 'flac',
                 'channel_layout': 'stereo'}
        for change in ({'sample_rate': '0'}, {'sample_rate': 48000}, {'channels': 1},
                       {'channels': True}, {'channel_layout': 'downmix'}, {'codec_name': None}):
            stream = valid | change
            with patch.object(adapter, 'run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout=json.dumps({'streams': [stream]}))):
                with self.assertRaises(ValueError):
                    adapter.probe(self.source, 'ffprobe')
        for malformed in ('not json', '[]', '{}', '{"streams": [null]}'):
            with patch.object(adapter, 'run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout=malformed)):
                with self.assertRaises(ValueError):
                    adapter.probe(self.source, 'ffprobe')

    def test_changed_format_fails_before_output(self):
        for second in ((44100, 2, 'stereo'), (48000, 2, 'unspecified-stereo')):
            with patch.object(adapter.shutil, 'which', side_effect=lambda name: name), \
                 patch.object(adapter, 'probe', side_effect=[(48000, 2, 'stereo'), second]):
                with self.assertRaises(ValueError):
                    adapter.decode(self.output, [self.source, self.source])
            self.assertFalse(self.output.exists())

    def test_subprocess_deadline_enforced(self):
        with patch.object(adapter, 'TIMEOUT_SECONDS', 0.05):
            with self.assertRaises(subprocess.TimeoutExpired):
                adapter.run([sys.executable, '-c', 'import time; time.sleep(10)'])

    def test_expired_shared_deadline_does_not_launch_child(self):
        with patch.object(adapter.subprocess, "run") as child:
            with self.assertRaises(subprocess.TimeoutExpired):
                adapter.run(["ffmpeg"], deadline=0)
            child.assert_not_called()

    def test_missing_tools_and_nonregular_input(self):
        with patch.object(adapter.shutil, 'which', return_value=None):
            with self.assertRaises(ValueError):
                adapter.decode(self.output, [self.source])
        with patch.object(adapter.shutil, 'which', return_value='tool'):
            with self.assertRaises(ValueError):
                adapter.decode(self.output, [self.root])


if __name__ == '__main__':
    unittest.main()
