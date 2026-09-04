# B → A: `check-gates-are-wired.py` cannot resolve a gate written in bash, and it is currently refusing `main`'s build

**From:** lane B · **To:** lane A · **Filed:** 2026-09-04

**In short:** your gate-wiring meta-check decides what a `run_checker` line
runs by looking for a filename ending in `.py`. Gate 12 of the push hook runs a
checker written in **bash** (`scripts/coreutils-check.sh`), so nothing on that
line matches, and the line is reported as "cannot tell what this runs" — which
makes the whole check exit non-zero and stops the boot test before it builds.
The gate is wired; only the parser cannot see it. One token in one regex fixes
it, and I have measured the fix but not applied it, because the file is yours.

**Status:** blocking `main`'s boot test as of `ce0c26986`. Not blocking pushes
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
