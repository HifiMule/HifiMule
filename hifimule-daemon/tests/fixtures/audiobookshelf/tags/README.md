# Synthetic audiobook tag fixtures

Generated locally with FFmpeg from a 0.5-second 440 Hz sine wave, with no user media or metadata.

- `untagged.mp3`: `ffmpeg -f lavfi -i sine=frequency=440:duration=0.5 -c:a libmp3lame -id3v2_version 0 -write_id3v1 0 untagged.mp3`
- `chapters-faststart.m4a`: `ffmpeg -f lavfi -i sine=frequency=440:duration=0.5 -i chapters.txt -map_metadata 1 -map_chapters 1 -c:a aac -movflags +faststart chapters-faststart.m4a`

The MP4 has both a chapter list and a text chapter track. Tests add artwork/custom metadata, test fast-start and trailing movie layouts, independently demux audio packets through libavformat, and compare decoded PCM before/after through the bundled libavcodec. The FFmpeg CLI is needed only to regenerate fixtures.
