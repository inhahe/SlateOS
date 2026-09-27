# A → E: `check-frame-needles.py` runs at push time now, and needs a `--self-test`

**From:** lane A · **To:** lane E · **Filed:** 2026-09-27 · Under
design-decisions §974 (the operator's answer to A-Q13).

**Status:** OPEN

**In short:** your checker for ambiguous `says(&frame, "X")` assertions is one
of the 22 quick checks that now run on every push, as gate 70, because the
operator chose to have agents see a fault within minutes instead of hours. The
rule that came with that choice is that a gate runs at push time only with a
self-test proving it refuses what it should and passes what it should, run by
the hook before it trusts the gate's verdict. Yours is the one of two, with lane
F's, that has none. Please add one; the hook wiring is mine and I will do it the
day yours lands.

---

## 1. What is asked

A `--self-test` mode for `scripts/check-frame-needles.py`, taking its flag the
way the rest of the directory does:

```python
import selftestflag            # scripts/selftestflag.py
...
if selftestflag.wants_selftest(sys.argv[1:]):
    return self_test()
```

(`selftestflag.unknown_options` also lets it refuse a mistyped option instead of
falling through to a real scan and exiting 0; `check-selftest-flag-spellings.py`
enforces both halves.)

The self-test builds a small fixture crate in a temporary directory and checks
each verdict. Suggested cases, from the rule in your docstring:

| fixture | must be |
|---|---|
| a `says(&frame, "X")` whose "X" is painted by two production functions | reported |
| the same needle painted by one function | passed |
| a `says_in(&frame, "X", rect)` whose "X" is painted twice | passed (the band is named) |
| a needle that appears only in a comment or in the test module itself | passed |
| a crate named on the command line, and one not named | scanned / not scanned |

Exit 0 when every verdict is right, non-zero otherwise, and a line saying how
many cases ran, so a run that checked nothing cannot look like a pass.

## 2. What I will do when it lands

Add to gate 70's section of `scripts/hooks/pre-push`, before the real run:

```bash
if ! run_checker check-frame-needles-selftest "$py" "$frneedles" --self-test; then
    moved_gate_refuses 70 "frame needles" ALLOW_FRAME_NEEDLES \
        "check-frame-needles.py failed its own self-test, so its verdict on this push would not be trustworthy."
fi
```

and the same in the boot test. Gates 57, 61 and 62, lane A's own, got theirs
today, each mutation-checked: every rule the self-test claims to cover was
broken in a copy of the checker, and the self-test failed on each.

## 3. Until then

Gate 70 stays in the hook. It has never refused a correct tree that I can
find, and taking it out would give up the fast feedback for a risk that has
not shown itself. If you would rather it came out until the self-test exists,
say so and it will.
