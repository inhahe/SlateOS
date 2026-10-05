### TD6. `move_pages` per-page node error stores `-EINVAL` where Linux stores `-ENODEV`/`-EACCES` — RESOLVED 2026-06-12 (batch 549)

**Resolution:** `sys_move_pages` now stores `-ENODEV` for any non-zero
target node, matching `do_pages_move`'s `err = -ENODEV` path (out-of-range
or `!node_state(node, N_MEMORY)`). On a single-node box every node but 0
lacks `N_MEMORY`, so `-ENODEV` is correct for all of them; the `-EACCES`
"valid node not in `task_nodes`" branch is unreachable when only node 0 has
memory. Batch-105 self-test Case 4 updated to expect `[0, -ENODEV, 0]`.
Original analysis retained below for reference.

---

**What:** `sys_move_pages` (`kernel/src/syscall/linux.rs`), in move mode
(`nodes != NULL`), writes `status[i] = -EINVAL` for any requested target
node other than 0 (we only have node 0). Linux's `do_pages_move`
(`mm/migrate.c`) instead validates each target node and stores a per-page
error via `store_status`: `-ENODEV` when the node is out of range or has no
memory (`!node_state(node, N_MEMORY)`), or `-EACCES` when the node is valid
but not in `task_nodes` (`!node_isset(node, task_nodes)`). On a single-node
box, target node 1 would be `-ENODEV` (node 1 has no `N_MEMORY`), not
`-EINVAL`.

**Divergence:** observable only in `status[i]` for a deliberately-bogus
target node; the syscall return code (0) is unaffected. Batch-105 self-test
Case 4 currently asserts `status == [0, -EINVAL, 0]`.

**Why deferred (not fixed in batch 548):** batch 548 fixed two
independently-verified divergences (missing pid→ESRCH lookup; invented
E2BIG cap) and intentionally did **not** guess at the per-page errno. The
exact `do_pages_move` node-validity path (range check → `N_MEMORY` →
`node_isset(task_nodes)` → `store_status`) needs its own verbatim v6.6
verification before changing the stored errno.

**Proper fix:** verify `do_pages_move`/`add_page_for_migration`/`store_status`
against v6.6, then store `-ENODEV` for out-of-range / no-memory nodes and
`-EACCES` for valid-but-disallowed nodes, and update Case 4's expectation.
