# Third-party audio runtime notices

HifiMule's playback daemon links to controlled FFmpeg 9 libraries recorded in
`audio-runtime.json`. Linux and Windows use FFmpeg 9.0.1 built from the official
signed release source. The selected builds exclude GPL and nonfree components
and are
redistributed under FFmpeg's LGPL 2.1-or-later terms. FFmpeg is a trademark of
Fabrice Bellard, originator of the FFmpeg project.

The Rust bindings are `ffmpeg-next` 9.0.0 and `ffmpeg-sys-next` 9.0.0. Audio
output uses CPAL 0.16.0. Their complete license texts and attribution metadata are
available from the corresponding locked source packages.

Source offer and reproducible build details:

- https://ffmpeg.org/releases/ffmpeg-9.0.1.tar.xz
- SHA-256: `cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635`
- Detached signature: https://ffmpeg.org/releases/ffmpeg-9.0.1.tar.xz.asc
- Release signing-key fingerprint: `FCF986EA15E6E293A5644F10B4322F04D67658D8`
- Configure flags: see `audio-runtime.json`

The Windows build wrapper verifies the archive hash and detached signature in
an isolated keyring before compiling with MSVC. It also generates the required
MSVC import libraries from FFmpeg's installed module-definition files.
