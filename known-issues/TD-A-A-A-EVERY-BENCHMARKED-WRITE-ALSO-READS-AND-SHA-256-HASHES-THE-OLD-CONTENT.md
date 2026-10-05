## TD-A-A-A-EVERY-BENCHMARKED-WRITE-ALSO-READS-AND-SHA-256-HASHES-THE-OLD-CONTENT (lane A, 2026-09-11) — **unmeasured, and it re-explains two over-budget benchmarks**

**In short:** SlateOS keeps an automatic 16-deep undo history for files. To do that, every
time a file is overwritten the kernel first reads back what was there and computes a
SHA-256 checksum of it. That work is real and it happens on every write the benchmarks
perform — but no benchmark measures it, so it has been sitting inside an "unaccounted"
figure while I looked for it in the wrong places. Two benchmarks that miss their
performance targets are both plausibly missing them for this reason.

### The chain, all of it verified

* `main.rs:9495` calls `fs::history::set_auto_version(true)` immediately after `BOOT_OK`.
* `main.rs:9527` calls `bench::run_all()`. **Auto-versioning is therefore on for every
  benchmark**, and the comment at `history.rs:143` explains why it is deliberately off
  *before* that point: hashing multi-megabyte files with `IF=0` during boot staging once
  starved the hard-lockup watchdog into a false positive (`B-PTHREAD-YIELDBUDGET`).
* `write_file_resolved` calls `history::try_auto_record(path)` on every write.
* `try_auto_record` proceeds when `is_auto_version_enabled()` (now true) and
  `should_auto_version(path)`. The latter returns **false** only for `/proc`, `/dev`,
  `/sys`, `/tmp`, and two internal metadata filenames. The write benchmarks use
  root-level paths, so it returns **true**.
* `record_version` then reads the file's current contents, hashes them, inserts into the
  content-addressed store, appends a version entry, and evicts past
  `max_versions_per_file: 16`.

So a 200-iteration write benchmark performs 200 read-backs, 200 SHA-256 computations and
200 CAS insertions, and attributes none of it.

> **Correction, same day: these runs are RELEASE builds, not debug.**
> `boot-test.sh`'s own usage text says it outright — *"Without it, `--bench` means
> release and everything else means debug"* — and `bench/boot-history.jsonl` confirms
> it: the two commits this analysis rests on, `4f6ca7a1b` and `96356d747`, are both
> recorded `profile=release`.
>
> I had argued the magnitude from debug-build slowness, quoting `history.rs`'s comment
> about hashing multi-megabyte files taking seconds. That comment is correct in its own
> context — boot-time staging, which genuinely is debug — and I carried it somewhere it
> does not apply. Optimised SHA-256 over 256 bytes is single-digit microseconds, not
> twenty-six.
>
> **What survives and what does not.** The mechanism is unchanged and still verified
> from the source: the write path reads the old contents back, hashes them, and inserts
> into the CAS, on every write outside `/proc`, `/dev`, `/sys`, `/tmp`, and
> `set_auto_version(true)` runs before `bench::run_all`. It is real, it is on the
> measured path, and nothing measures it. What does **not** survive is the claim that it
> explains most of the residual. It is probably a few microseconds of the twenty-six,
> which puts the weight back on the residual being an *upper bound* rather than a cost —
> the other entry filed today.
>
> This makes the A/B measurement more valuable, not less: it was designed to confirm a
> prediction and will now have to produce the number instead.
>
> **Second correction, same day: the cost is not the hash at all.** `record_version`'s
> body contains
> `let timestamp_ns = crate::hpet::elapsed_ns();` — the **same MMIO clock read** fixed
> in `journal::record` earlier today, where switching it to `clock_monotonic()` took that
> phase from 14,206 ns to 337 ns. `VersionEntry::timestamp_ns` is documented as "HPET
> timestamp (nanoseconds since boot)", the identical contract, which `clock_monotonic`
> satisfies.
>
> So roughly **13.9 µs of the ~26 µs residual is one line**, and it is the line I had
> already fixed once, one module over. I reached it by reading the function to predict
> what the A/B would contain — not by the A/B, which has not run yet.
>
> **The write path's HPET exposure is exactly this one site**, checked rather than
> assumed: `notify`, `audit`, `quota`, `intercept`, `cas`, `journal`, `vfs` and
> `page_cache` have zero calls; `index` has two and both are in `rebuild()` and
> `age_of_last_rebuild_ns()`, neither on the write path. That is a consistency check as
> well as a survey — if `add_entry` read the HPET, the `index` phase could not have
> measured 4,115 ns, because one read alone costs ~13.9 µs.
>
> **What the residual now looks like**, with the arithmetic stated so it can be checked:
>
> | component | ns | basis |
> |---|---|---|
> | `record_version`'s HPET read | ~13,900 | the journal's measured 14,206 → 337 delta |
> | `index::add_entry` | ~4,100 | measured phase, and HPET-free per the survey above |
> | memfs write, `cache_identity`, mount resolution, CAS, hash | remainder | unmeasured |
> | co-occurrence slack | unknown | `min(whole) − Σmin(parts)` is an upper bound |
>
> So about half the residual is now named and fixable, and the rest stays consistent with
> the upper-bound framing rather than demanding a further hidden cost. **The fix is held
> behind the measurement on purpose**: repairing it first would measure the repaired state
> and destroy the before/after that makes the improvement checkable.
>
> **The 13,900 figure was checked, and checking it found a lapse of mine.** It rests
> on the journal delta being attributable to one line, so I read the commit.
> `journal.rs` changed exactly one code line; the other 18 were comments. But the same
> commit also touched `bench.rs` by 22 lines, *removing the `ns` phase* — the removal
> that red-treed the tree hours later. It does not disturb the attribution: `ns_only`
> measured **4 ns**, so dropping it moved the reported remainder by 4 ns, and
> `journal_only` itself was untouched.
>
> The verification was only necessary because I had put two logical changes in one
> commit, against the one-logical-change rule. A clock fix sharing a commit with a
> benchmark-phase removal is exactly the shape that makes a later delta
> unattributable, and it is the same shape as the three baseline artefacts produced
> earlier today by computing over a window straddling an unrelated change. Here it
> cost one `git show`, because the phase was 4 ns. With a phase worth microseconds the
> attribution would not have been recoverable from the recorded numbers at all.

