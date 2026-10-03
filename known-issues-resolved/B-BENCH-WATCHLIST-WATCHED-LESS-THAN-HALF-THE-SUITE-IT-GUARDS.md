### [A] B-BENCH-WATCHLIST-WATCHED-LESS-THAN-HALF-THE-SUITE-IT-GUARDS. `BENCH_CRITICAL_PATHS` omitted idt.rs, fs/, net/ and crypto.rs — FIXED 2026-08-14

**Where:** `scripts/boot-test.sh`, `BENCH_CRITICAL_PATHS` (feeds
`report_bench_absence`).

**What.** The list added earlier the same day to close
`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE` held five entries —
`kernel/src/{mm,sched,ipc,syscall,smp.rs}` — because it was derived from
CLAUDE.md's perf-critical *table*, read as directory names. The suite it is
supposed to guard measures far more than that. Against the 63 recorded
benchmark names:

- `isr_latency`, `page_fault` → **`kernel/src/idt.rs`**. CLAUDE.md's table
  names both "interrupt dispatch" and "page fault handling", but the handlers
  live in `idt.rs`, not under `mm/` — so the two benchmarks that measure them
  were unwatched.
- 8 × `vfs_*` (`read_256`, `write_256`, `readdir`, `stat_{root,3comp,deep}`,
  `throughput_16k_{read,write}`) → **`kernel/src/fs`**. CLAUDE.md lists "VFS
  path lookup" and "filesystem read/write" as critical.
- ~20 × `net_*`, `tcp_checksum_*`, `dns_build_query`, `firewall_check`,
  `http_*`, `dashboard_api_*` → **`kernel/src/net`** (`http.rs`,
  `dashboard.rs` live under it).
- 9 × `crypto_*` → **`kernel/src/crypto.rs`**.

So **30+ of 63 benchmarks measured code the watch list did not watch**, and a
change to any of them printed "No perf-critical changes since the last
benchmarked commit, so skipping the suite is reasonable here." Confidently,
and wrongly.

**How it surfaced.** The `W-KERNEL-COW-WRITE` diagnostic commit edits
`kernel/src/idt.rs`. The following boot reported no perf-critical changes —
while the suite contains `isr_latency` and `page_fault`, both measured by code
in that exact file. (No real regression: that diagnostic sits on the fatal
path, which is not hot. The harness had no way to know that, and did not
reason about it — it simply never looked.)

**Fix.** Widened the list to the four missing paths and annotated **every**
entry with the benchmarks it guards, so the mapping is auditable instead of
implicit. Verified: `git diff --name-only 17dbde179 HEAD` over the new list
now returns `kernel/src/idt.rs`, which the old list missed.

**Lesson (the recurring one this week).** This is the third instance in a row
of the same shape: `TD-BENCHMARKS-...` (the suite silently never ran),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff
confidently named innocent benchmarks), and now a watch list that confidently
reported "nothing to see" about a file it had never been told to look at. A
check that cannot fire is indistinguishable from a check that passes — and
every one of these was *my own* freshly-written tooling, reporting success.
When adding a guard, the first test should be "does it fire on a case I know
is positive?", not "does it run cleanly?".
