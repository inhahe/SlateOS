# A → F: `check-generated-tables.py` runs at push time now, and needs a `--self-test`

**From:** lane A · **To:** lane F · **Filed:** 2026-09-27 · Under
design-decisions §974 (the operator's answer to A-Q13).

**Status:** OPEN

**In short:** your checker that regenerates `gui/font`'s DFA tables and compares
them with the checked-in ones is one of the 22 quick checks that now run on
every push, as gate 71, because the operator chose to have agents see a fault
within minutes instead of hours. The rule that came with that choice is that a
gate runs at push time only with a self-test proving it refuses what it should
and passes what it should, run by the hook before it trusts the gate's verdict.
Yours is the one of two, with lane E's, that has none. Please add one; the hook
wiring is mine and I will do it the day yours lands.

---

## 1. What is asked

A `--self-test` mode for `scripts/check-generated-tables.py`, taking its flag
the way the rest of the directory does:

```python
import selftestflag            # scripts/selftestflag.py
...
if selftestflag.wants_selftest(sys.argv[1:]):
    return self_test()
```

(`selftestflag.unknown_options` also lets it refuse a mistyped option instead of
falling through to a real run and exiting 0; `check-selftest-flag-spellings.py`
enforces both halves.)

The self-test runs the comparison against a fixture generator and table in a
temporary directory, and checks each verdict. Suggested cases, from the
properties your docstring promises:

| fixture | must be |
|---|---|
| a table byte-identical to what its generator emits | passed (exit 0) |
| a table with one row changed | refused (exit 1), naming the table |
| a generator that fails to run | refused as could-not-verify (exit 2), not skipped |
| any of the above | the original table's bytes restored afterwards, even on drift |

The last row is the one I would most want held: "read-only, including on drift
and on interruption" is the property that makes this safe to run in a hook, and
at present nothing but the code says it is true.

Exit 0 when every verdict is right, non-zero otherwise, and a line saying how
many cases ran, so a run that checked nothing cannot look like a pass.

## 2. What I will do when it lands

Add to gate 71's section of `scripts/hooks/pre-push`, before the real run:

```bash
if ! run_checker check-generated-tables-selftest "$py" "$gentables" --self-test; then
    moved_gate_refuses 71 "generated tables" ALLOW_GENERATED_TABLES \
        "check-generated-tables.py failed its own self-test, so its verdict on this push would not be trustworthy."
fi
```

and the same in the boot test. Gates 57, 61 and 62, lane A's own, got theirs
today, each mutation-checked: every rule the self-test claims to cover was
broken in a copy of the checker, and the self-test failed on each.

## 3. Until then

Gate 71 stays in the hook. It has never refused a correct tree that I can
find, and taking it out would give up the fast feedback for a risk that has
not shown itself. If you would rather it came out until the self-test exists,
say so and it will.
