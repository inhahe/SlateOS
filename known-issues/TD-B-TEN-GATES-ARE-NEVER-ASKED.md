## TD-B-TEN-GATES-ARE-NEVER-ASKED

**Filed:** 2026-09-02 by Lane B, while wiring `check-gates-can-refuse.py`.
**Status:** OPEN — measured, partly another lane's to act on.

**In short:** the repo has 31 automated checkers under `scripts/check-*.py`.
Nine of them are not run by the boot test, which is the gate that actually
blocks a merge, and eight of those nine are not run by the pre-push hook
either. Those eight execute only inside `scripts/pre-boot.py`, a local
pre-flight script that nobody is required to run and that takes about forty
minutes. So a rule they enforce can be broken, merged, and pushed without
anything objecting.

This is the same defect as `B-CHECK-DOC-LINKS-BARE-RUN-PRINTED-HELP-AND-PASSED`
one level up. That gate ran and could not refuse; these gates are never asked.
Both present identically from the outside — a rule that appears guarded and
is not — and neither is visible in a green log, because the evidence is an
*absence*.

### The measurement

`scripts/boot-test.sh` names every checker it runs in an explicit `run_checker`
call; it does **not** glob. Set difference against `scripts/check-*.py`, taken
2026-09-02 (and see the methodology caveat below — the first three attempts at
this number were all wrong):

| Gate | in pre-push? | scans | whose |
|---|---|---|---|
| `check-doc-links.py` | **yes** (`doclinks`, hook line 1692) | docs tree-wide | shared |
| `check-diskcleanup-test-roots.py` | no | `apps/diskcleanup` | C |
| `check-evdev-elf-asm.py` | no | ring-3 evdev test payload | C |
| `check-frame-needles.py` | no | windowed app test suites | C |
| `check-generated-tables.py` | no | `gui/font/**` generated tables | C |
| `check-key-release-wiring.py` | no | windowed programs | C |
| `check-window-wiring.py` | no | GUI programs' `main` | C |
| `check-selftest-reinit.py` | no | `kernel/src/**` | A |
| `check-libc-shape.py` | no | `libc.a` object granularity | B |

Only `check-doc-links.py` is covered elsewhere, by the hook.
`check-gates-can-refuse.py` was a tenth row when this entry was drafted; it
was wired in `b5246478b` and is why the count moved from ten to nine.

### Measuring this is harder than it looks, and three obvious methods are wrong

Worth writing down, because the check this entry asks for has to get it right,
and each of these produced a confidently wrong number first:

1. **"Is the basename mentioned in `boot-test.sh`?"** — over-counts. The file
   discusses gates in prose: a worked example named `check-thing.py` that has
   never existed, and post-mortems naming gates it does not run.
2. **"Mentioned in non-comment text?"** — still over-counts, and the example is
   this session's own commit. `b5246478b` added the refusal line
   `echo "as scripts/check-doc-links.py now does." >&2`. That is executable
   shell naming a gate it does not run, so stripping comments does not remove
   it. A wiring check written the obvious way would have been broken by the
   commit that motivated it.
