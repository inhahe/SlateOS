## TD-B-PRE-BOOT-QUICK-IS-NOT-QUICK

**Filed:** 2026-09-02 by Lane B, measured while verifying the §747 SKIP
rendering end to end (two runs killed by their own timeout, which is how this
was noticed at all).
**Status:** open. Not a correctness bug; it costs time and it misleads.

**In short:** `scripts/pre-boot.py --quick` is documented as skipping "the
slowest gate". It is not the slowest gate — it is the sixth. `--quick` removes
about **5%** of the run, and the run is about **forty minutes**. Anyone who
reaches for `--quick` expecting a pre-flight they can sit and wait for will
instead sit for forty minutes, or kill it and get nothing, which is what
happened twice here.

**Measured 2026-09-02**, `--quick --no-fmt --no-unix-check` on `os-lane-b`:
**2261s across 27 gates (37.7 min)**, and **two gates had not started** when a
2400s cap killed the tree. So the true figure is higher than 37.7 min; it has
never been observed to completion in this configuration.

| Gate | Time |
|---|---|
| `check-variant-lists.py` | **834s** |
| `check-doc-links.py` | **412s** |
| `check-user-access-sites.py` | 144s |
| `check-live-counter-reads.py` | 127s |
| `check-tick-wiring.py` | 116s |
| *clippy, what `--quick` skips* | *~113s* |
| `check-key-release-wiring.py` | 83s |
| (21 others) | ≤ 80s each |

Two of the 29 gates are 55% of the wall clock.

**Why it matters beyond the wait.** `pre-boot.py` exists to be run *before* the
boot test, i.e. often. A gate suite nobody can afford to run is a gate suite
that does not run, and the failure mode is silent: you skip the pre-flight,
push, and find out from the shared blocking `boot-test.sh` instead — which is
the one that serialises QEMU across all three worktrees. The cost lands on the
other two lanes.

**What the proper fix looks like**, in the order I would do it:

1. **Profile `check-variant-lists.py` and `check-doc-links.py` specifically.**
   834s and 412s for scripts that read source text is far off what the work
   requires; at a guess (untested) each is re-parsing the tree per query rather
   than once. Two scripts fixed would return more than half the run.
2. **Run the `check-*.py` gates concurrently.** They are independent, read-only,
   and separate processes already — a `ThreadPoolExecutor` over the glob would
   collapse the tail to roughly the longest single gate. This changes the output
   ordering, so results must be buffered per gate and printed in glob order, and
   it must stay sequential for anything that writes (`cargo fmt` already runs
   first and alone).
3. **Only then rename or re-scope `--quick`.** Once the phase is minutes rather
   than forty, the flag can mean what its name says.

Corrected the two places that stated the false claim (module docstring Usage
section, and the `--quick` argparse help) rather than leaving the measurement
only here — but the numbers above are the debt, not the wording.

### Update, 2026-09-04: the same gates cost more in `boot-test.sh`, where it is not optional

The measurement above was taken on `pre-boot.py`, which nobody is obliged to
run. The same gate suite runs inside `scripts/boot-test.sh` — the blocking one,
the one every merge waits on — and there it is longer, because `boot-test.sh`
also runs `check_python_suites()`, a gate `pre-boot.py` does not have.

Measured on `lane-b`, 2026-09-04, from `/tmp/boot.log` (`run-timeout.py`
heartbeats give the clock to ±60 s):

| Phase | Starts at | Length |
|---|---|---|
| checker gates (`check-*.py`, shellcheck, …) | 0s | **~2400s (~41 min)** |
| `check_python_suites` (32 `scripts/test-*.py`) | ~2400s | **≥ 480s, unfinished** |
| build | never reached | — |
| QEMU | never reached | — |

The run was killed at 2880 s (48 min) with the build not yet begun. In the
480 s the suite gate did get, it finished **12 of 32** suites, alphabetically
through `test-check-self-tests-wired.py`. The next one in order is
`test-checkers-honour-head.py`, which is **582 s standalone** — measured this
same day — so the suite gate alone is plainly a further 20–35 minutes, and the
whole pre-build phase is **on the order of 75 minutes** before a single crate
is compiled. Build (~6.5 min) and QEMU (~7 min) come after that.

Three things follow that the entry above did not say:

1. **A 3600 s timeout on `boot-test.sh` cannot pass on this tree**, regardless
   of whether the tree is good. That is not a hypothesis — it is what killed
   the run measured here. Any `run-timeout.py` budget for a full boot test
   must now be **≥ 10800 s**. A too-short budget does not merely waste 60
   minutes; it returns exit 124, which is indistinguishable at a glance from a
   real failure, and the two previous kills recorded at the top of this entry
   are the same mistake made twice already.
2. **`check_python_suites` is the right design and should not be trimmed.** It
   globs deliberately (`boot-test.sh:5159`, and see the retracted entry
   `TD-B-THE-THIRTY-TWO-SUITES-…` below for why the glob defeated a search for
   the names). The fix for its cost is to make it *concurrent*, exactly as fix
   2 above proposes for the checker gates — the 32 suites are independent
   processes over disjoint temp dirs, and the gate's wall clock would collapse
   to roughly its longest member (~582 s) rather than their sum. Do not
   respond to the cost by dropping suites from the glob.
3. **The two fixes now have a much larger payoff than when they were filed.**
   Concurrency over ~29 checker gates plus 32 suites turns ~75 minutes into
   roughly the longest single member — `check-variant-lists.py` at 834 s or
   `test-checkers-honour-head.py` at 582 s, so call it 15 minutes. That is the
   difference between a boot test a lane can run per task and one it runs once
   a day, and the serialisation lock means the cost is paid by all three lanes.
