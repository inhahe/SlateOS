### TD-BASELINES-TOML-IS-INVALID-TOML-AND-NOTHING-READS-IT — 2026-08-14 — ✅ FIXED 2026-08-14 (`bench/baselines.toml`, `scripts/test-bench-history.py`)

`bench/baselines.toml` — the file CLAUDE.md names as the place performance
baselines live, and which ~30 comments across `kernel/src/bench.rs` cite as
their source — **did not parse as TOML.** It carried two `[compositor_frame_4k]`
tables, at lines 296 and 389, which is a hard error in every conforming parser:

```
tomllib.TOMLDecodeError: Cannot declare ('compositor_frame_4k',) twice
                         (at line 389, column 21)
```

The two disagreed about the **unit**: `target_ns = 2000000` in one,
`target_ms = 2.0` in the other. Only one carried the measured figure and the
optimisation history (48.6 ms → 21.4 → 15.8 → 11.9 → 10.6 ms). So the file had
been carrying two contradictory records of the same benchmark, and a parser
that tolerated duplicates would have silently taken whichever came last.

**Why it survived: nothing reads the file.** Every reference to it in the tree
is a *comment*. `kernel/src/bench.rs` hard-codes each target as a literal with
`// Target from baselines.toml: < 200 ns` beside it; `scripts/bench-history.py`
never opens the file. So the file *looked* like the authority while the real
authority was ~60 scattered literals in Rust, and no parser was ever pointed at
the thing that was supposed to be the source of truth.

**This is the fifth instance of the same defect class**, after
`TD-BENCHMARKS-...` (the suite never ran), `B-BENCH-WATCHLIST-...` (the watch
list never looked), `B-BENCH-COMPARATOR-...` (the diff named innocents) and
`TD-BENCH-CANARY-...` (the canary never fired). The invariant keeps holding: *a
check that cannot fire is indistinguishable from a check that passes.* Here it
went one step further — the artefact could not even be **loaded**, and that too
was indistinguishable from health, because loading was never attempted.

