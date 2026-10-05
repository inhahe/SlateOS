## Three lanes building at once exhausts the Windows commit limit (all lanes) — OPEN

**In short:** this machine's commit limit is ~262 GB (RAM plus pagefile). With
all three lanes compiling, it sits at 96-97% — and at that point *any* process
that tries to start fails, including the ones a boot test needs. This is not
hypothetical: it has now cost two boot runs, on 2026-09-01 and 2026-09-02, one
of which also corrupted the boot-history evidence base (see the entry above).

**What it looks like when it bites:** `dofork: child -1 ... exit code
0xC000012D, errno 11` followed by `fork: retry: Resource temporarily
unavailable`. `0xC000012D` is `STATUS_COMMIT_LIMIT`. Free *physical* memory can
look healthy — 20 GB free while commit is at 97% is the normal shape — so
watching free RAM does not predict it.

**Why it is not simply fixed.** The obvious response, killing the other lanes'
builds, is forbidden: they are another agent's in-flight work, and the standing
rule is to kill only processes you started, by PID. Raising the pagefile is a
system-wide change that needs the operator. And a boot test is the single most
expensive thing to lose to this — 8-50 minutes, discarded at the very end.

**Mitigation in use now:** check commit charge before starting a boot
(`Get-CimInstance Win32_OperatingSystem`, `TotalVirtualMemorySize -
FreeVirtualMemory`), and do code work rather than boot work while it is above
~90%. With the fix above, a run lost this way is at least recorded honestly
instead of being blamed on the kernel.

**What a proper fix looks like:** the boot lock already serialises QEMU across
lanes; the same mechanism could serialise *builds*, or `boot-test.sh` could
refuse to start — with a distinct exit status, not a boot verdict — when commit
charge is above a threshold, so the cost is paid in seconds rather than at the
end of a 50-minute run. The second is lane A's to do and is the smaller change.

### Addendum 2026-09-02: it also costs wall clock in a way that has nothing to do with the commit limit, and that is the failure mode you will hit first

Concurrency does not have to reach `STATUS_COMMIT_LIMIT` to lose a run. On
2026-09-02, with lane B running a boot test in parallel, a lane-A boot test was
killed by its own 1800s budget having **never reached QEMU** — the pre-build
static gates alone consumed the entire budget. No fork failed, commit charge
never tripped the floor, and every gate that ran passed. The run was simply
too slow to finish.

**The measurement, because the first diagnosis was wrong.**
`check-variant-lists.py` sat for ~400s and looked like a hang; the hypothesis
was that its file walk had wandered into a `target/` tree. It had not — the
walk enumerates 6441 `.rs` files in 6.4s and the `target` filter discards
exactly zero of them, because the build directories are at the workspace root
and not under any scanned prefix. Timing `Path.read_text()` over the same
6441 files under live contention gave:

| | value |
|---|---|
| total | **368.5s** |
| mean per file | 57 ms |
| slowest single file | 0.86s (`userspace/psalm-cli/src/main.rs`) |

The distribution is the finding. There is no outlier and no pathological
input: every read is uniformly ~2 orders of magnitude slower than it should
be. That is a saturated disk, not a bug in the gate — the gate is innocent and
should not be "optimised" in response to this.

**What to take from it.** Two things, and the second is the one that bites.

1. **Do not size a boot-test timeout from an uncontended run.** The harness's
   own header cites 1004s of gates before it "says a word"; under a second
   lane that becomes the whole of a 30-minute budget. Budget 7200s when
   another lane is building, as lane B already does.
2. **A slow gate is not a hung gate, and the difference is not visible from
   the log.** Both look identical — a heartbeat line every N seconds against a
   header that never advances. The only thing that separated them here was
   measuring, and the cheap measurement (`faulthandler.dump_traceback_later`,
   which named `read_text` and cleared the regexes) is worth reaching for
   before forming a theory about the gate's *logic*. The first theory was
   wrong in a way that would have produced a real change to a correct file.

#### Addendum 2026-09-02 (lane A), second measurement: it is not "another lane *building*" — three lanes running their own gate suites is enough, and 3000s is also not a large enough budget

A lane-A boot test was killed at **3000s having never reached QEMU**, the same
shape as the run above. The interesting part is what the machine looked like
while it happened, because it rules out the explanation the addendum above
gives:

