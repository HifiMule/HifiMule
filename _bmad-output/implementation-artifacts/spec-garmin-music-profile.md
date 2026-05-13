---
title: 'Garmin Music Watch Device Profile'
type: 'feature'
created: '2026-05-13'
status: 'done'
route: 'one-shot'
---

## Intent

**Problem:** The `device-profiles.json` asset had no profile for Garmin music-enabled watches (Forerunner, Fenix, Venu, Vivoactive). Users assigning a transcoding profile to a Garmin device had to fall back to a generic MP3 profile that doesn't express the watch's native AAC/M4A capability, causing unnecessary re-encoding of AAC files already present in Jellyfin.

**Approach:** Add a `garmin-music` entry to `hifimule-daemon/assets/device-profiles.json` that passes through MP3 and AAC/M4A directly, transcodes all other formats (FLAC, OGG, Opus, WAV, etc.) to MP3 320 kbps, and caps channels/sample-rate to guard against hi-res or surround sources bypassing transcoding.

## Suggested Review Order

- [device-profiles.json](../../hifimule-daemon/assets/device-profiles.json) — new `garmin-music` profile entry at the end of the `profiles` array

## Spec Change Log

- 2026-05-13: Adversarial review — removed bare `aac` container DirectPlayProfile (unreliable Jellyfin behavior); added `MaxAudioChannels: 2` and `MaxAudioSampleRate: 48000` to prevent multi-channel/hi-res sources from being sent direct-play to the watch.

## Design Notes

Garmin music watches support MP3 and AAC/M4A natively but not FLAC, OGG, Opus, or WAV (per Garmin support FAQ JyNEOTsZaR3KMXqej3oQp5). The `mp4` container with `aac` codec covers `.m4a` files (Jellyfin normalizes `.m4a` to the `mp4` container internally). The bare `aac` container (raw ADTS) was intentionally excluded — Jellyfin's handling of this container for direct play is inconsistent across versions and would silently fall back to transcoding anyway.

`MaxAudioChannels: 2` and `MaxAudioSampleRate: 48000` ensure that any hi-res (96/192 kHz) or surround-encoded source triggers the MP3 transcode path rather than being sent as-is to a device that cannot decode it.
