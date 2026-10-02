## B-PATCH-CANNOT-SAY-WHY-IT-FAILED-TO-FIND-THE-TARGET (lane B, 2026-09-12) — FIXED

When `patch` could not find the file to patch, this build printed
`patch: can't open file X: No such file or directory` to stderr. GNU prints a
whole block to **stdout**, exit 1, nothing on stderr:

    can't find file to patch at input line 3
    Perhaps you used the wrong -p or --strip option?
    The text leading up to this was:
    --------------------------
    |--- a/base.txt	<stamp>
    |+++ new.txt	<stamp>
    --------------------------
    File to patch: 
    Skip this patch? [y] 
    Skipping patch.
    1 out of 1 hunk ignored

Ours told the reader the file was missing. GNU tells them **why**, which is
almost always that `-p` is wrong — and that is the thing a person actually
needs. The echoed header is why `FilePatch` now carries `header_lines`
verbatim: the timestamps come from the patch file, so a reconstruction would
not match.

**The two prompts are printed and not asked.** GNU asks them on a terminal;
with stdin elsewhere it takes the defaults and skips. Every harness case runs
that way, so echoing them reproduces the transcript without pretending to have
read an answer.

**The last line of the fix was an ORDERING one, and it was worth nine cases.**
With the block written, output still differed on every missing-target case
because this build announced `patching file X` *before* discovering the file was
missing. GNU announces only after it has found the target. Moving one emission
after the read took `patch-diff.sh` from 20 passed to **29**.

That is the third ordering bug in `patch` in one night — `-d` chdirs before the
patch file is opened, groups-then-gid-then-uid in `become_user`, and now this.
**When two correct-looking operations disagree, the question is usually which
happens first.**

`patch-diff.sh`: 3 passed / 62 differed this morning, **29 / 36** now.
