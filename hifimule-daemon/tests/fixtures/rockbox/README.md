# Rockbox binary fixtures

`generate.py` independently packs the official C layouts with Python `struct`;
it does not call HifiMule's reader or writer. The four directories cover v0x0f
(Rockbox 3.15, 22 slots) and v0x10 (Rockbox 4.0, 23 slots), little and big endian.
Each has one track with UTF-8 title `Zèbre`, path `/Music/song.mp3`, play count
12, rating 7, and nonzero play/resume history.

The master header is six 32-bit words. Rows have the release tag count plus a
flags word; string files have three-word headers and length/owner/string records.
Regenerate with `rtk proxy python hifimule-daemon/tests/fixtures/rockbox/generate.py`.
The Rust tests must continue to read these fixtures independently of encoding.
