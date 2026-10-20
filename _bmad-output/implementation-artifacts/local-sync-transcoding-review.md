# Independent implementation review

Reviewed against baseline `a3cf28b6db7e2665a3104efe1a05ab5107c0a679` using three reviewers without conversation context: blind adversarial hunter (diff only), edge-case hunter (diff and project), and acceptance auditor (diff, project, approved spec and companion).

All findings below are implementation patches under existing approved requirements. No change to frozen intent or specification interpretation is required. Duplicate reports were merged.

| Finding | Evidence / impact | Classification | Disposition |
|---|---|---|---|
| Source preferences treated as Opus device ceilings | Source normalization inserts 44.1 kHz; local Opus resolution requires 48 kHz even when device permits it. | patch | Fixed; targeted regression and full suite pass |
| Invented PCM depth ceiling | Absent maximum becomes 24; source-derived 16-bit maximum also rejects explicitly requested PCM24/32 representation. | patch | Fixed; targeted regression and full suite pass |
| Original-fetch failure mistaken for server negotiation | Local mode has a conversion profile but sends no server profile; URL/404 failures incorrectly warn and skip manual work. | patch | Fixed; targeted regression and full suite pass |
| Auto-Fill non-audio response cancels whole sync | New validation does not use existing per-item Auto-Fill skip policy. | patch | Fixed; targeted regression and full suite pass |
| Ogg/Opus artwork loss | Attached-picture copying handles MP3/FLAC/MP4 only; Ogg needs picture metadata. | patch | Fixed; targeted regression and full suite pass |
| Unknown source properties inflate lossless reservation | Five minutes at fabricated 192 kHz/eight channels/four bytes plus overhead exceeds default staging budget before original download. | patch | Fixed; targeted regression and full suite pass |

Rejected candidates: global-to-Ogg metadata loss (the pinned muxer merges container tags into stream comments); FLAC32 capability alone (the pinned encoder defaults to 24-bit without experimental compliance, and profile maxima are ceilings). Cross-container metadata preservation remains included in corrective test coverage.

No unrelated pre-existing defects were accepted for deferral.

Final verification: **1,616 daemon unit tests and five contract tests passed**, 13 ignored, zero failures. Native corrective regressions cover all six findings. The reviewer-facing code trail is in the spec. No accepted finding remains unresolved. Platform-native packaging checks outside macOS ARM64 remain explicitly documented release-validation limits.
