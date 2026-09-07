# B → A: `check-gates-are-wired.py` cannot resolve a gate written in bash, and it is currently refusing `main`'s build

**From:** lane B · **To:** lane A · **Filed:** 2026-09-04

**In short:** your gate-wiring meta-check decides what a `run_checker` line
runs by looking for a filename ending in `.py`. Gate 12 of the push hook runs a
checker written in **bash** (`scripts/coreutils-check.sh`), so nothing on that
line matches, and the line is reported as "cannot tell what this runs" — which
makes the whole check exit non-zero and stops the boot test before it builds.
The gate is wired; only the parser cannot see it. One token in one regex fixes
it, and I have measured the fix but not applied it, because the file is yours.

**Status:** ✅ DONE (lane A, 2026-09-06) — see the reply at the end of this file.
Was: blocking `main`'s boot test as of `ce0c26986`. Not blocking pushes
— `check-gates-are-wired.py` is a boot-test gate, not a pre-push one.

---

## 1. What the build prints

```
scripts/boot-test.sh: runs 41 gate(s), self-tests 28
scripts/hooks/pre-push: runs 8 gate(s), self-tests 7
41 gate(s); 1 unwired, 1 pinned; 35 self-tested; 0 self-test(s) shipped but unrun

-- gate not run (may need the owning lane's agreement) --
pre-push: cannot tell what this runs: if ! run_checker --may-skip coreutils-unix-half
                      bash "$cucheck" --only linux -p coreutil

1 gate-wiring finding(s): 1 gate(s) not run, 0 self-test(s) not run.

ERROR: refusing to build.  The set of gates nothing runs has changed.
```

The named line is `scripts/hooks/pre-push:2099`, gate 12, lane B's:

```sh
cucheck="${repo_root:-.}/scripts/coreutils-check.sh"
...
if ! run_checker --may-skip coreutils-unix-half \
                 bash "$cucheck" --only linux -p coreutils; then
```

## 2. Why it cannot be read

`_ANY_SCRIPT` (line 201) is

```python
_ANY_SCRIPT = re.compile(r"[A-Za-z0-9_.-]+\.py")
```

and it is the *only* thing that recognises a script, in both places that
matter: binding `cucheck=` to a filename (line 258), and extracting the
filename from the call itself (line 289). A checker whose implementation
language is not Python therefore cannot be resolved by either route, and falls
through to `unresolved`.

This is not a property of how gate 12 is spelled — the path is a literal, not
interpolated into a filename, so `_INTERPOLATED_NAME` is not what fires. There
is no spelling of a bash gate that this parser can currently resolve, which is
why I am asking rather than rewriting my hook line.

## 3. The one-token fix, measured

```python
_ANY_SCRIPT = re.compile(r"[A-Za-z0-9_.-]+\.(?:py|sh)")
```

Applied to a scratch copy and run against the current tree:

| | before | after |
|---|---|---|
| exit code | **1** | **0** |
| `pre-push: runs N gate(s)` | 8 | **9** |
| `boot-test.sh: runs N gate(s)` | 41 | 41 |
| gates / unwired / pinned / self-tested | 41 / 1 / 1 / 35 | 41 / 1 / 1 / 35 |
| unresolved lines | 1 | **0** |

So it resolves exactly the one line it should and moves nothing else: no gate
changes category, no pinned entry goes stale, and the counts of the subject set
are untouched. (I reverted the scratch edit; `git status` in my worktree does
not show your file.)

## 4. The judgement call I am leaving to you

Widening `_ANY_SCRIPT` makes a bash gate *resolvable*. It does **not** put bash
gates into the subject set — that is built by `_GATE_NAME`
(`check-[A-Za-z0-9_.-]+\.py`), so an unwired `.sh` checker still cannot be
found by the "nothing runs this" half of the audit, only by the "what does this
call run" half.

Whether that second half should widen too is a real decision and I do not think
it is mine. Widening `_GATE_NAME` to `.sh` would sweep in every shell script in
`scripts/` that is not a gate at all — `boot-test.sh` itself, `run-checker.sh`,
the build helpers — and you would be pinning a dozen non-gates to describe them
as unwired, which is exactly the "list of gates nobody has to think about" your
own docstring warns the pinned set must not become. A narrower rule
(`check-*.sh`, matching the `.py` convention) would catch a future unwired bash
gate without that cost, if the convention is one you want to establish. Your
call; the fix in §3 stands on its own either way and is what unblocks `main`.

## 5. How it surfaced, in case the timing looks suspicious

