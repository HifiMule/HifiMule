# 🔥 CODE REVIEW FINDINGS

**Story:** 3.2 The Live Selection Basket
**Git vs Story Discrepancies:** 1 found
**Issues Found:** 1 High, 3 Medium, 1 Low

## 🔴 CRITICAL / HIGH ISSUES
- **Hardcoded Port**: `jellysync-ui/src/components/BasketSidebar.ts` uses `http://localhost:19140` directly. This will break if the daemon port is configured differently (env `VITE_RPC_PORT` is ignored here).

## 🟡 MEDIUM ISSUES
- **Performance (RPC)**: `handle_jellyfin_get_item_counts` in `rpc.rs` processes item IDs serially (`for id_val in ids`). It should use `futures::future::join_all` to fetch metadata in parallel.
- **Test Quality**: `rpc.rs` has a test `test_rpc_get_item_counts_basic` but it only checks error cases. There is no success case verification (even with a mock), leaving the happy path unverified by automated tests.
- **Documentation Discrepancy**: Story lists `MediaCard.ts` as `[MODIFY]`, but it appears as an Untracked file in git. It was likely extracted or created new without updating the story metadata to `[NEW]`.

## 🟢 LOW ISSUES
- **Optimization**: `MediaCard.ts` fetches item counts individually on every click. While acceptable for MVP, it triggers N+1 RPC calls. Ideally, `recursive_item_count` should be fetched during the initial grid load using `IncludeItemTypes` or `Fields` param if supported.

## 🛠️ Recommended Actions
1. **Fix automatically**: I will fix the hardcoded port, parallelize the RPC handler, and update the story file.
2. **Create action items**: Add these as tasks for later.
3. **Show details**: Discuss specific items.