3. **"A literal `scripts/X.py` on a `run_checker` line?"** — under-counts, in
   two ways. `boot-test.sh` wraps long calls with `\` continuations, so a
   line-at-a-time scan misses them (it missed the gate wired minutes earlier).
   And `scripts/hooks/pre-push` never writes the path on the call at all: it
   binds `doclinks="${repo_root:-.}/scripts/check-doc-links.py"` 200 lines
   earlier and calls `run_checker … "$doclinks"`. Judged literally, the hook
   runs zero checkers.

What works: join `\` continuations, drop comment lines, resolve simple
`var=…/scripts/X.py` assignments, and take the script argument of `run_checker`
calls. The load-bearing requirement is the **conservative direction** — a
`run_checker` call whose script argument cannot be resolved must be *reported*,
not skipped. A silently-dropped invocation makes a wired gate look unwired,
which is a false alarm; but the same weakness in the other direction is what
produced every wrong number above.

### One of the nine must NOT be wired, and that is not an oversight

**RESOLVED 2026-09-03 — it is now wired, and the reasoning below is kept
because its *general* claim is still true and its *specific* conclusion is
not.** The generalisation ("a gate that can legitimately answer 'I could not
look' cannot be wired as things stand") was correct and is what motivated
`--may-skip`. What it did not anticipate is that the skip channel alone would
not have been enough here: wiring this gate with `--may-skip` and nothing else
would have made it decline on nearly every run, since the sysroot is rebuilt by
hand and `posix/` changes daily — a wired gate that never answers, which is the
*other* failure this entry warns about and is still OPEN as a general problem.

What made it wirable was pairing the skip channel with `--ignore-age`, which
changes the question rather than the answer: instead of "is the archive both
current *and* well-shaped?", the boot test now asks "is the archive on disk
well-shaped?" — weaker, but soundly answerable on every host. Nothing is lost,
because staleness was already reported: `boot-test.sh`'s fixture-freshness
block prints a loud WARNING naming the newer `posix/` sources, and deliberately
warns rather than fails because repairing it means rebuilding lane B's tree.
`--may-skip` still earns its place for the two declines that remain — no
archive at all, and an index too small to grade.

Wired as `check_libc_shape` in `boot-test.sh`; `PINNED` entry deleted in the
same commit. The gate got a `--self-test` first (24 cases, run unconditionally
because it builds its own `ar` archives in memory and so still checks the
checker on a host with no sysroot) and a discovery floor of 100 members / 500
symbols, an order of magnitude below the 615/3251 a real build produces.

Two things worth keeping from doing it:

- **A floor that nothing consults is decoration.** The floor's constants were
  pinned by the self-test from the start; that nothing proved `main()` ever
  *read* them was invisible until mutation testing deleted the floor block and
  the suite stayed green. The same shape of hole as the shellquote gate's
  verified-then-unused escape alphabet, found the same way. The fix was to
  drive `main()` end to end, in both directions — it must refuse a small
  archive *and* accept a large one, or "refuses everything" would pass too.
- **A two-part condition needs its parts driven apart.** With the floor's
  halves only ever tested together (fixtures below both floors, or above both),
  a floor that checked only members, or only symbols, passed every case.
  Killing those two mutants took fixtures that breach exactly one half each.

The original reasoning follows.

`check-libc-shape.py` grades a **build artifact** (`libc.a`), and since
`533e34e00` it returns **2 — "could not look"** when the archive predates its
own sources. `run_checker` (`scripts/run-checker.sh:105-128`) treats any exit
that is neither 0 nor 1 as *no verdict reached* and **aborts the whole build**.
Wiring this gate into `boot-test.sh` would therefore stop the boot test dead on
every worktree without a freshly built sysroot — which is nearly all of them,
nearly all the time, since the archive is not rebuilt per commit.

That interaction is worth stating plainly because it generalises:

> **A gate that can legitimately answer "I could not look" cannot be wired into
> `boot-test.sh` as it stands.** The exit-2 convention and the `run_checker`
> abort are individually right and jointly exclusive.

`check-libc-shape.py` stays where it is (invoked with an explicit path by
`toolchain/build-sysroot.ps1:148`, at the one moment the archive is current)
until that is resolved. Resolving it means giving `run_checker` a way to say
"this gate is allowed to skip" — an opt-in per call site, not a global
loosening, since the abort-on-no-verdict behaviour is load-bearing everywhere
else (see the header comment at `boot-test.sh:1168-1183`).

### What to do

1. ~~**Wire `check-gates-can-refuse.py`**~~ — **done, `b5246478b`.** Lane B's,
   no artifact dependency, sub-second, exits only 0/1 in any real checkout.
2. **Lane C: wire the six GUI/apps gates,** or record why each should not be.
   Filed as a request; not lane B's to change, because a gate that fails on
   lane C's tree blocks all three lanes.
3. **Lane A: wire `check-selftest-reinit.py`** (`kernel/src/**`), same caveat.
4. ~~**Give `run_checker` an opt-in skip channel,** then wire
   `check-libc-shape.py`.~~ — **done.** The channel landed 2026-09-03
   (design-decisions.md §753) and the gate was wired the same day, with
   `--may-skip --ignore-age`; see the RESOLVED note above for why the age flag
   was needed as well as the skip channel, and its `PINNED` entry is gone.

   **The channel is DONE as of 2026-09-03:** `run_checker --may-skip <label> …`
   treats exit 2 as a loud skip that returns 0 and sets `RUN_CHECKER_SKIPPED`,
   while an *unflagged* exit 2 keeps aborting (lane A's stated constraint — a
   floor must still stop the run). See design-decisions.md §753, and
   `scripts/test-pre-push-run-checker.py` group 9. ~~**Wiring the gates is the
   half still outstanding**~~ — done for lane B's five (the four bash oracles,
   `e891b2216`, and `check-libc-shape.py`); items 2 and 3 above, which are
   lane C's and lane A's, are what remain.

   The caution that came with the channel stands and is why `--ignore-age` was
   needed here: a gate wired with `--may-skip` whose tool is missing everywhere
   is back to being an unrun gate *without the ratchet reporting it as one*,
   because it now counts as wired. That visibility regression is named in §753
   and is **not solved** — it is only avoided, one call site at a time, by
   arranging that each skippable gate can in fact answer on this host. Nothing
   yet notices a gate that skips on every run.

### Why this was not caught by the meta-gate that found it

`check-gates-can-refuse.py` answers "can this gate return non-zero?" It cannot
answer "does anything run this gate?" — a different question about a different
file (`boot-test.sh`).

**Now checked, as of `809cac670`:** `scripts/check-gates-are-wired.py`, wired
into `boot-test.sh` beside its sibling. It is a **ratchet, not a gate** — the
eight unwired checkers are pinned in `PINNED` with a reason each, and it fails
only when the set changes: a new unwired gate, a pinned entry that is now
wired, or a pinned entry whose file is gone. Six of the eight are lane C's, so
failing outright would have blocked three lanes on work none of them scheduled.
Pruning is enforced in both directions, because an exemption list nobody prunes
stops describing the tree it exempts.

### The mutant that survived the first version of that ratchet

Worth recording, because it is the same bug one more level down. Deleting the
real call

    run_checker check-tick-wiring "$py" ".../check-tick-wiring.py"

from `boot-test.sh` changed the ratchet's verdict not at all. The line above it
runs the *same script* with `--self-test`, and the first version counted any
`run_checker` naming the script. So **a gate whose own cases still run, but
whose actual check has been deleted, read as fully wired** — "appears enforced,
is not", inside the checker written to catch exactly that.

Self-test invocations are now excluded (both `--selftest` and `--self-test`
spellings are live in this repo, so both are matched), the mutant is caught,
and three self-test cases pin it. Re-measuring after the tightening changed no
counts, which was the hoped-for answer: no gate here is wired *only* through
its self-test.

The general point, for the next checker of checkers: **running a gate's own
cases is not running the gate**, and a wiring audit that cannot tell the two
apart will certify a deleted check as present.

### Third arm: self-tests nobody runs — one found and fixed, fifteen absent

Measured 2026-09-02, same session. **17 of 32 gates ship a `--self-test`; 13
were run as one.** Three of the four gaps were lane C gates that nothing runs
at all, so the ratchet suppresses the finding for unwired gates — restating a
finding trains the reader to skim it.

The fourth was live: **`check-option-refusal.py` was wired and running while
its own fixtures had never executed in the blocking path.** It scans
`kernel/src/kshell.rs`, so it fails the usual silent way — lose the Rust parse
and it reports no findings, which is spelled exactly like a clean tree. Its
self-test passed when finally run, so wiring it (`db691d1b0`) was safe; it now
catches the regression rather than the absence.

**The remaining debt is the fifteen gates with no self-test at all**, i.e. the
*detector* half untested:

`check-boot-skips` · `check-design-decisions-bands` · `check-evdev-elf-asm` ·
`check-frame-needles` · `check-gated-selftests` · `check-generated-tables` ·
`check-libc-shape` · `check-live-counter-reads` · `check-query-status` ·
`check-self-tests-wired` · `check-selftest-reinit` · `check-usage-status` ·
`check-user-access-sites` · `check-vfs-permission-gate` · `check-vfs-under-lock`

For each of these the question "does a planted defect actually make it exit
non-zero?" is **unanswered**. `check-gates-can-refuse.py` proves only that
*some* non-zero exit is reachable; it says nothing about whether the detector
fires on a real defect.

Not turned into a ratchet, deliberately. Pinning fifteen entries that all say
"nobody has written one yet" is a list nobody reads. The version worth having
is a ratchet on *new* gates — no newly added `check-*.py` may ship without a
self-test — which stops the debt growing without demanding fifteen retrofits
first. That is a policy affecting all three lanes' files, so it belongs in a
request to A and C rather than a unilateral lane-B gate. **Filed `c2ab2c30c`**
as `requests/b-ac-should-a-new-gate-be-allowed-to-ship-without-a-self-test.md`,
asking for agreement or objection to one rule: *a newly added
`scripts/check-*.py` ships a `--self-test` covering at least one true positive
and one true negative, plus a call that runs it.* "One of each" is the
load-bearing half — positives alone pass a checker that reports everything,
negatives alone one that reports nothing.

If that request is never answered, nothing breaks and nothing improves: lane B
applies the rule to its own new gates, the fifteen stay untested, and the count
drifts upward as gates are added — which is the status quo, and is exactly how
it reached fifteen.

**Answered 2026-09-03 by lane A: yes, unnarrowed** — see
`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md`. Lane
A declined the offered narrowing to source-parsing gates on the grounds that the
narrowing describes where the failures happened rather than where the failure
mode lives, and reported two real bugs that its own *markdown*-reading gate's
fixtures caught before it ever ran on the real document.

### The two defects are now graded apart — and what is left after that

**Done 2026-09-03, `75af74e0b`,** on lane A's §4. "Gate not run" and "self-test
not run" print under separate headings with separate counts, because they are
not equally actionable: **wiring a gate can turn another lane's build red and so
may need that lane's agreement; wiring a gate's `--self-test` cannot**, since a
self-test reads only fixtures the checker carries in its own source. The heading
states that reason inline rather than pointing at it — a reader of a red build
has not read this file.

Both halves still exit 1, and the self-test arm deliberately has **no `PINNED`
equivalent**: `PINNED` is justified by excuses that genuinely exist for wiring a
gate (WSL, a stale build artifact), and by lane A's own argument no such excuse
can exist for wiring a self-test. Rationale, and the alternative I rejected, are
in design-decisions.md §754.

**What remains open in this entry after §753 and §754** — listed together
because the three are easy to mistake for one:

| | status |
|---|---|
| a gate nothing runs | **closed** — reported; `PINNED` requires a written reason; pruning enforced both ways |
| a wired gate whose self-test nothing runs | **closed** — reported, and unsilenceable except by wiring it |
| a wired gate that **declines on every host** | **OPEN — and the ratchet cannot close it** |

The third is the visibility regression `--may-skip` introduced. It cannot be a
fourth arm of `check-gates-are-wired.py`: that script is a static reader of
script *text*, and "did this call skip?" is a fact about a *run*. A checker that
answers a question it cannot see is the precise defect this entry is about. The
per-run evidence exists (`RUN_CHECKER_SKIPPED`, and the spoken `SKIPPED` line);
what is missing is anything that accumulates it across runs, which needs a
record of past runs this tree does not keep. **That is the remaining work item
under this heading**, and it is a larger thing than a gate.
