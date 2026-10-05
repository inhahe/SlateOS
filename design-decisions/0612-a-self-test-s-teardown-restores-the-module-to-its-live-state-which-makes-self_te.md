## 612. A self-test's teardown restores the module to its live state — which makes `self_test()` the boot-time initialiser for 146 `/proc` tables, deliberately

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** ~146 little kernel modules each keep a table of numbers for
`/proc` to print, and each has a `self_test()` that runs during boot to check
the module works. Those self-tests used to finish by wiping the table back to
"not set up yet" — which sounds tidy, but "not set up yet" is not the same as
"empty": it makes the module *refuse* every later write, so `/proc` prints
zeros for the rest of the boot. The fix is to have the teardown wipe the
fixtures and then re-open the table. The consequence, which is the part worth
writing down, is that the self-test is now the thing that leaves these tables
open — so a diagnostic has become load-bearing.

**The options.**

| | *What changes:* |
|---|---|
| **(a) Leave the table closed** (status quo) | `/proc/cpustat` reads zeros on a busy machine and an idle one alike. This is the defect. |
| **(b) Stop wiping at all** | `/proc/cpustat` shows the self-test's invented CPUs as though a real driver had reported them. |
| **(c) Wipe, then re-open** (chosen) | `/proc/cpustat` reads a genuine empty table at boot, and real numbers once something feeds it. |
| **(d) Wipe, and open it again from `main.rs`** | Same observable result as (c), via 146 extra lines in `main.rs`. |

**Why (c) over (d), which is the only close call.** (d) is the more honest
layering: it would make boot-time openness an explicit property of the boot
sequence instead of a side effect of running a diagnostic, and it would survive
someone deleting the self-test calls. It was rejected on the ground that the
coupling it avoids *already exists and cannot be removed*: the first rung of
most of these self-tests asserts "empty after init", which requires the
self-test to call `init_defaults()` itself. So (d) does not decouple anything —
it adds a second caller alongside the one that has to be there regardless, and
then has to be ordered *after* the self-test or the wipe undoes it. Two callers
with an ordering constraint between them is worse than one caller, not better.

**The liability this accepts, stated plainly so it is not a surprise later.** A
future reader may reasonably think "diagnostics should not run during boot" and
remove the `fs::*::self_test()` calls from `main.rs`. That is a defensible
change on its face, and it would silently switch 146 `/proc` tables back off —
reproducing exactly the defect this fixed, with no error and no failing test,
because a table that refuses writes and a table with no writers print the same
zeros. Mitigations: `scripts/check-selftest-reinit.py` pins the teardown shape,
and `scripts/check-self-tests-wired.py` already pins the fact that the
self-tests are called. Neither states the dependency *between* them, which is
what this entry is for.

**Why the wipe cannot simply be dropped** — option (b) — is that the fixtures
are fabricated. `cpustat`'s test registers CPUs 0 and 1 and records time
against them; publishing that to `/proc` would be indistinguishable from a real
scheduler having reported it. Section 608's rule about printed fields that
nothing writes applies here in its stronger form: inventing the *reading* is
worse than admitting there is none.

**Where it bites.** `kernel/src/fs/*.rs`, the last two lines of every
`self_test`; `kernel/src/main.rs`, the block of `fs::*::self_test()` calls;
`scripts/check-selftest-reinit.py`. See known-issues.md
`A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT` for the measurement and
the burn-down.
