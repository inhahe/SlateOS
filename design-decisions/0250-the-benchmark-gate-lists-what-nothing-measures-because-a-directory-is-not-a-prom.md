## §250 — The benchmark gate lists what nothing measures, because a directory is not a promise

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** When you change kernel code, the boot test looks at which files
you touched and tells you whether you ought to run the benchmark suite. It
decided that by directory: touch anything under `kernel/src/fs`, and it said
"CLAUDE.md requires benchmarking these — run `--bench`". But `kernel/src/fs`
holds 467 source files and the suite has a stopwatch on three of them. So for
almost every file the gate named, running the twenty-minute suite it asked for
measured nothing about the change and came back green — an assurance nobody had
earned. The gate now says, per file, whether a benchmark exists: measured files
in one list, unmeasured files in another headed "running `--bench` will not tell
you whether these got slower". The tradeoff is that the second list is often
long and slightly pessimistic.

**Decision.** Replace the directory-level coverage claim with a per-file map
(`BENCH_COVERAGE` in `scripts/boot-test.sh`), where the default for any file not
explicitly listed is *no benchmark is known to cover this*. The gate's report is
split into a covered list and an uncovered list, and it only recommends
`--bench` when something in the change is actually covered.

**The numbers that forced it.** Counted 2026-08-20 against the 86 benchmarks in
the newest recorded run:

| Watched directory | Tracked `.rs` files | Files the suite reaches |
|---|---|---|
| `kernel/src/fs` | 467 | 3 |
| `kernel/src/net` | 49 | 10 |
| `kernel/src/mm` | 47 | 5 |
| `kernel/src/ipc` | 22 | 10 |
| `kernel/src/sched` | 18 | 5 |
| `kernel/src/syscall` | 9 | 3 |

**Four benchmarks measure a copy of the code rather than the code.** This is the
part that makes the old annotations worse than merely coarse, because they cited
these by name as the benchmarks a directory "actually guards":

| Benchmark | What it actually times |
|---|---|
| `net_checksum` | `internet_checksum()` — defined in `bench.rs:6395` |
| `tcp_checksum_v4` | `tcp_checksum_bench()` — `bench.rs:6451` |
| `tcp_checksum_v6` | `tcp_checksum_v6_bench()` — `bench.rs:6520` |
| `dns_build_query` | `build_dns_query_bench()` — `bench.rs:6664` |

Each is a private reimplementation living in `bench.rs`; one even says so in its
own doc comment ("duplicated to avoid depending on tcp module internals"). Make
the real label encoder in `net/dns.rs` ten times slower and all four stay green,
because none of them ever calls it. Two more were cited for files they never
enter: `isr_latency` opens its measurement window at `apic.rs:1059`, *inside*
`handle_timer_irq` and well past the `idt.rs` stub, and `page_fault` hand-rolls
map/unmap through `page_table` with the CPU exception explicitly excluded
(`bench.rs:5450`) — so neither measures `kernel/src/idt.rs`, and neither
measures `mm/fault.rs`, the actual `#PF` handler.

**Why the default is "uncovered" and not "inherit from the parent directory".**
This is the real tradeoff, and it is a genuine one. Inheritance is quieter: a
file exercised incidentally by a benchmark aimed at its neighbour gets credit
without anyone writing a rule. The per-file default is noisier, and it will
sometimes call a file uncovered that a benchmark does in fact walk through.

It wins anyway because the two errors are not the same size. A file wrongly
listed as uncovered costs one line of output and, at worst, a benchmark someone
did not strictly need to write. A file wrongly implied covered costs a
regression that ships, and costs it *silently* — the whole documented failure
mode of this mechanism, stated in `BENCH_CRITICAL_PATHS`' own comment, is that
its false negatives make no sound. When one direction fails loudly and the other
fails quietly, the default belongs on the loud side.

**Why the map is per-file rather than a smarter rule.** Every entry was read out
of `bench.rs`'s call sites — which module each benchmark actually invokes — not
inferred from a name or a directory. That is what makes it checkable, and it is
what a directory rule can never be: there is no property of "being under
`kernel/src/fs`" that implies a stopwatch exists.

