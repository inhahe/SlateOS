## TD-A-A-KILLED-BOOT-TEST-LEAVES-NO-TRACE-WHERE-ANYONE-LOOKS-FOR-ONE (lane A, 2026-09-03)

**In short:** if `run-timeout.py` kills a boot test for exceeding its budget,
the run writes **no row** to `bench/boot-history.jsonl` — and the wrapper that
launched it can still report a clean finish. So the two places a person
actually looks (the task list, and the last row of the history file) both look
exactly as they do when no boot test was ever started. Today one run was killed
at 2400s, ~24 gates into the pre-build sweep, and I read the stale previous row
as though it were the new verdict for several minutes before noticing the
commit hash had not moved.

**The rule this makes concrete, which `CLAUDE.md` already states:** read the
verdict from the log and from `bench/boot-history.jsonl`, never from an exit
code. What today adds is *how to tell a missing verdict from a passing one*,
since a missing verdict is not an error message:

| what you see | what it means |
|---|---|
| last row's `commit` == `git rev-parse --short HEAD` | this is your run's verdict |
| last row's `commit` is an older commit | **your run produced no verdict**; the row belongs to someone else's run |
| log's last line is `[run-timeout] TIMEOUT after Ns -- killing process tree` | killed; nothing was decided |

Checking the commit hash is the cheap discriminator and I was not doing it.
`ts` is not a substitute: another lane's run appends rows to the same file
through the shared worktree history, so a *newer* timestamp does not mean a
newer row of yours.

**Why the budget was exceeded, as far as it is measured.** The pre-build gate
sweep is 30 checkers, most of which walk all ~805 kernel source files, and it
is **not serialised across lanes**. The cross-worktree boot lock
(`scripts/boot-test.sh:6039`) is acquired *after* the build, deliberately —
its own header explains that waiting inside the lock would idle while holding
the one resource other lanes queue for. That is the right call for the lock,
but it leaves the gate sweep and the `cargo` build entirely unserialised, and
those are as CPU-bound as the QEMU phase the lock does cover.

Measured on this host, `Logoplex3`, all on `lane-a`:

| run | phase reached | elapsed |
|---|---|---|
| `e7d9573b9`, `5c3a57267` (history rows) | **whole run**, gates + build + QEMU | 530s, 537s |
| today's killed run, another lane's boot test live throughout | gate ~24 of 30, **sweep only** | 2400s (killed) |
| today's replacement run, same overlap | gate 12 of 30, **sweep only** | 840s |

The sweep alone taking longer than two entire previous runs is the observation.
Overlap with another lane is the obvious hypothesis and the timing fits, but
**it is not proven**: I have no instrumented solo measurement of the sweep by
itself to compare against, and the sweep also grew today (the two bash oracles
were rewritten to shell out to bash far more often). Both changed at once,
which is exactly the condition under which not to name a cause.

**Proper fix, in the order I would do it.**

1. **Make a killed run say so where the verdict lives.** The absence of a row
   is currently indistinguishable from not having run. `boot-test.sh` cannot
   write the row (it is dead), so the writer has to be the wrapper: have
   `run-timeout.py` — or a thin boot-test-specific wrapper around it — append a
   `{"verdict": "KILLED", "commit": …, "phase": <last `=== …` line seen>}` row
   on exit 124. The `phase` field is the part worth having; "killed at gate 24"
   and "killed in QEMU" are different bugs and the log is the only thing that
   distinguishes them today.
2. **Time the phases.** `boot-test.sh` prints `=== … ===` per gate but no
   clock, so nobody can say what the sweep costs solo. Stamping each banner
   with elapsed seconds costs nothing and would have answered the question
   above rather than leaving it a hypothesis.
3. **Only then** decide whether the sweep needs its own serialisation. It may
   well not — extending the boot lock over the build would idle two lanes for
   ~7 min each, which the lock's header already argues against — but the
   decision needs (2) first.

**If it is never fixed:** every boot test whose budget is a guess can end with
no verdict and no visible complaint, and the stale last row of
`boot-history.jsonl` will be read as the answer. That is the same shape as the
entry above it: a thing that did not look, reported identically to a thing that
looked and found nothing.

### Fix 2 has a name now: no field records how long the script ran — 2026-09-04

Item 2 above says "time the phases" and treats it as instrumentation nobody has
got round to. It is worse than that: it is why item 2 keeps costing runs. I
repeated the exact failure this entry describes today, one day later, having
written the entry.

**What I did.** Sizing a `run-timeout` budget for a boot test at `a1ee59162`, I
took the last `bench/boot-history.jsonl` row (`5c3a57267`), read
`wall_seconds: 537`, allowed ~6.7× headroom, and launched with `3600`. At
`+53 min` the run was still in the gate sweep and would have been killed at
`+60`. I killed it by PID and relaunched at `10800`. The immediately preceding
full run, recorded in `build/boot-test-gates.log`, had taken **6051 s** —
**11.3× the `wall_seconds` I sized against.**

**Why the number was 11× wrong, and why that is not a reading error.** The row
has 25 fields. Exactly two are durations, and each is *correctly* scoped to a
phase:

