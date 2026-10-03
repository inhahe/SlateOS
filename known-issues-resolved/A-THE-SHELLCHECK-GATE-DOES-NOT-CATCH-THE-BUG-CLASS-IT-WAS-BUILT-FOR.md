## `A-THE-SHELLCHECK-GATE-DOES-NOT-CATCH-THE-BUG-CLASS-IT-WAS-BUILT-FOR` (lane A, 2026-08-29) — **FIXED 2026-08-29**, verified still closed 2026-08-31

**Status: closed.** The gate reads `bash "$PROJECT_ROOT/scripts/shellcheck-all.sh"
warning` at `scripts/boot-test.sh:3666`, and the tree clears it: 85 script(s), 0
with findings at severity `warning`, 0 findings total (re-measured 2026-08-31).
Both halves of "How to close this" below are done — lane B cleared all 44
findings and quoted the template at `diff-wsl.sh:99` so the count does not
regrow, and lane A made the one-word change. See
`requests/a-b-shellcheck-floor-the-remaining-findings-are-all-yours.md` and
`requests/b-a-shellcheck-is-clean-at-warning-raise-the-floor.md`.

**Why this heading said "open" for two days.** The remedy landed the same day it
was filed, in a different file, by a different lane; nothing then came back to
this entry. That is the ordinary failure mode of a tracking file — an entry
whose *fix* is recorded elsewhere stays open by default, and an open entry that
is actually closed is worse than no entry, because it costs a reader the time to
rediscover the fix before they can trust the file again. The remainder of this
entry is kept verbatim for the reasoning, which is still the argument for why
the floor belongs at `warning` rather than `error`.

### Original entry (2026-08-29), unedited

**What it is.** `check_shellcheck` in `scripts/boot-test.sh` refuses to build
when `scripts/shellcheck-all.sh` reports a finding — but it asks for findings
at severity **`error`**, and `error` is a bar almost nothing clears. It catches
shell that will not parse. It does **not** catch the unquoted-expansion class,
which is severity `warning`, and the unquoted-expansion class is the one that
has actually cost this project money: see
`A-A-STRAY-D-VISUAL-TURNED-EVERY-WORD-SPLIT-OF-OUR-OWN-PATH-INTO-A-SILENT-SUCCESS`
above, where `D:\visual studio projects` split on its space and every consumer
of the fragment silently succeeded on the wrong path.

So the gate as it stands would not have caught the bug that motivated having a
gate. It is not useless — a parse error caught before a 900-second QEMU window
is worth the ~40 s it costs — but its headline claim is broader than its reach,
and that gap should be written down rather than discovered later by someone
trusting it.

**Why it is not simply raised.** The tree is not clean at `warning`. As of
`bc1965e3f`:

```
78 script(s), 38 with findings at severity warning, 44 finding(s) total
```

All 44 are in lane B's differential harnesses (`scripts/*-diff.sh` and
`gen-chmod-fixture.sh`, which ship with `userspace/**`). Lane A may not edit
them — `scripts/which-lane.py` gives lane A exactly `boot-test.sh`,
`run-timeout.py` and `wedge-soak.sh` under `scripts/` — and a 37-file
mechanical sweep is the worst possible shape of cross-lane change, touching
everything and conflicting with whatever lane B has in flight.

**What is already done.** Lane A's own six findings are cleared, each with a
per-line `# shellcheck disable=` and a written reason (`ce784e028`). None was a
real defect; each had to be read to establish that. Those six were the half of
the blocker that the original reversal condition in `design-decisions.md` §630
failed to mention, which is why the condition read as satisfied while the
reversal would still have broken the build.

**The remedy, and who holds it.**
`requests/a-b-shellcheck-floor-the-remaining-findings-are-all-yours.md` gives
lane B every site as `file:line`, the command to regenerate the list, and the
seven findings that need reading rather than quoting. 37 of the 44 are one
character each: `DIFF_PROG=awk` → `DIFF_PROG='awk'`. `dd-diff.sh:107` is
already quoted and is the one harness of the set with no finding, so the fix is
proven, not proposed.

**It gets worse with time, slowly.** The count is not fixed. It was 43 across
76 scripts when the request was drafted and 44 across 78 an hour later, because
`xargs-diff.sh` merged from `main` carrying the same unquoted idiom as its 36
predecessors. Every new harness adds one. The highest-value single action is
therefore not the sweep but fixing the template lane B copies from.

**How to close this.** When `bash scripts/shellcheck-all.sh warning` exits 0,
change one word at `scripts/boot-test.sh` in `check_shellcheck`:

```sh
out="$(bash "$PROJECT_ROOT/scripts/shellcheck-all.sh" warning 2>&1)" && rc=0 || rc=$?
```

That file is lane A's, so lane B should not make the change — it should say
the tree is clean and lane A will.

**One trap for whoever measures this.** `shellcheck-all.sh` runs `shellcheck
-x` from *inside* `scripts/`, and `-x` is load-bearing: it resolves the
harnesses' `# shellcheck source=diff-wsl.sh` and suppresses about three
findings per harness. Run by hand from the repo root the relative `source=`
cannot resolve, `-x` appears to do nothing, and the count comes out much
higher. Lane A hit exactly that and briefly recorded "`-x` does not help" as a
finding. Trust the number `shellcheck-all.sh` prints, because that is the
invocation the gate itself uses.
