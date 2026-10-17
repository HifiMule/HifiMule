# Investigation: Sync destination preserved

## Hand-off Brief

The user reports sync failing for `Music/Ace of Base/The Sign/01 - The Sign (radio edit).mp3` on this computer. The original guard combined inspection failures and unowned existing files. The approved change now allows selected music outputs to replace untracked destinations in the configured music folder; inspection failures retain their underlying cause. Regression tests cover replacement and preservation paths.

## Case Info

- Date opened: 2026-10-17
- Status: Implementation complete — runtime cause remains unconfirmed
- System: macOS development host; destination transport and installed app version unknown
- Evidence: user error report and local source

## Problem Statement

User reports: `Music/Ace of Base/The Sign/01 - The Sign (radio edit).mp3: Destination exists outside managed music or could not be inspected; file preserved`

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Reported error | Available | Exact message supplied by user |
| Source | Available | Exact match in hifimule-daemon/src/sync.rs:3805 |
| Runtime logs, destination manifest, device state | Missing | Needed to distinguish guard branches |
| Installed version | Missing | Local source may differ from installed application |

## Confirmed Findings

1. `hifimule-daemon/src/sync.rs:3798` first ensures the parent directory exists, then asks for the destination length. Either operation can fail and produce this message.
2. `hifimule-daemon/src/sync.rs:3803` also rejects an existing destination whose normalized path is absent from `owned_manifest_paths`.
3. `hifimule-daemon/src/sync.rs:3806` requests cancellation, removes the staged temporary file, and prevents further writer work. This branch precedes the destination write.

## Deduced Conclusions

The message alone does not prove the file exists or is outside the Music folder. In this code, ownership depends on manifest paths; an inspection failure produces the same message.

## Hypothesized Paths

| Hypothesis | Status | Confirmation / refutation |
| --- | --- | --- |
| Destination exists but its normalized path is not owned | Open | Compare destination metadata with the loaded ownership set |
| Parent directory creation or file length inspection fails | Open | Obtain underlying device I/O error |

## Investigation Backlog

| Priority | Task | Status |
| --- | --- | --- |
| High | Identify destination transport and installed version | Open |
| High | Trace ownership-set construction and device inspection implementation | Open |
| High | Inspect runtime diagnostics and destination manifest | Open |

## Missing Evidence

Transport (mounted disk or MTP), whether this device was previously synced elsewhere, installed version, and the original I/O error would separate ownership mismatch from inspection failure.

## Source Code Trace

Error origin: `hifimule-daemon/src/sync.rs:3805`. Trigger: staged provider file reaches the device writer. Condition: failed destination check, or existing destination absent from the manifest ownership set.

## Conclusion

High confidence in the local guard's behavior; actual runtime cause remains unconfirmed.

## Recommended Next Steps

Validate the next sync using the updated build. If reuse is still unexpectedly skipped, compare the new scanner diagnostics with the MP3 tags and effective output profile.

## Reproduction Plan

After identifying transport, exercise an existing unowned destination and an inspection failure independently to distinguish the two paths.

## Approved Resolution (2026-10-17)

The user clarified that a selected music destination should be overwritten when discovery cannot adopt it. Pre-sync reconciliation is selective reuse: it enumerates music, probes tags/encoding, and only adopts a unique compatible selected track. It is not a broad manifest rebuild. The old unconditional untracked-collision guard was replaced by music-specific folder containment and explicit inspection errors. Non-music ownership rules and deletion rules remain unchanged.

The new inspector validates relative paths and mounted canonical ancestry before creating parents, then checks destination length immediately before the existing staged verified write. Successful writes use the existing manifest checkpoint path and actual staged size. Failed source acquisition or verified writes do not claim ownership, and unrelated files are untouched.

Verification: scanner suite 9 passed; RPC suite (including preparation/execution and reuse) 169 passed / 1 ignored; sync suite 121 passed / 1 ignored; diff whitespace check clean. The initially prescribed `sync_plan_tests` filter selected no tests, so the real containing module `rpc::tests` was run. Sync/RPC runs required sandbox escalation for loopback fixture servers/macOS system configuration. No hardware MTP validation was performed, and this particular MP3's missing adoption remains unconfirmed.

The original source-line references above describe baseline `9ab9dc34e62daa58973c0d67ee1f14d5bc097ba4`; the current implementation is `inspect_sync_destination` and the writer call in `hifimule-daemon/src/sync.rs`.

Review also added operation-local collision protection: a later selected track cannot overwrite the first selected track at the same generated path. Backend atomicity was not expanded: deterministic MSC sibling staging and MTP delete-before-upload behavior remain preexisting limitations. Preservation tests cover source failure and injected pre-write failure, not all transport failures.


## Runtime follow-up: two selected catalog records (2026-10-17)

Read-only local diagnostics now confirm a second cause: two distinct selected source IDs generate the same Music destination. These are not proven identical recordings.

| Field | First source | Second source |
| --- | --- | --- |
| Track ID | `558mY8oZtpZyqR7qeuJqP1` | `3MPvUNc43yMrRVPjTst6nl` |
| Duration from `browse.getAlbum` | 253 seconds (4:13) | 210 seconds (3:30) |
| Source RPC size | 4,119,166 bytes | 3,435,105 bytes |
| Disc / track | 1 / 1 | 1 / 1 |
| Encoding | MP3, 128 kbps | MP3, 128 kbps |

Both returned records have title `The Sign (radio edit)`, artist `Ace of Base`, album `The Sign`, album ID `5bD5A1OynmntPslBvpFIBg`, artist ID `5RXNBBcAaumlTg6WCTiNcB`, and server ID `c896f5112ba9ce720a19ce76688cdedac185fe08c190c521288d2a48d535c4af`. The authenticated local `browse.getAlbum` read was successful; its token was kept internal. The API exposes no source filesystem path in these song records. The different durations establish different timed source entries, but do not establish which label/version is correct or whether either underlying file is damaged.

Evidence:

- `/Users/akartmann/Library/Application Support/HifiMule/daemon.log:1527` and `:1537` show both IDs selected in one operation. Lines 1535 and 1546 report corresponding staged sizes above; line 1542 records the first write succeeding.
- `/Volumes/FLASHDISK/.hifimule.json.zst`, decoded read-only, now assigns `Music/Ace of Base/The Sign/01 - The Sign (radio edit).mp3` to the first ID, with `isAutoFill=true` and timestamp `2026-10-17T09:03:12Z`.
- The read-only pending plan `/private/var/folders/n6/wxs4x7ns5s37fj2cjz_l57km0000gn/T/hifimule-sync-plan-fvImIn.sqlite` contains the first ID as a resident, the second as a `new-selection` addition with `isAutoFill=true`, and both in playlist track rows. This confirms that a writer-only skip would not fix subsequent sync planning.
- `rpc.rs:7434–7462` deduplicates expanded selections by source ID. `sync.rs:1394–1443` derives the destination from artist/album/track/title/extension, omitting source identity and duration. `sync.rs:4759–4800` calculates additions by source ID. These source references describe the current investigation snapshot and may shift during implementation.

Conclusion: the provider supplies two differently timed entries with identical destination metadata. Existing ID-based selection deduplication retains both, while destination construction collapses both to one pathname. Neither multi-server routing, separate album IDs, nor differing disc numbers explains this specific collision. The requested one-file policy needs stable destination-level ownership and playlist alias resolution across later syncs and deletion, rather than merely ignoring the second write. Prefer an existing selected resident, otherwise a deterministic representative; retain the duration distinction in diagnostics.