### What this re-explains

* **The ~26 µs "remainder"** in `bench_vfs_write_breakdown`. I had enumerated the
  residual's contents twice and missed `try_auto_record` both times, because it looks like
  a cheap feature-flag check at the call site and the expensive half is two functions
  down.
* **`vfs_throughput_16k_write`, 2.3× over budget.** I recorded this as CPU-bound kernel
  write work on the strength of a 3.77× accelerator ratio. The ratio says *real CPU work,
  not VM exits*, which is correct and which SHA-256 also satisfies — and 16 KB is 64× the
  data of the 256-byte case, so a per-byte hash cost would dominate there exactly as
  observed. Consistent with, not yet proven.
* **`vfs_write_256` over budget**, same mechanism at 1/64th the data.

### A clean A/B exists with no new hooks, and it is free

`should_auto_version` skips `/tmp`, and `/tmp` is mounted by `fs::memfs::mount("/tmp")`
(`main.rs:1497`) — **the same filesystem implementation as `/`**. So writing identical
bytes to `/bench_x.tmp` and `/tmp/bench_x.tmp` differs in essentially one thing: whether
the version record happens. The delta isolates the cost with no `pub(crate)` change, no
instrumentation, and no feature gate.

Confounders, each bounded and each small against a predicted ~20 µs effect, stated so they
are designed out rather than discovered later:

| Confounder | Size |
|---|---|
| Different mount, so `resolve_mount` scans to a different entry | a few `starts_with` calls |
| Separate memfs instance with far fewer children at its root, so `child_ino`'s map lookup is shallower | `O(log n)` on a small *n* either way |
| The indexer's watch/exclude lists may differ between `/` and `/tmp` | **none — eliminated, see below** |
| The dcache is global, so `invalidate_negative_prefix` is identical | zero |

### What is still unknown

Whether the **read-back** or the **hash** dominates. They are separable with the same
trick — a path that exists versus one that does not, since `record_version` returns early
for a missing file — but that changes the write from overwrite to create, which is a
different code path. Prefer splitting it by data size instead: the hash is per-byte and
the read-back's resolution cost is not.

### The policy question, not mine to settle

Whether every file write in the OS should cost a read-back plus a SHA-256 is a
user-visible performance tradeoff, and `design.txt` does not mention auto-versioning at
all — so it is not settled by the spec. Raised in `open-questions.md` rather than decided
here. Note that the benchmark-validity half is independent of the answer: whatever the
policy, a benchmark named `vfs_write_256` should say whether its number includes
versioning.
