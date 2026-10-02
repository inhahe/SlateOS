## BUG-BC-PRINTS-A-BANNER-A-LEADING-ZERO-AND-NEVER-WRAPS (lane B) — WITHDRAWN 2026-08-22, THE BUGS WERE NEVER THERE

**In short:** This entry reported three bugs in `bc` that `bc` does not have.
The harness that found them was running a *different program of the same name*:
two packages in this workspace both build a file called `bc.exe` into the same
place, and the one the harness happened to pick up was an older, abandoned
implementation. The `bc` we ship agrees with GNU on all 200 cases, and did on
the day this was filed. Nothing below needs fixing; the real defect is
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`, filed the same day.

**What actually happened.** `scripts/calc-diff.sh` named its subject as the
*path* `target/x86_64-pc-windows-gnu/debug/bc.exe` and ran it without building
it. Two packages write that path — `userspace/bc` (the bignum rewrite, which
this harness was written to certify) and `coreutils/src/bin/bc.rs` (a separate
June implementation nobody has touched since) — so which program the harness
measured was decided by whichever had been built last, at some unrelated earlier
moment. It was `coreutils`'s. Rebuilding `userspace/bc` and re-running the
identical harness, unchanged, gives **200 passed, 0 differed**.

So all three sections below are accurate descriptions of
`coreutils/src/bin/bc.rs`, and none of them describes `userspace/bc`:

| # | Claimed of `bc` | True of `coreutils`'s bc | True of `userspace/bc` |
|---|---|---|---|
| 1 | banner on a pipe | yes | no — gated on `stdin.is_terminal()`, and `-q` accepted (`main.rs:2139`) |
| 2 | `0.333…` not `.333…` | yes | no — fixed in `a1625db82` |
| 3 | never wraps | yes | no — `BC_LINE_LENGTH` honoured, `wrap_chunk` (`main.rs:1121`) |

**What this cost, and the lesson.** A day of a red harness that everyone
believed, an `all-diff.sh` invocation documented as needing `calc` skipped, and
three fixes queued against code that already had them. The reading that should
have raised the alarm was in the entry itself: it said the harness "has been red
at roughly this number since it was written", which is not how a harness written
*to certify a rewrite* behaves — it is how one behaves when it is pointed at
something other than the rewrite. The measurement was doubted only after the
banner text in the failure rows (`bc 1.0 (slateos coreutils)`) turned out not to
occur anywhere in `userspace/bc/src/main.rs`, which says the source of the
program under test in one grep. **When a differential harness disagrees with a
unit test, check that they are testing the same binary before believing either.**

The three sections are kept below as the record of what was claimed, and because
they remain a correct bug report against `coreutils/src/bin/bc.rs` — which is
one of the 42 duplicates that has to be resolved one way or the other.

Reproduce (as originally written): `bash scripts/calc-diff.sh`. Every case is
`printf '<expr>\n' | bc` against GNU bc 1.07 in WSL. The harness now builds
`-p bc --bin bc` itself, so it can no longer be reproduced.

### 1. The welcome banner is printed even when stdin is not a terminal

```text
ours   stdout `5`   stderr `bc 1.0 (slateos coreutils)` + `Type "quit" to exit.`
gnu    stdout `5`   stderr empty
```

GNU prints the banner only on an interactive session, so `echo 2+3 | bc` is
exactly one line. Ours prints it unconditionally, which means every pipeline
that uses `bc` gets two lines of noise on stderr. This alone accounts for most
of the 105 rows. The fix is to gate the banner on `stdin.is_terminal()` — and
to check whether `-q`/`--quiet` is accepted, since that is the other way GNU
suppresses it.

### 2. A value below 1 is printed with a leading zero

```text
$ printf 'scale=10; 1/3\n' | bc
ours   0.3333333333
gnu     .3333333333
```

POSIX bc prints no integer part when it is zero. Ours emits `0`. Affects every
scaled division in the harness. The fix is in whatever formats a `Number` back
to decimal; note the sign case too (`-0.5` is `-.5` in GNU).

### 3. Long output is never wrapped

GNU bc breaks a number longer than 70 characters with a `\` and a newline:

```text
gnu    10715086071862673209484250490600018105614048117055336074437503883703\
       51051124936122493198378815695858127594672917553146825187145285692314\
       ...
ours   (one 302-character line)
```

The width is bc's `BC_LINE_LENGTH`, 70 by default, and GNU honours the
environment variable of that name (0 disables wrapping). Ours has no concept of
it. The fix is a small output filter applied to every line bc writes, plus
reading `BC_LINE_LENGTH`; do not apply it to the *input* echo or to
diagnostics, which GNU does not wrap.

**The proper fix is all three, measured against the harness** — `calc-diff.sh`
already covers them, so the work is done when it reports 0 differed rather than
when the three changes are written.

*(2026-08-22: the paragraph above is what made this entry survive a day. It is
right that the harness is the measure, and the harness did report 0 differed —
the moment it was pointed at the right binary. "Measured against the harness" is
only worth anything if the harness builds what it measures, which is what
`scripts/diff-subject.sh` now guarantees for all twenty-seven of them. That
guarantee moved to `scripts/diff-wsl.sh` on 2026-08-25, unchanged in substance
and now covering all forty-five; `diff-subject.sh` itself is gone. See
design-decisions.md §382.)*
