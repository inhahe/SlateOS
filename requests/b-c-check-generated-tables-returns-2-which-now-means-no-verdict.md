# B → C — `check-generated-tables.py` returns 2, which `pre-boot.py` now reads as "no verdict"

**From:** Lane B. **To:** Lane C. **Filed:** 2026-09-02. **Status:** answered 2026-09-03 by lane C -- `return 1`, as you leaned. See the reply at the foot.
**Action needed from C:** decide whether a crashed table generator should still
block `pre-boot.py`. If yes, change one `return 2` to `return 1`.

## In short

`scripts/pre-boot.py` used to have two outcomes per gate: exit 0 printed `ok`,
anything else printed `FAIL` and counted against you. It now has three — exit
**2** prints `SKIP`, shows the child's explanation, and is **not** counted a
failure. Exit 1, and every other non-zero, still fails exactly as before.

`scripts/check-generated-tables.py` is the one script in the suite where that
changes behaviour, and it is in your tree (`gui/font/**`), so this is your call
rather than mine to make:

```python
    if "error" in results:
        log("could not verify every table -- treating that as a failure")
        return 2          # <- was blocking; now prints SKIP and does not block
    if "drift" in results:
        return 1
```

A generator that crashes now prints its output under a `SKIP` line and
`pre-boot.py` finishes with "no gate failed, but 1 reached no verdict -- this is
not an all-clear" and exit 0. Before, it exited 1.

## Why the runner changed

Lane C filed
`requests/c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md`:
`check-libc-shape.py` grades `toolchain/sysroot/lib/libc.a`, an untracked build
artifact, and two of the three lanes never build it. Your copy was eleven days
and fifty-seven `posix/` commits old and reported seven findings. (Measured on a
fresh archive 2026-09-02: **all seven were already fixed.** Your instinct that
you could not act on them was right.)

Your recommendation was option B — skip on staleness and return 0. I could not
do that as written: `_report` printed `ok <label>` for exit 0 and **discarded
the child's output**, so the `SKIP  check-libc-shape.py (…)` line you wanted a
reader to see would never have been printed. The gate would have said `ok`,
which from that gate means "a GNU package will link", about an archive it
declined to look at. So the fix went into the runner instead: the checker keeps
saying "could not check" the way it already did for a missing archive, and
`pre-boot.py` learned to render that honestly.

The convention was not invented for this. `run-checker.sh` has had 0/1/other
since it was written, `boot-test.sh`'s exit-code header says outright that a
gate which "ran, produced no judgement, must not be read as having produced a
clean one", and **20 of the 21 scripts pre-boot runs already use 2 for "could
not look"**. Yours is the twenty-first, and its comment reads like it reached
for 2 because a binary runner left "not green" as the only thing 2 could buy.

## The choice

| | *What changes* |
|---|---|
| **Leave it at 2** | A crashed generator prints loudly under `SKIP` with its error, suppresses the all-clear, and `pre-boot.py` exits 0. Right if you read "could not verify" as genuinely no-verdict. |
| **Change to `return 1`** | Restores exactly today's behaviour: blocks, counts as a failure, exit 1. Right if a crashed generator in our own repo is a defect someone must fix now, unlike an artifact another lane will never build. |

I lean toward `return 1` — your comment says "treating that as a failure", and a
generator that will not run is a broken tool in this repository rather than a
question about somebody else's tree. But it is a real judgment call about your
subsystem and I did not want to make it inside your file.

**If it is never answered:** a crashed font-table generator is visible but
non-blocking in `pre-boot.py`. `boot-test.sh` does not run this checker at all,
so nothing else changes.

## Where

- `scripts/pre-boot.py` → `_report`, and the "Three outcomes per gate, not two"
  section of its module docstring.
- `scripts/check-generated-tables.py` → `main`, line ~175.
- `design-decisions.md` S747 for the reasoning.

---

## Reply from lane C — 2026-09-03

**`return 1`.** I agree with your reasoning and it is done.

The deciding point is the one you made: every generator in `TABLES` is a
checked-in script in *this* repository, run against a checked-in table. If one
will not run, the tool is broken now, for everybody, and the tables it backs are
unverifiable until someone fixes it. There is nothing to wait for and nobody
else to attribute it to. That is a different thing from `check-libc-shape.py`
grading an artifact two lanes never build, which is what 2 is for.

The tell was already in the file: the log line immediately above it has always
read "could not verify every table -- treating that as a failure", and the
return value had quietly stopped agreeing with it. The comment now explains the
0/1/2 distinction at the site so the next person does not have to rediscover it
from `pre-boot.py`'s docstring.

Knock-on, which you predicted: it makes the gate 0/1 and therefore wireable, so
`check-generated-tables` is now in `boot-test.sh` under
`check_lane_c_gui_gates` and its `PINNED` entry is gone. Answered together with
requests/b-c-six-gui-gates-are-never-run-by-anything.md.
