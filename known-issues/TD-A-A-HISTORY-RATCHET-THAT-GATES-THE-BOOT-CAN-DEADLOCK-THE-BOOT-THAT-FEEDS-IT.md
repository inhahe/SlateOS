## TD-A-A-HISTORY-RATCHET-THAT-GATES-THE-BOOT-CAN-DEADLOCK-THE-BOOT-THAT-FEEDS-IT (lane A, 2026-09-11) — **two instances, both fixed; the second fix opens a hole named below**

**In short:** several checks here learn from a file that only a boot test writes, and
they also run *as* a gate on that boot test. When one of them sees something it dislikes,
the only thing that would clear it is a new entry in that file — which it has just
prevented anyone from producing. The build is then stuck in a way that has nothing to do
with the code.

### Instance 1 — `check-boot-skips.py`

A ratchet over `bench/boot-history.jsonl`'s recorded skips. A skip whose emitting source
line had been deleted stayed in the history forever, so the gate kept failing and the
only way to update the history was a boot it was blocking. Fixed by `_collapse()` +
`still_emitted()`: a history entry whose source line no longer exists is **stale**, not a
finding.

### Instance 2 — `test-bench-history.py`, same day

*"no undeclared benchmark vanished between consecutive records."* Caused by me:
`vfs_write_breakdown_ns` was recorded at `4f6ca7a1b`, I removed the phase before the run
at `96356d747` — because it sat exactly on the harness's empty-closure floor and tripped
`BELOW-FLOOR` every run — and the series then vanished between two consecutive records.
`BOOT=1`, at a pre-build gate, so nothing could merge.

Restoring the phase does not clear it. The guard compares `records[-2]` and `records[-1]`,
and those are fixed history; clearing it needs a *third* record, which needs the boot the
gate refuses.

Fixed the same way as instance 1: `declared_series()` reads `kernel/src/bench.rs` for
every `score("…")` and `track("…")`, and a removal whose name the **source still
declares** is a lag rather than a loss.

### Why that is not filing off a ratchet that caught me

The distinction worth keeping, because the action looks identical to the bad version:

* The guard's question is *"did this benchmark stop being measured?"* The records are a
  **proxy** for that; `bench.rs` is the authority. Consulting the source replaces a proxy
  with the thing itself.
* Its teeth are intact — a name absent from the source *and* from the latest record is
  still an undeclared removal and still fails.
* It is **positive** evidence, which is the test `unexplained_removals`' own docstring
  sets: a name is excused because the source says the series continues, not because
  nothing says it stopped.

### A correction to my own reasoning: there was no dilemma

I justified the restore as two mechanisms in conflict — BELOW-FLOOR against the
history guard — and chose the guard because lost records are irreversible. That
reasoning is sound and it was not needed, because the tree had already answered the
question and I had not looked.

**Two benchmarks already sit at exactly the floor** and have not been removed:
`shm_rw_64bytes` and `net_ethernet_parse`, both `min=18 cycles <= harness floor 18`,
the same 18 as mine. Three cheap operations all pinning to 18 is the harness saying
"below my resolution", not "something is wrong" — and the standing treatment of such
a series here is to keep it and let the line print.

So removing mine was not a judgement call between two guards; it was inconsistent
with two existing precedents in the same scorecard. The cheaper way to that answer was
one `grep -c BELOW-FLOOR` on a boot log, which I ran only after the tree had gone red.

### The hole this fix opens, uncovered

`declared_series()` trusts the presence of a `track("name")` call in the file. **It does
not check that the enclosing function is still called.** So a benchmark whose call was
removed from `run_all`, with the function left behind, would stop appearing in records,
keep its `track(` text, and have its disappearance excused — which is precisely the
"coverage lost silently" this guard exists to prevent.

Two things I checked, hoping one would close it, and neither does:

* **No reachability check exists.** Nothing in `scripts/` verifies that every declared
  benchmark is reachable from `run_all` — the analogue of `check-self-tests-wired.py` for
  benchmarks does not exist.
* **The compiler does not close it either.** An uncalled `fn bench_x()` is `dead_code`,
  which is a *warning*, and this build already emits ~18 000 of them, so one more is
  invisible. `dead_code` is `#[allow]`-ed at three sites in these files, so it is
  otherwise active — but active and fatal are different things here.

**CLOSED the same day, and with the real fix rather than the cheap one.** The paragraph
below is kept because the reasoning for deferring was sound and the deferral was short.

> `declared_series` now consults `reachable_bench_fns`, which walks the call graph from
> `run_all` and returns only the bodies it can reach; a `track("x")` inside a function
> nothing calls is no longer a declaration. Proven by mutation against the real
> `bench.rs`: cutting the single line `bench_vfs_write_breakdown();` out of `run_all`
> drops that function from the reachable set and its **12 series** out of the declared
> set, 106 to 94, so a removal of them stops being excused. Six cases added to
> `test-bench-history.py`, and the last of them asserts that the *old* text-only scan
> still sees the orphaned series — so the test documents the defect, not just the fix.
>
> **Which way it fails was chosen, not inherited.** A call made through a function
> pointer or generated by a macro is invisible to the walk, so a function that really
> runs can look unreachable. That makes the gate *stricter* — it declines to excuse —
> and it can only bite alongside a genuine gap in the records, which a running benchmark
> does not produce. The opposite error loses coverage in silence. And if `run_all`
> cannot be found at all, the function returns `None` and the caller falls back to the
> unrestricted scan, because a parse failure must not be what makes this gate strict:
> strictness here means a pre-build failure only a new boot record can clear, which is
> the deadlock the whole entry is about.

**The cheap partial fix that was deliberately skipped:** require the enclosing `fn`'s
name to
appear more than once in `bench.rs` (definition plus at least one call). Removing the
call from `run_all` drops it to one. That is a weak reachability proxy — it would miss a
function reached only through a pointer or a macro — but it is ~15 lines and closes the
ordinary case. Left undone deliberately rather than silently: it is a second change to a
shared guard in the same sitting as the first, and the first was prompted by my own
breakage, which is the worst moment to keep editing a guard. The sitting ended with a
green boot test, which is what made the proper fix the right next move instead.
