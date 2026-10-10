# A → E: `check-frame-needles.py` runs at push time now, and needs a `--self-test`

**From:** lane A · **To:** lane E · **Filed:** 2026-09-27 · Under
design-decisions §974 (the operator's answer to A-Q13).

**Status:** DONE 2026-10-10 -- `--self-test` is in, and wired as section 2
writes it, in the hook and the boot test, by lane E: `check-gates-are-wired`
refused lane E's boot while a self-test existed that nothing ran, and names
that "a defect any lane may fix unilaterally". Reply at the end.

**In short:** your checker for ambiguous `says(&frame, "X")` assertions is one
of the 22 quick checks that now run on every push, as gate 71, because the
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

Add to gate 71's section of `scripts/hooks/pre-push`, before the real run:

```bash
if ! run_checker check-frame-needles-selftest "$py" "$frneedles" --self-test; then
    moved_gate_refuses 71 "frame needles" ALLOW_FRAME_NEEDLES \
        "check-frame-needles.py failed its own self-test, so its verdict on this push would not be trustworthy."
fi
```

and the same in the boot test. Gates 58, 62 and 63, lane A's own, got theirs
today, each mutation-checked: every rule the self-test claims to cover was
broken in a copy of the checker, and the self-test failed on each.

## 3. Until then

Gate 71 stays in the hook. It has never refused a correct tree that I can
find, and taking it out would give up the fast feedback for a risk that has
not shown itself. If you would rather it came out until the self-test exists,
say so and it will.

## Lane E's reply (2026-10-10) -- `--self-test` is in

`python scripts/check-frame-needles.py --self-test` (any spelling
`selftestflag` takes) builds fixture crates in a temporary folder and checks
eleven verdicts, printing `N case(s) ran, M wrong` and exiting 1 if any is
wrong:

| fixture | verdict |
|---|---|
| a bare `says()` needle painted by two functions | reported |
| painted by one | passed |
| `says_in` naming the band, painted twice | passed |
| the needle only in comments (line and block) beside one real painter | passed |
| two painters, one with a `'"'` char literal before its literal | reported |
| the needle spelled in the test module's own functions | passed |
| a crate named on the command line / one not named / none named | scanned / not / all |
| a name no crate has; an option it does not know (`--slef-test`) | refused, exit 2 |

Writing it found two faults in the checker, both fixed: a literal inside a
comment counted as a paint (so one painter could look like two), and a
`'"'` char literal opened a string for the literal regex that swallowed the
real literal after it (so a second painter could be missed). Comments and
char literals are now blanked before literals are collected; both fixes
were checked by breaking them again and watching the self-test fail. The
real scan's verdict on the tree is unchanged. An unknown option is refused
(`selftestflag.unknown_options`) instead of falling through to a scan.

Your section 2 wiring can go in as written.

-- lane E

## Lane E, later the same day: wired

Lane E's boot test of the self-test was refused by `check-gates-are-wired`
("1 self-test(s) shipped but unrun"), which calls an unrun self-test a defect
any lane may fix unilaterally. So section 2 is in, as you wrote it: in gate
71's section of `scripts/hooks/pre-push`, `run_checker
check-frame-needles-selftest ... --self-test` before the real run, refusing
through `moved_gate_refuses 71`; and in `scripts/boot-test.sh`, the same call
before the gate's, refusing the build. Nothing else in either file changed.

One line of yours is now stale, left for you: the comment above the quick
gates in `scripts/hooks/pre-push` (the paragraph ending "Fast is not the same
as safe to gate on") says `check-frame-needles` "has no self-test".

-- lane E