**Why a rot check is part of the fix, not an extra.** A map that decides whether
a file counts as covered is exactly as dangerous as the directory rule it
replaced once it goes stale — a rule left behind by a deleted, renamed, or
runtime-`SKIP`ped benchmark answers "yes, covered" for a file nothing measures.
`report_bench_coverage_rot()` therefore checks every name the map cites against
the newest row of `bench/history.jsonl` on every green boot. Checking the
*recorded run* rather than grepping `bench.rs` is deliberate: several benchmarks
print `SKIP` and record nothing, and a source grep cannot see that.

**Alternatives considered.**

- *Leave the directories and add the missing benchmarks instead.* This is not an
  alternative so much as the other half, and it is still open (a ZFS read
  benchmark is logged in `known-issues.md`). But it cannot be a substitute:
  covering 467 `fs` files is not a task that finishes, and until it does the
  gate would keep certifying the uncovered remainder.
- *Derive coverage automatically by instrumenting a `--bench` run* (which files
  executed). Genuinely better in principle — it cannot go stale and it catches
  incidental coverage. Rejected for now because "a benchmark's code path touched
  this file" is not the same claim as "this benchmark would notice this file
  getting slower", and the automated version would reinstate exactly the
  over-claim being removed, with more authority behind it.
- *Drop the gate.* It has found real omissions; the problem was never that it
  asked, it was what it implied by asking.

**How it was verified.** The gate's functions were extracted into a standalone
harness and run against deliberately dirtied trees: a modified covered file
(`mm/frame.rs`) is reported under "measured by the suite" with its three
benchmark names; a modified uncovered file (`mm/user.rs`), a modified file in an
entirely unbenchmarked subtree (`fs/zfs/mod.rs`) and an untracked new file
(`ipc/scratch_new.rs`) are all reported under "covered by NO benchmark"; and
with no covered file in the change, the `--bench` recommendation is correctly
replaced by "the missing piece is a benchmark, not a run". The rot check was
given a negative control — a bogus `crypto_sha256_DELETED` spliced into the map
— and reported it; against the real map it is silent, which means all 86 cited
names were confirmed present in the last recorded run.

**The first draft of the map contained six of the same over-claims it exists to
remove**, which is the strongest available argument for both the rot check and
the pessimistic default. Auditing it entry by entry against `bench.rs` found
that `mm/frame_owner.rs` is A/B-tested only through `timed()`/`ab_interleaved()`
and recorded nowhere (an unrecorded measurement cannot detect a regression, so
it is not coverage); `syscall/number.rs` contributes a compile-time constant and
therefore no instruction inside the window; `net/interface.rs` is named by four
benchmarks but only to construct an `Ipv4Addr` *outside* the timed closure;
`sched/task.rs` supplies types the switch reads while the timed work happens
elsewhere; `fs/path.rs` was credited to `vfs_stat_breakdown_ns`, which times an
atomic load on the namespace fast path, rather than to `_prologue`/`_resolve`,
which actually run `validate_path`/`normalize_path` over its types; and
`sched/priority_rr.rs` was *under*-claimed, missing `context_switch` even though
`PerCpuScheduler::pick_next_local` lives there and sits on the yield path.

Five of those six were over-claims, made in the course of writing a fix *for*
over-claiming, by someone who had just spent an hour cataloguing the harm it
does. That is not carelessness so much as the nature of the mistake: "this
benchmark mentions this file" is a much easier question than "this benchmark
would notice this file getting slower", and the first is constantly mistaken for
the second. A map maintained by hand will keep drifting toward the easy
question, which is why the rot check is mandatory rather than nice-to-have, and
why the default has to fail loudly.

**The demonstration that prompted it.** Earlier the same day the gate named
`kernel/src/mm/user.rs`, the `--bench` cycle it asked for was duly run, and all
86 benchmarks completed without once calling `copy_to_user` or `copy_from_user`.
The run was not wrong about anything; it simply had nothing to say, and the old
report gave no way to know that in advance.

**How to reverse.** Delete `BENCH_COVERAGE`, `bench_coverage_for()` and
`report_bench_coverage_rot()`, and restore the single undifferentiated list in
`report_bench_absence()`. `BENCH_CRITICAL_PATHS` is unchanged in meaning and
keeps working on its own.

**What would change this.** If the suite ever grows to where most watched files
are genuinely measured, the uncovered list stops being informative and becomes
noise, and the balance above flips. Likewise if execution-trace-derived coverage
becomes available and can distinguish "executed" from "would detect a
regression", the hand-written map should give way to it.
