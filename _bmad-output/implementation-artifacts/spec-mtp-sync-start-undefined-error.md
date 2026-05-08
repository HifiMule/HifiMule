---
title: 'MTP sync start undefined error'
type: 'bugfix'
created: '2026-05-03'
status: 'done'
baseline_commit: 'NO_VCS'
context: []
---

<frozen-after-approval reason="human-owned intent - do not modify unless human renegotiates">

## Intent

**Problem:** Starting a sync on an initialized MTP device can fail with the UI message `Failed to start sync: undefined`, hiding the actual daemon or Tauri error that would explain why sync did not start.

**Approach:** Normalize RPC rejection values at the UI RPC boundary so callers receive an `Error` with a stable message, and harden the sync error rendering path against missing file-level error fields.

## Boundaries & Constraints

**Always:** Preserve the existing two-step sync flow: `sync_calculate_delta` followed by `sync_execute`. Keep the fix in the UI layer unless investigation proves the daemon is returning a malformed success response.

**Ask First:** Any change to MTP device initialization semantics, daemon sync execution, or manifest ownership rules.

**Never:** Do not suppress the failure, auto-clear the basket, or replace specific daemon errors with a generic message when a specific message is available.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Tauri string rejection | `invoke('rpc_proxy')` rejects with `"No device selected"` | Sync error panel says `Failed to start sync: No device selected` | Preserve string message |
| Error object rejection | RPC caller catches an `Error` | Existing `(err as Error).message` call sites receive a real message | Preserve original `Error` |
| Unknown rejection | RPC rejects with null, empty string, or unexpected value | UI shows a non-empty fallback error | Use `Unknown RPC error` |
| Failed operation errors | `sync_get_operation_status` returns `failed` with missing `errorMessage` | Error list stays readable and never includes `undefined` | Use `Unknown file error` fallback |

</frozen-after-approval>

## Code Map

- `hifimule-ui/src/rpc.ts` -- shared Tauri invoke wrapper for daemon RPC calls; best place to normalize rejection shape.
- `hifimule-ui/src/components/BasketSidebar.ts` -- sync start catch block and sync failure panel rendering.
- `hifimule-ui/src-tauri/src/lib.rs` -- confirms `rpc_proxy` returns `Result<serde_json::Value, String>`, so TypeScript may catch strings.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-ui/src/rpc.ts` -- wrap `invoke('rpc_proxy')` failures in `Error` with a normalized message -- prevents all RPC call sites from seeing string/plain-object rejections.
- [x] `hifimule-ui/src/components/BasketSidebar.ts` -- avoid `undefined` in operation failure entries -- keeps sync failure details readable.

**Acceptance Criteria:**
- Given a sync-start RPC rejects with a string, when the basket shows the failure panel, then the panel includes that string after `Failed to start sync:` instead of `undefined`.
- Given a sync operation fails with incomplete per-file error payloads, when the failure panel renders, then no list item contains the literal text `undefined`.
- Given TypeScript compilation runs, when the UI build checks the changed files, then no type errors are introduced.

## Spec Change Log

## Verification

**Commands:**
- `rtk npm run build --prefix hifimule-ui` -- expected: TypeScript and Vite build succeed.

## Suggested Review Order

**RPC Error Boundary**

- Normalize Tauri rejection shapes before UI callers inspect `.message`.
  [`rpc.ts:7`](../../hifimule-ui/src/rpc.ts#L7)

- Re-throw normalized RPC failures as real `Error` instances.
  [`rpc.ts:46`](../../hifimule-ui/src/rpc.ts#L46)

**Sync Error Rendering**

- Prevent incomplete operation error payloads from rendering `undefined`.
  [`BasketSidebar.ts:1019`](../../hifimule-ui/src/components/BasketSidebar.ts#L1019)