| | value |
|---|---|
| commit charge | **65.3%** (167.2 GB of 255.9 GB) — nowhere near the 96-97% ceiling |
| `cargo` / `rustc` processes, any lane | **0** |
| `qemu-system-x86_64` processes | **0** |
| other lanes active | yes — lane C running `check-doc-links.py`, lane B with live sessions |

So there was no compile anywhere on the machine, and memory pressure was not a
factor. What *was* running was three lanes' worth of pre-build gates at once —
and those gates are themselves whole-tree file walks. Lane A's own suite at
that moment was a `grep -rh --include=*.rs include_bytes!` across the
workspace, with `bootstrap-worktree.sh --check` beside it.

**The refinement: the contended resource is the file walk, not the compiler.**
Every lane's pre-build phase reads all ~6441 `.rs` files, repeatedly, once per
gate. Three lanes doing that concurrently saturates the disk just as
effectively as a `cargo build` does — which means the trigger condition for the
slow-gate regime is *any other lane being awake*, not "another lane is
building". That is a much easier condition to meet, and it is invisible from
the two things one would naturally check (free RAM and commit charge), both of
which looked healthy here.

Per-gate costs measured from this run, by pairing each `=== Checking` header
against the nearest `run-timeout` heartbeat:

| gate | duration |
|---|---|
| `scan-orphan-modules.py` | ~420s |
| `check-variant-lists.py` | ~300s |
| `scripts/test-*.py` suite | ~730s |
| VFS permission gate → usage-message gate | ~180s |
| the other 26 gates | 0-120s each |
| **total before the first line of kernel is compiled** | **>3000s** |

**Consequences, both of which sharpen advice already given above.**

1. **7200s is a floor, not a target.** Point 1 above says "budget 7200s when
   another lane is building". Read it as "when another lane is *running*", and
   note that 3000s has now failed for this reason twice on the same day
   (1800s once, 3000s once). The relaunch here used 9000s.
2. **Do not "fix" this by caching the gates on a source digest.** That was the
   first idea this measurement suggested, and the addendum above already
   forecloses it: the gates are innocent, and their inputs *had* changed on
   both of these runs anyway, so a digest cache would have skipped nothing. The
   cost is contention, and the honest levers are budget and scheduling.

One thing that *is* cheap and was not obvious: the clippy gate inside the same
run reported `Clippy OK (debug profile, 38s, ...)`. The build cache was warm
throughout. The 50 minutes were entirely pre-build static analysis — so a run
lost this way has not lost any compilation work, only wall clock.

### Addendum 2026-09-06: 7200s is no longer a floor, it is a *failing* budget — the gate phase alone measured 7402s

The advice above ("budget 7200s when another lane is building") has now been
overtaken by the gate phase's own growth. A lane-A boot test on 2026-09-06,
with lane B running `cargo test -p posix` alongside it, printed:

```
=== Gates OK (7402s) ===
=== Slowest gates (of 219 timed, 3717s of the 7402s phase) ===
    3685s of the phase was NOT in a run_checker gate
    (the tooling test-suite sweep, shellcheck and clippy are not routed
     through run_checker, so they are in the remainder, not the table).
```

**7402s of gates, before a single line of kernel was compiled.** A 7200s
budget cannot reach QEMU under these conditions — not "might not", cannot. An
earlier run that same night was killed at exactly that point: it was 92 minutes
in, still inside `test-checkers-honour-head.py`, with the tooling sweep,
shellcheck, clippy, the build and the whole QEMU window still ahead of it.

Three things worth carrying forward:

1. **Budget 21600s (6h) for a full boot test, not 7200s.** That is what the
   successful run used. It is not a generous number; it is roughly 2× the
   measured gate phase, which is the smallest multiple that leaves room for the
   build and the boot.
   **→ Superseded the same day by "Correction" below: 21600 was still derived
   from one observation, and the history file had eight. Use 28800.**
2. **The split has moved.** The 2026-09-02 measurement above put >3000s in
   `run_checker` gates. Today the *timed* gates are 3717s and the untimed
   remainder — the `scripts/test-*.py` sweep, shellcheck, clippy — is 3685s,
   i.e. very nearly half the phase is in work the slowest-gates table does not
   show. Reading that table and concluding "the gates cost 3717s" understates
   the phase by a factor of two. The harness says so itself, in the note quoted
   above; the note is easy to skim past.