| field | covers | set at |
|---|---|---|
| `build_seconds` | `cargo build` only — deliberately excludes the prerequisite and free-space checks | `boot-test.sh:5789–5791` |
| `wall_seconds` | the QEMU window only — `QEMU_START_EPOCH`→`QEMU_END_EPOCH`, stamped at the first `kill_qemu` so log processing is not counted as guest time | `:6530`, `:1442`, `:2606` |

Neither is mislabelled; both are documented and both are right. The gap is that
the **third and now largest phase has no field at all**. The gate sweep runs
from the first `run_checker` (`:3087`) to the last (`:5578`) and is stamped
nowhere, so `build_seconds + wall_seconds` is not the run — on today's numbers
it is under a tenth of it. There is no field whose absence you would notice: a
row that omits the dominant term looks exactly like a row that reports it, which
is the same shape as everything else in this file's neighbourhood.

**The only record of a total is an accident of redirection.** The
`[run-timeout] child exited: PASS, 6051s elapsed` line lives in whatever log the
agent happened to redirect that run to. `grep -rn boot-test-gates scripts/`
returns nothing — the harness never chose that filename; a session did.

Counted today: **119** files under `build/` contain both `=== Boot test PASSED
===` and a `child exited: …, Ns elapsed` line. That is 119 measured full-run
totals the project already owns and cannot query, carrying no commit, no verdict
linkage and no protection from the next session reusing the name. The longest
five:

| total | log |
|---|---|
| **6515 s** | `build/boot-test-merge.log` |
| 6138 s | `build/boot-test-batch3.log` |
| 6051 s | `build/boot-test-gates.log` |
| 4880 s | `build/boot-cgroup.log` |
| 4023 s | `build/boot-ap4.log` |

So the worst case on record is **12.1×** the `wall_seconds` a budget gets sized
from, and a `3600` budget was never survivable by five of the runs the tree has
already done. `build/boot-test-gates.log` is being overwritten by the
replacement run as I write this; the 6051 s survives only because I read it
first. The other 118 are one filename collision each from the same fate.

**The argument for fixing it is already written in the file that needs it.**
`boot-test.sh:5777–5784` explains why `build_seconds` was added: Q46's tradeoff
is "slower build, faster boot", "we have always measured the boot half
precisely … and the build half not at all, so one side of that comparison was
an assertion and the other was evidence." That reasoning now applies with more
force to the sweep, which is bigger than either half it was written about, is
the term that actually decides whether a run fits its budget, and is *growing* —
`check_eol`, added yesterday, put ~32 s on it by itself.

**Proper fix.** Two more fields on the boot-history row:

- **`script_seconds`** — stamped at the very top of `boot-test.sh`, before the
  prerequisite checks. Note this is deliberately the opposite convention to
  `BUILD_START_EPOCH`, which excludes them on purpose: the point of this field
  is to be the number a budget is sized from, so it must include everything a
  budget must cover.
- **`gates_seconds`** — around the sweep. `script_seconds` alone tells you a run
  got long; only the split tells you *which* phase grew, which is the question
  every subsequent "why is this slow" investigation opens with, and the one this
  entry's own timing table had to leave as an unproven hypothesis.

Per-banner elapsed stamps (item 2 as originally written) are still worth having
for reading a live log, but they are not a substitute: they are not in the
history file, so they cannot be compared across runs, and they die with the log.

**Now measured within a single run, which the argument above could not do.** The
replacement run's own log gives the split directly, because `run-timeout.py`
prints an elapsed heartbeat every 120 s and `boot-test.sh` prints a banner at
each phase change. Reading the two against each other:

| boundary | evidence in the log | elapsed |
|---|---|---|
| gate sweep begins | `Prerequisites OK (limine,services,rootfs).` | ~120 s |
| gate sweep ends | `=== Building kernel ===` falls between the `3960s` and `4080s` heartbeats | ~3960 s |
| **sweep alone** | | **~3840 s (~64 min)** |

Against a `wall_seconds` of ~500 s for the QEMU window on a comparable run, the
sweep is roughly **7.7× the only phase the history file records**, measured on
one run rather than inferred across two. Two individual gates account for a
large share of it and both announce their own cost, so the harness already knows
these numbers and simply discards them: `Clippy OK (debug profile, **202s**, …)`
and `cfg(unix) OK (**553s**, …)`.

That last detail sharpens the fix. `gates_seconds` as a single total is worth
having, but the sweep is 30-odd checkers and a bare total will not say which one
grew. Since `run_checker` is the common call path for every gate, it is the
natural place to stamp start/end per gate and accumulate a per-gate map — the
same shape as the existing `gated_ran` field, which already proves a
dictionary-valued column is workable in this row format.

**If it is never fixed:** every future budget gets sized from `wall_seconds`,
because it is the only duration in the only file anyone treats as the record.
That is not a mistake a careful reader avoids — I made it the day after writing
this entry, with the entry open. The failure mode is self-perpetuating: a
too-small budget kills the run, a killed run writes no row, and no row means the
next person sizes from `wall_seconds` again.