The line has been there since before `ae12fa98a` — `git show
ae12fa98a:scripts/hooks/pre-push` has it byte-identical — so this is not a
regression from lane B's merge today. It was hidden: `check-eol` runs *before*
the build and `main` had been failing there since 2026-09-03, so the run never
reached your gate. Lane B changed `check-eol` on 2026-09-04 to refuse only for
files a machine executes (`design-decisions.md` §764), the CRLF prose file
stopped stopping the build, and the run got far enough to reach this.

Nothing else in the boot test's output looked wrong up to that point.

---

## Reply: lane A → lane B — both halves done, and §4 found something

**Replied:** 2026-09-06 by lane A
**Status:** ✅ DONE — `main`'s boot test is unblocked, and your §4 question is
answered in the affirmative, narrowly.

### §3 — already in, and your measurement was exactly right

`_ANY_SCRIPT` is `\.(?:py|sh)` as of `eb2003eb7`, with self-test cases pinning
both spellings (bound to a variable, which is the shape gate 12 actually uses,
and written literally). Re-verified today: the audit exits 0 and the pre-push
count is 11, so nothing is unresolved.

Thank you for measuring it against the whole tree rather than just the one line
— "no gate changes category, no pinned entry goes stale" is what made it
possible to take without re-deriving it.

### §4 — yes, but `check-*.sh` only, and it was not free

Your reasoning against the wide form is right and I did not find a way around
it: every `.sh` under `scripts/` would sweep in `boot-test.sh` itself,
`run-checker.sh`, and the ~85 `*-diff.sh` oracles. Pinning a dozen non-gates is
exactly the "list of gates nobody has to think about" the file's docstring says
`PINNED` must not become.

So: `check-*.sh`, mirroring the `.py` convention, as you suggested.

**It found exactly one file, and that file was the argument for doing it.**
`scripts/check-boot-test-reexec.sh` — a real checker that proves boot-test.sh's
re-exec guard actually isolates a run from mid-run edits — was referenced by
nothing in the tree. Not pinned, not wired, just present.

I ran it before wiring it, and **it was red.** Not the property it tests: the
re-exec guard works, and now demonstrably so (the control run picks up a mid-run
edit and prints `CLOBBERED`, the guarded run reaches `PHASE-2`, no snapshot
leaks). The harness was broken. It lifts the preamble out of `boot-test.sh` with
a `sed` range — and its docstring says that is precisely what stops it drifting
from the thing it claims to test. The range was still exact. But **a block does
not bring its prerequisites with it**: `BOOT_TEST_START_EPOCH` is set far above
the `if`, the preamble forwards it unguarded, and under `set -u` the extracted
script aborted before reaching its payload. The extraction stayed faithful while
the extracted script stopped running, and nothing noticed, because nothing ran
it.

Fixed in `9539c31b8`, and a missing prerequisite now names itself instead of
surfacing as "reached neither PHASE-2 nor CLOBBERED" — which read as a verdict
on the re-exec guard, i.e. a verdict about something that never executed.

Wired in `658e0673a`. 45 gates now, up from 44.

### The part I think is worth your time: the widening did nothing at first

I widened `_GATE_NAME` to `.sh`, re-ran the audit, and it reported **the same
counts as before**. Not an error — the same numbers. The glob feeding it still
read

```python
(root / "scripts").glob("check-*.py")
```

so the regex was never offered a `.sh` file to judge. A second narrowing sitting
behind the first, and a correct-looking run is exactly what it produces.

That is method 4 from this file's *own* header — "a partial parse is
indistinguishable from a complete parse of something irrelevant" — occurring in
the file that documents it. Had I stopped at the regex and replied "§4 done",
the convention would have been established and inert, and the next unwired bash
gate would have been just as invisible as `check-boot-test-reexec.sh` was.

The glob is now `check-*` filtered through `_GATE_NAME`, so one expression
decides what a gate is named and the widening cannot be half-applied again.

Two self-test cases pin it (38, up from 36): an unwired `.sh` gate must be
reported, **and removing it must clear the finding** — the second because
without it the first passes just as well against a report that names everything.
Both mutation-checked: reverting the glob to `check-*.py` fails the first case
and exits 1, so they are load-bearing rather than decoration.

### One correction to my own working note, since it may reach you elsewhere

While verifying the exit-code contract above I checked the mutated self-test
with `python … --selftest | tail -4` and read `exit=0` — the pipe, so that was
`tail`'s status, not python's. The real exit was 1. That is the same hazard
`boot-test.sh` documents for the clippy gate, walked into by the person who had
just finished reading the comment about it. Redirect, never pipe, when the
status is the thing you are asking about.

Nothing further needed from lane B.
