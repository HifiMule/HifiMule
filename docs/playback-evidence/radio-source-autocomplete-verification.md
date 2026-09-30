# Radio source autocomplete verification

## Changes checked

- Source selection searches beyond the initial catalog page and retains provider IDs independently of displayed names.
- Saved artist names resolve by exact reference, including artists added from the library outside the initial page.
- Autocomplete supports keyboard selection, duplicate names, pagination, and rejection of stale replies.
- Jellyfin artist startup and new radio scan cursors use bounded direct track requests, retaining recording and quality metadata. Persisted album cursors retain their previous interpretation.

## Live API evidence

Checked the supplied Jellyfin 12.1.0 test server with Warrant. Expanding its nine albums sequentially took 23.66 seconds, exceeding radio startup's 15-second deadline. A direct artist-track request returned 96 tracks in 0.15 seconds. This was API verification; a complete audible playback session was not exercised.

## Verification results

- TypeScript check and production interface build: passed.
- JavaScript suite: 294 passed, six skipped.
- Daemon suite: 1,354 passed, seven ignored. All new Jellyfin and OpenSubsonic radio regressions passed.
- Native UI suite: ten passed; documentation tests passed.
- Independent code review: no remaining actionable findings after fixing pagination focus.

The full Rust workspace is not completely green. Unchanged lifecycle tests failed with Windows `LocalAccessDenied` / OS error 5:

- `simultaneous_activation_requests_coalesce_to_a_valid_request` (also failed independently).
- `ui_loser_exits_promptly_after_activation_acknowledgment` (encountered in the serial workspace retry).

One parallel retry failed `rpc::tests::rejected_control_does_not_supersede_pending_selection` with `PLAYBACK_SELECTION_CANCELLED` instead of `PLAYBACK_SELECTION_SOURCE_UNAVAILABLE`; it passed in the initial daemon suite and in the complete serial daemon run. No lifecycle or unrelated concurrency implementation was changed.
