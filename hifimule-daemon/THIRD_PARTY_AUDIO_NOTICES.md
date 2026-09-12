# Third-party audio runtime notices

HifiMule's playback daemon links to controlled FFmpeg 9 libraries recorded in
`audio-runtime.json`. Linux uses FFmpeg 9.0.1 built from the official release
source; Windows uses the separately identified, hash-pinned LGPL shared build
below. The selected distributions exclude GPL and nonfree components and are
redistributed under FFmpeg's LGPL 2.1-or-later terms. FFmpeg is a trademark of
Fabrice Bellard, originator of the FFmpeg project.

The Rust bindings are `ffmpeg-next` 9.0.0 and `ffmpeg-sys-next` 9.0.0. Audio
output uses CPAL 0.16.0. Their complete license texts and attribution metadata are
available from the corresponding locked source packages.

Source offer and reproducible build details:

- https://ffmpeg.org/releases/ffmpeg-9.0.1.tar.xz
- SHA-256: `cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635`
- Configure flags: see `audio-runtime.json`

Windows packages use the LGPL shared build of FFmpeg revision
`n9.0.1-27-g9b0578816c` published by BtbN/FFmpeg-Builds in the dated
`autobuild-2026-09-10-15-31` release. The architecture-specific immutable
archive names, download URLs and SHA-256 digests are recorded in
`audio-runtime.json`; the archives include their corresponding license files.
