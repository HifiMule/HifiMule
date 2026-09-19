# Third-party audio runtime notices

HifiMule's playback daemon links to controlled FFmpeg 9 libraries recorded in
`audio-runtime.json`. Windows, macOS and Linux use FFmpeg 9.0.2 built from the
official signed release source. The selected builds exclude GPL and nonfree components
and are
redistributed under FFmpeg's LGPL 2.1-or-later terms. FFmpeg is a trademark of
Fabrice Bellard, originator of the FFmpeg project.

The Rust bindings are `ffmpeg-next` 9.0.0 and `ffmpeg-sys-next` 9.0.0. Audio
output uses the repository's patched CPAL 0.18.2. Linux shared output uses
`libpulse-binding` 2.30.1. Native transport integration uses Souvlaki 0.8.3.
Their complete license texts and attribution metadata are
available from the corresponding locked source packages.

Source offer and reproducible build details:

- https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- SHA-256: `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`
- Detached signature: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz.asc
- Release signing-key fingerprint: `FCF986EA15E6E293A5644F10B4322F04D67658D8`
- Configure flags: see `audio-runtime.json`

The Windows build wrapper verifies the archive hash and detached signature in
an isolated keyring before compiling with MSVC. It also generates the required
MSVC import libraries from FFmpeg's installed module-definition files. The
macOS and Linux wrappers verify the same pinned archive hash, build with the
manifest's LGPL-compatible reduced feature set and record a target-specific
receipt. Every installer includes this notice and `audio-runtime.json`.