> **Resolution.** The duplicate table is merged (the poorer one removed, with a
> comment at the site recording why). `scripts/test-bench-history.py` gained
> `test_baselines_is_valid_toml()`, which `tomllib.load`s the real file — so the
> file is now machine-read for the first time and a duplicate or syntax error
> fails the suite. The test also asserts every table names a target in some
> unit, matched by `target*` **prefix** rather than an enumerated list (the
> units are open-ended by design: `target_accesses_over_nop` and
> `target_accesses_delta` exist because TCG harness overhead swamps the
> absolute number, and an enumerated list would silently under-report the day
> it wasn't extended). Calibration constants and host metadata opt out via a
> declarative `not_a_target = true` in the data rather than a name list in the
> test. Writing that assertion immediately found four more tables to classify.
> 16 checks pass.

**Not fixed — the duplication itself.** Targets still live in two places: this
file and the literals in `bench.rs`, with nothing keeping them in sync, so they
can drift silently and at least one (`vfs_stat_root`: 700 ns in the file) should
be re-derived anyway. The proper fix is for the kernel's scorecard to be checked
against the parsed file by `bench-history.py`, so the file becomes the authority
it already claims to be. Blocked on nothing but effort; tracked here.

#### FOLLOW-UP 2026-08-14: with the file finally parseable, the drift is measurable — and it is near-total

Making `baselines.toml` load was worth doing for its own sake, but the first
thing a working parser bought was a number for the damage. Matching the 63
benchmark names the kernel prints against the 57 baseline tables:

| | count |
|---|---|
| benchmarks measured by the kernel | 63 |
| baseline tables in the file | 57 |
| **matched by name** | **30** |
| measured with no baseline at all | 33 |
| baselines naming a benchmark never measured | 27 |

**Less than half of what runs has a baseline it can be compared to.** And the
two lists are not describing different work — they are largely the *same*
benchmarks under two names, drifted apart because nothing ever had to reconcile
them:

| kernel prints | baselines.toml calls it |
|---|---|
| `syscall_dispatch` | `syscall_trivial` |
| `page_fault` | `page_fault_anon` |
| `tcp_checksum_v4` | `net_tcp_checksum_v4_1460b` |
| `tcp_checksum_v6` | `net_tcp_checksum_v6_1460b` |
| `vfs_stat_deep` | `vfs_stat_deep_2comp` |
| `vfs_throughput_16k_read`/`_write` | `vfs_throughput_16k` |
| `heap_alloc_free_64` | `heap_alloc_small` |
| `ipc_channel` | `ipc_channel_roundtrip` |
| `ipc_pipe` | `ipc_pipe_roundtrip` |
| `ipc_eventfd` | `eventfd_signal_read` |
| `ipc_semaphore` | `semaphore_signal_wait` |
| `firewall_check` | `net_firewall_inbound_check` |
| `dns_build_query` | `net_dns_build_a_query` |
| `io_ring_nop` | `iouring_sqe_submit` |
| `isr_latency` | `interrupt_dispatch` |
| `service_connect` | `service_connect_accept` |
| `cp_notify_wait_rt` | `cp_notify_wait_roundtrip` |
| `net_tcp_conn_lookup` | `net_tcp_conn_table_scan` |

That is 18 of the 33 unmatched accounted for as pure renames. The remainder
split into benchmarks genuinely lacking a baseline (`vfs_stat_root`,
`vfs_read_256`, `vfs_write_256`, `vfs_readdir`, `vfs_stat_3comp`,
`http_gzip_*`, `ipc_channel_sync`, `net_arp_lookup`, `net_checksum`,
`net_ethernet_parse`, `net_ipv4_parse`, `pick_next`, `sched_pick_next`) and
baselines for work that is not benchmarked at all (`futex_uncontended`,
`futex_contended_wake`, `futex_wait_mismatch`, `compositor_frame_4k` — the last
is Lane C's and is measured by a host-side `cargo test`, not by this suite).

**Note what this does to the headline number.** The `over_target` count the
kernel reports (15 of 63 on the release run) is computed from the literals in
`bench.rs`, not from this file — so it is not wrong, but it is also not
*checkable* against the stated baselines for the 33 unmatched. Ranking the
release run against the parsed file yields only 7 over-target entries, and that
smaller number is an artefact of the missing half, not good news. Notably
`vfs_stat_root` — the benchmark currently under investigation at 8.5x over — has
**no** table here at all; its 700 ns target exists only as a comment in
`bench.rs` citing a file that does not mention it.

**Proper fix, unchanged but now specified.** `bench-history.py` should parse
this file and check each recorded entry against it, reporting unmatched names
in both directions as a failure rather than silence. That requires first
reconciling the names — one canonical name per benchmark, used by both the
`run()` call in `bench.rs` and the table here. The rename table above is the
work list. Until then the parse test added today guarantees only that the file
is *loadable*, not that it is *true*.


#### FOLLOW-UP 2026-08-14 (2): the file is now *checked*, and 11 targets disagree

`bench-history.py` gained `load_baselines()` + `report_baselines()`, which
compare the target the kernel prints on each `SCORE` line — the literal in
`bench.rs` — against the target this file states. The very first run of that
check, against `build/serial-test.txt` (63 benchmarks):

```
Baselines: 11 disagree, 15 unbaselined, 7 unused
  context_switch:      kernel says   5000ns, file says  10000ns
  crypto_aead_1KiB:    kernel says 100000ns, file says  70000ns
  crypto_sha256_1KiB:  kernel says  50000ns, file says  40000ns
  dns_build_query:     kernel says  40000ns, file says   2000ns   (20x)
  firewall_check:      kernel says   2000ns, file says   1000ns
  heap_alloc_free_64:  kernel says    400ns, file says    200ns
  http_mime_type:      kernel says   2000ns, file says    500ns   (4x)
  io_ring_nop:         kernel says    200ns, file says    300ns
  ipc_channel:         kernel says   2000ns, file says   3000ns
  page_fault:          kernel says  10000ns, file says   8000ns
  syscall_dispatch:    kernel says    200ns, file says   1200ns   (6x)
```

**Every PASS/OVER verdict for those 11 has been graded against a number its own
documentation contradicts.** The direction matters case by case: `syscall_dispatch`
measured 653 ns is *OVER* against the kernel's 200 ns and would *PASS* against
the file's 1200 ns. Which is correct is not obvious — 200 ns is the CLAUDE.md
hardware figure (Linux getpid ~100 ns, "within 2x"), while 1200 ns looks like a
TCG-adjusted budget. That is exactly why the check **reports and does not
reconcile**: picking a side automatically is how the two drifted apart.

The check distinguishes three failure modes deliberately, because they are
different problems: *disagree* (one side edited without the other), *unbaselined*
(the Rust literal is the only record of the target — 15 benchmarks, including
`vfs_stat_root`), and *unused* (the file claims coverage that does not exist — 7).
It also refuses to conflate an unparseable file with an agreeing one, printing
`UNVERIFIED`; that distinction is the entire lesson of this entry and is pinned
by a test.

Table renames brought name-matching from 30/63 to 48/63 (the tables moved, not
the benchmarks — `history.jsonl` is append-only and its names cannot change
without orphaning every historical record). 23 checks pass, up from 13.

**Still open:** the 11 disagreements need adjudicating one at a time, and the 15
unbaselined benchmarks need tables with real provenance. Both are now *visible on
every bench run* rather than invisible, which is the change that matters.

#### FOLLOW-UP 2026-08-14 (3): the 11 disagreements were mostly ONE bug — two kinds of target merged into one number

Adjudicating the 11 turned up a structural cause rather than eleven clerical
errors. `bench.rs` says it plainly in its own comments:

```rust
// OpenSSL SHA-256 1KiB: ~1500ns.  QEMU target: 50000ns.
score("crypto_sha256_1KiB", &result, 50000);

// DNS query build includes a heap allocation (Vec::with_capacity) which
// is expensive under QEMU (~35us).  Target set to 40us to track regressions
// without false-failing on the allocation overhead.
score("dns_build_query", &result, 40000);
```

**Those are TCG budgets, not hardware references** — and `baselines.toml` was
storing the hardware reference under the same key. Comparing them reported a
20x "disagreement" where in truth the two files were each right about a
different quantity. Two more (`heap_alloc_free_64`, `http_mime_type`) were the
same shape one level down: a *scope* difference, where the benchmark measures a
fixed multiple of the per-operation target (alloc+free is 2x an alloc; the MIME
benchmark does 4 lookups).

Worse, `bench-history.py` printed this on every run:

> *(The 'target' column in the scorecard above is a **hardware** reference and
> cannot be met under TCG — see bench/baselines.toml.)*

which is **false for at least six benchmarks**, whose targets are explicit QEMU
budgets. The line explaining the number misdescribed it, and so did the
scorecard headline: "48/63 within hardware target" counts passes that were
scored against TCG budgets.

**Fix: make the two kinds separate keys.** `target_ns` stays the hardware
reference; `tcg_target_ns` is the budget the suite is graded against under
emulation, and the cross-check prefers it when present. The explanatory line now
says the column is a mix and points at which key records which.

**Three were real disagreements.** Two are settled by CLAUDE.md's performance
table, which outranks the file:

* `context_switch`: file said 10 µs, spec says *"Target: < 5 µs"* → file corrected.
* `page_fault`: file said 8 µs, spec says *"Target: < 10 µs"* → file corrected.
* `ipc_channel`: file said 3 µs, spec says *"Target: < 2 µs round-trip"* → file corrected.
* `syscall_dispatch`: file said 1200 ns, derived by doubling a **638 ns WSL2
  measurement of a full syscall including spectre mitigations** — not the same
  quantity as dispatch. Spec says *"Linux: ~100 ns for getpid. Target: within 2x"*
  → 200 ns. **This one changes a verdict:** the measured 653 ns is OVER at
  200 ns and would have PASSed at 1200 ns. The 638 ns figure is kept as context,
  not as a derivation.
* `io_ring_nop`: file said 300 ns (2x a 150 ns measurement), spec says
  *"~100-200 ns per SQE; same order"* → 200 ns.

Result: **11 disagreements → 1.**

**The last one is instructive and is deliberately still open.** `firewall_check`
carries the comment `// Target from baselines.toml: 2000ns` in `bench.rs` while
the file says 1000 ns — a citation that is simply false, and the direction
(2x looser) means the kernel silently relaxed its own target at some point.
Both pass comfortably (measured 55 ns), so nothing is hidden by it; it is left
for the next `bench.rs` change rather than fixed now, because a kernel edit
during an in-flight release build would produce a binary that does not
correspond to any commit. Recorded here so it is not lost.