3. **This is growth, not contention.** The 2026-09-02 entry correctly diagnosed
   a saturated disk. That is not the story here: the competing load was one
   `cargo test`, the free-space and toolchain-temp checks both passed with
   large margins, and the phase is simply doing more than it was — 219 timed
   gates plus a tooling sweep that now has its own several-hundred-case
   suites. Sizing the budget from a 2026-09-02 measurement is what produced the
   failed run.

**Not proposing a fix here, deliberately.** The obvious lever — let the boot
test skip gates — is one `boot-test.sh` already refuses on purpose
("people are tempted to skip, which is worse than no gate", line ~3592), and
that judgment is right. The honest response is a bigger budget and the
knowledge that a full boot test is now a ~2.5h operation, which is what this
addendum records.

#### Correction, same day: 28800s, and I made the same mistake I was documenting

Point 1 above says 21600 "because that is what the successful run used". That
is one observation — exactly the error the entry it corrects was written about.
`bench/boot-history.jsonl` has carried `script_seconds`, `gates_seconds` and
`build_seconds` on every row since 2026-09-04 precisely so nobody has to do
this, and `boot-test.sh`'s own header says to query it. I did not, until the
run above failed a second time. Querying it (`python scripts/boot-history.py
--list`, plus the raw rows for the phase columns) gives eight rows:

| when | script | gates | build | qemu | verdict |
|---|---|---|---|---|---|
| 2026-09-04T13:44 | 5278 | 4229 | 553 | 115 | PANIC |
| 2026-09-04T16:11 | 6710 | 5067 | 790 | 449 | PANIC |
| 2026-09-04T18:41 | 8118 | 5427 | 941 | 909 | PANIC |
| 2026-09-04T23:23 | 4373 | 3560 | 44 | 454 | PASS |
| 2026-09-05T02:32 | 5881 | 5027 | 36 | 490 | PASS |
| 2026-09-05T12:56 | 10712 | 8732 | 1032 | 466 | PASS |
| 2026-09-05T16:46 | **12760** | **10435** | 1299 | 513 | PASS |
| 2026-09-05T23:30 | 11608 | 10000 | 107 | 806 | PASS |

Median `script_seconds` **7414**, max **12760**. So 7200 was not merely tight,
it was *below the median run* and would have killed four of the eight; and the
7402 in this addendum's title is unremarkable rather than exceptional — it is
the middle of the distribution. Two further things the table shows that a
single observation could not:

- **QEMU is now the small half by a wide margin.** 115–909s of runs lasting
  4373–12760s. Any budget reasoned from boot time is off by an order of
  magnitude, which is the 2026-08-31 failure mode still lying in wait.
- **`build_seconds` tops out at 1299**, and is often ~40s on a warm cache. This
  is what sizes the *commit-headroom* wait, below — the thing being waited for
  is a sibling lane's build, so a 900s wait was shorter than the event it
  existed to wait out.

`scripts/boot-test.sh` now says 28800 (max 12760 + the adaptive commit wait +
rounding), with the derivation and the query in the header so the next person
does not repeat this.

#### The related bug this run actually died of: exit 5 after 8754s

The re-run with the larger budget did not time out. It failed at
`check_commit_headroom`:

```
ERROR: gave up after 900s waiting for commit headroom (6742 MiB free,
floor 12288 MiB, before building).
NOTHING WAS BUILT AND NOTHING WAS BOOTED — this says nothing about the code
under test.
```

8754s elapsed, **7402s of which was a gate phase that had passed**. The gate
threw all of it away rather than wait past fifteen minutes for a sibling
lane's `cargo` to finish. The script's own error text — "Refusing now costs
seconds instead" — was written for a check that runs before any investment,
and is simply false at the point the check actually runs.

Fixed in `943381d7b`: the wait budget is now the run's own elapsed time,
floored at 3600 (above the 1299s worst-case build in the table), recomputed
per call, uncapped, with an explicit `BOOT_TEST_COMMIT_WAIT` still honoured
verbatim. The rule is *never abandon an investment over a wait shorter than
the investment*. `scripts/test-boot-test.py` asserts the property rather than
the constant, and was checked against three mutants (flat 900, capped growth,
ignored env knob) to confirm it fails on each.

**Lesson, and it is the same one twice.** Both halves of this entry are a
number that outlived its evidence: 7200 was derived in August from ~3000s of
pre-QEMU, 900 was derived from a build that no longer takes 900s. Neither was
wrong when written. The defence is not a better constant — it is deriving the
number from `bench/boot-history.jsonl` at the moment of use, which is what the
adaptive wait does structurally and what the header now tells a reader to do
manually.
