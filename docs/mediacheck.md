# Audit a library with mediacheck

`mediacheck` checks local audio sources before you sync them with HifiMule. It recursively reads a folder, fully decodes each supported source, writes a report, and displays progress and final counts. Originals remain unchanged, and no transcoded copies are created.

```sh
mediacheck "/path/to/Music" --log "/path/to/library-audit.log"
```

The only required argument is a folder. Without `--log`, the tool creates a unique `mediacheck-*.log` in your current working directory. An explicit report path must not already exist. Its parent directory must exist and be writable. A report created within the audited folder is excluded from the scan.

Use `mediacheck --help` for options and `mediacheck --version` for its HifiMule release version. No running HifiMule application, server connection, device, system FFmpeg installation, or target profile is required.

## Installed locations

The installers include the console tool alongside HifiMule and its controlled audio runtime. They do not change your shell's PATH automatically.

On macOS, run the executable inside the installed application:

```sh
"/Applications/HifiMule.app/Contents/MacOS/mediacheck" "$HOME/Music" --log "$HOME/Desktop/library-audit.log"
```

On Windows, open PowerShell and run `mediacheck.exe` from the HifiMule installation directory. The executable sits beside the other application executables; substitute your installation location:

```powershell
& "C:\path\to\HifiMule\mediacheck.exe" "D:\Music" --log "$env:USERPROFILE\Desktop\library-audit.log"
```

On Linux, the deb/rpm installation provides `/usr/bin/mediacheck`:

```sh
mediacheck "$HOME/Music" --log "$HOME/library-audit.log"
```

For an AppImage, extract it and run the packaged executable, preserving the extracted directory structure and its native libraries:

```sh
./HifiMule.AppImage --appimage-extract
./squashfs-root/usr/bin/mediacheck "$HOME/Music" --log "$HOME/library-audit.log"
```

## Windows shared folders

UNC paths and mapped drives are supported directly. Keep quotes around paths containing spaces or non-ASCII characters. The report can be local while the source library is on a share:

```powershell
& "C:\path\to\HifiMule\mediacheck.exe" "\\server\Music share\Björk" --log "$env:USERPROFILE\Desktop\shared-library-audit.log"
& "C:\path\to\HifiMule\mediacheck.exe" "Z:\Music share\Björk" --log "$env:USERPROFILE\Desktop\mapped-library-audit.log"
```

Run under a Windows account that already has access to the share. The tool uses ordinary filesystem paths and your current account's filesystem permissions. It neither changes directories nor establishes network-share credentials. A disconnected drive or inaccessible subtree makes the audit incomplete, with exit code 2.

## What the results mean

A healthy source produces one escaped `OK` line in the report. A `RECOVERABLE` source decoded within HifiMule's existing damage limits and passed source completeness checks. Its report includes invalid-event counts, charged bytes, first error position when known, probed and decoded durations, and the duration estimation method. Audible defects may remain.

A `BAD` source failed opening, decoding, recovery limits, or source completeness validation. The report includes the full available diagnostic chain and source/recovery evidence when decoding completed. Bad media do not stop the remaining scan.

The summary reports:

| Count | Meaning |
| --- | --- |
| Checked files | Supported source files attempted, including healthy, recoverable and bad files |
| Ignored files | Other file types, AppleDouble `._*` sidecars, symlinks, Windows junctions and other path redirects, and non-regular filesystem entries |
| Recoverable files | Checked files accepted with tolerated decoding damage; separate from bad files |
| Bad files | Checked files rejected by the source checks |
| Operational errors | Directory enumeration or filesystem inspection failures that leave the audit incomplete |

Ordinary directories and the active report do not count as ignored files. Symlinks encountered during traversal are skipped, including links to directories, to prevent cycles. Windows directory junctions and other path-redirecting reparse points are skipped too. Cloud placeholder directories remain traversable. The explicitly supplied root folder may itself resolve through a symlink.

Filenames and diagnostics are escaped in reports and console progress so embedded newlines cannot forge results. Hidden audio files are checked; AppleDouble sidecars are ignored because they contain Finder metadata. A filename that cannot be represented as UTF-8 is reported as bad with an explicit path diagnostic; rename it to audit its audio content. The remaining scan continues.

## Shared configuration and limits

Both HifiMule sync and mediacheck call the same source-open, decoder, MP3 metadata-tail, error recovery and duration-validation implementation in `hifimule-daemon/src/source_audio.rs`. They link the same controlled runtime built from `hifimule-daemon/audio-runtime.json`. Source policy changes and runtime changes apply to both tools when HifiMule is rebuilt; there is no separate mediacheck recovery configuration.

Scannable extensions come from the enabled runtime demuxers. Shared conventional aliases supplement demuxers whose extension metadata is incomplete, covering WAV/WAVE, AIFF/AIF/AIFC/AFC, ASF/WMA/WMV, Ogg/OGA/Opus and AC3 when their demuxers are enabled. Other currently recognized containers include MP3, FLAC, AAC and MP4/M4A/M4B/MOV. An extension selects a container candidate; a recognized container containing an unsupported codec, no audio, or corrupt data is bad. Image-only AVIF/HEIC/HEIF suffixes advertised by the MOV parser are excluded. AC3 sources (including `.AC3`) are decoded and checked; healthy mono, stereo and known-layout 5.1 sources are supported. Containers absent from the controlled runtime are ignored.

The checks keep HifiMule's existing bounded recovery rules and trusted-duration protection. AC3 CRC failures and frames marked damaged by the native decoder enter those recovery limits; concealed audio is excluded from decoded duration and encoding. Estimated bitrate durations may include metadata, so those use decoded samples instead. Trusted source durations retain the existing two-second allowance. This is an audit of source compatibility and detected integrity problems; it does not certify every target encoder, conversion setting, output container, or undetectable source corruption.

Files are decoded serially with bounded frame buffers and a streaming directory walk. A large or remote library can take time because every candidate is read completely. Avoid modifying a library while it is being audited.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Audit completed with no bad media; recoverable files may be present |
| `1` | Audit completed and found bad media |
| `2` | Usage, report writing, or filesystem traversal failure; results may be incomplete |

If an operational failure occurs after the report was created, retain the partial report and address the error before treating the library as fully audited.

## Development

Use the verified runtime wrapper when building or testing:

```sh
rtk npm run build:daemon -- build -p hifimule-daemon --bin mediacheck
rtk npm run build:daemon -- run -p hifimule-daemon --bin mediacheck -- "/path/to/Music" --log "/path/to/audit.log"
rtk npm run build:daemon -- test -p hifimule-daemon --lib
rtk npm run build:daemon -- test -p hifimule-daemon --test mediacheck_cli
```

The package's default run target remains `hifimule-daemon`. CLI integration tests audit temporary fixtures, including deliberately damaged MP3s made by the controlled test runtime; they do not scan user libraries.
