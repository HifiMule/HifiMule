# HifiMule Souvlaki patch

This directory vendors Souvlaki 0.8.3 under its original MIT license.

The repository-local patch adds explicit per-command capabilities, truthful
MPRIS application capabilities, macOS Stop support and metadata cleanup,
Windows replacement metadata and complete event-token cleanup. HifiMule keeps
the default D-Bus backend and does not enable the optional zbus backend.

The D-Bus publisher stores only the latest value of each property and processes
all pending properties together. Detach discards pending publication and sets a
terminal flag; the bus poll is capped at 20 ms so shutdown does not wait behind
old metadata or progress. Portable mailbox tests cover burst coalescing and
terminal shutdown. D-Bus interface tests exercise actual Crossroads dispatch,
callback rejection, dynamic capabilities, metadata clearing, and unsupported
seek/URI calls; these tests need the native libdbus development library.

macOS temporary NSString allocations use owned `StrongPtr` guards, releasing
the caller's ownership after Cocoa retains or copies each argument.

Story 15.7 adds checked seek handling: metadata carries an occurrence-derived
MPRIS track object path, stale `SetPosition` identities are ignored, signed
microseconds are preserved until validation, and `Seeked(i64)` is coalesced and
published only after HifiMule reports a committed discontinuity. macOS uses the
public `positionTime` accessor with finite/nonnegative validation. Windows
rejects negative `TimeSpan` requests before converting them to `Duration` and
uses HifiMule's fixed 10-second directional step.
