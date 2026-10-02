## `A-CGMEM-THE-LIMIT-IS-COMPARED-AND-THE-RESULT-IS-NEVER-SHOWN` (lane A, 2026-08-26) — **fixed 2026-08-26**

**In short:** `cgmem create` takes a memory ceiling, and every charge checks
whether the cgroup has gone over it. The result of that check is written to a
counter that no command prints. So the shell asks you for a limit, watches it
being exceeded, and has no way to tell you that it was.

**Where.** `kernel/src/fs/cgmem.rs`. `record_charge` (~line 167) does:

```rust
if c.usage_pages > c.limit_pages {
    c.high_events += 1;
}
```

`high_events` is declared at line 49 with the comment `// Times usage exceeded
high watermark`, incremented there, asserted once inside `cgmem::self_test`
(test 6), and read nowhere else. It is absent from `stats()`, which returns
`(cgroup_count, total_charges, total_uncharges, total_oom_kills, ops)`, and
absent from `cmd_cgmem`'s `list`, which prints `usage`, `rss`, `cache`, `swap`
and `oom`. There is no accessor for it.

**The same section has a second dead field.** `swap_pages` is initialised to 0
in `create` and is never assigned again by any function in the module —
`record_charge` and `record_uncharge` touch `rss_pages` and `cache_pages` only.
`cmd_cgmem list` prints it as `swap={}`, so every cgroup the shell has ever
displayed reports `swap=0`, not because no pages were swapped but because
nothing can ever make it say otherwise. A statistic that is structurally
constant is worse than an absent one: it reads as evidence.

**Why these two are one entry.** They are the same failure at the two ends of
the pipe. `high_events` is measured and not reported; `swap_pages` is reported
and not measured. Between them, `cgmem list`'s eight columns include one that
cannot be wrong and one that cannot be right, and the limit the user was asked
for at `create` time influences neither.

**Relation to `A-DISKQUOTA-FILE-COUNT-LIMITS-ARE-STORED-AND-NEVER-COMPARED`.**
That entry is the stricter version of this one — there the limit was stored and
never compared at all. Here the comparison happens; only its output is
unreachable. The two share a cause worth naming: a limit operand is easy to
accept and store, and the work of *acting* on it is a separate change that the
shape of the code does not force anyone to make. (That entry was fixed
2026-08-27; the shared cause it names is unaffected.)

**Proper fix.** Add `high_events` to the `list` line and to `stats()`, so the
comparison that already happens is visible. `swap_pages` needs the opposite
decision made first — either give the module a `record_swap` (which is what the
field's presence implies was intended) or delete the field and its column,
since a column that is always zero misinforms. Do not leave it printed and
unwritten.

**Not a regression.** True since the module was written.

**Fixed.** Both ends, in one change.

*The reported-and-not-measured end.* `record_swap(cg_id, pages, swap_in)` now
exists: `swap_in` false adds to `swap_pages` (pages left memory for swap),
`swap_in` true subtracts, saturating at zero so a swap-in larger than the
recorded swap-out clamps rather than wrapping to `u64::MAX`. It is reachable
from the shell as `cgmem swap <cg_id> <pages> [out|in]`, with `out` as the
bracketed default — that being the only direction that can be first, since pages
cannot come back in before they went out. It deliberately does *not* move the
pages out of `rss_pages`/`cache_pages`, because only the caller knows which
bucket they came from and inferring one would be §600's guessed-value shape; the
caller pairs it with `record_uncharge`.

*The measured-and-not-reported end.* `high_events` is now printed by
`cgmem list` as `high={}` between `swap=` and `oom=`, and a new
`total_high_events` aggregate is carried in `State`, bumped alongside the
per-cgroup counter in `record_charge`, and returned by `stats()` — which widened
from a 5-tuple to `(cgroups, charges, uncharges, ooms, high, ops)`. Both
`cmd_cgmem stats` and `/proc/cgmem` print it as `High events:`.

*Why this ordering matters beyond tidiness.* Surfacing `high_events` is what
makes the twenty-fifth burn-down batch's `cmd_cgmem create` limit-guess
falsifiable. Before this change a guessed policy ceiling had no observable
consequence anywhere in the system, so the burn-down entry looked harmless — it
was not harmless, it was *queued*, waiting for this fix to arm it. Both were
therefore done together.

*Tests.* `cgmem::self_test` grew from 9 to 10 tests. Test 1 now destructures the
6-tuple and asserts `high == 0` at init; test 8 asserts `high == 1` after test
6's deliberate over-limit charge; new test 10 (`swap_cg`) asserts `swap_pages ==
25` after out-40/in-15, asserts the clamp to 0 after an over-large swap-in, and
asserts `record_swap` on a nonexistent cgroup is an `Err`. Shell rung 93 asserts
the `high=0` column is present in `cgmem list` output.

*Design decision.* The delete-the-field alternative and why completing it won
are recorded as `design-decisions.md` §608.
