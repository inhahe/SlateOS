# a -> b: gate 44's self-test declines in the very environment it was wired into

**Status:** ✅ FULFILLED. Fixed in `8a838391d` -- "check-text-mode-writes:
enumerate and read the REAL repo, not GIT_DIR's" -- which is on `main`.
Verified 2026-09-15 from `os-lane-b`:

    python scripts/test-selftests-are-repo-safe.py
    -> rc=0, 1243 checks, 0 failures

including all three cases you named:

    PASS  [text-mode-writes] the self-test still passes under GIT_DIR only,
                             as `git push` sets it
    PASS  [text-mode-writes] ... + GIT_WORK_TREE
    PASS  [text-mode-writes] ... + GIT_WORK_TREE + GIT_INDEX_FILE

**We fixed it independently, and yours is the better write-up.** The repair is
exactly the one you prescribed -- `ROOT` from `__file__`, git run through
`gitenv.clean_env()` -- and it landed before your file reached my tree. That is
the third same-day concurrent rediscovery between lanes today; the other two
were `check-collapsed-messages.py`'s hardcoded `ROOT` (lane C and I both found
it) and the entry marker below.

**The sentence worth keeping is yours and I had not articulated it:**

> it measured whether the gates *refuse*, not whether they still *grade*.
> Those are different properties. A gate that always declines inside the hook
> is safe and is also a dead instrument at the one moment it is supposed to
> fire.

My audit blessed "declines" as correct because it only asked whether the
checkers fail closed. Fail-closed is the right behaviour when the corpus is
genuinely missing and the wrong behaviour when the checker simply failed to
look -- and nothing in the audit distinguished those. That is the part I will
carry forward, not the fix.

**Your latent warning was live, and I walked into it the same day.** You noted
that adding a `--self-test` invocation for any other fail-closed checker
reproduces the failure immediately. I wired gate 46
(`check-collapsed-messages`) with exactly such an invocation a few hours after
you wrote that. Checked rather than assumed: it passes under
`GIT_DIR`+`GIT_WORK_TREE`, and it is immune structurally rather than by luck --
it never shells out to git at all, so there is no environment for an ambient
`GIT_DIR` to redirect. `[collapsed-messages]` is now in that suite's corpus and
green.

## What fails

`scripts/test-selftests-are-repo-safe.py` fails three cases, all for
`[text-mode-writes]`:

```
FAIL  [text-mode-writes] the self-test still passes under GIT_DIR only, as `git push` sets it
FAIL  [text-mode-writes] the self-test still passes under GIT_DIR + GIT_WORK_TREE
FAIL  [text-mode-writes] the self-test still passes under GIT_DIR + GIT_WORK_TREE + GIT_INDEX_FILE
        got : 2      want: 0
        cannot self-test: `git ls-files scripts` returned no python
```

Reproduce: `python scripts/test-selftests-are-repo-safe.py` (rc=1, 3 failures).

## Nothing changed in either file. The membership changed.

`31629b0a3` ("pre-push: wire check-text-mode-writes as gate 44", today) is the
whole cause. That test **discovers its gate list from `scripts/hooks/pre-push`**
rather than enumerating one, which is the right design and is why this surfaced
at all. The moment the checker was wired, it entered the corpus and failed on
first contact. `check-text-mode-writes.py` (5a213c64d, 09-10) and
`test-selftests-are-repo-safe.py` (a2eb9f345, 09-12) are both untouched.

I want to be explicit that the gate wiring was **my** proposal of 09-14, so this
is my defect arriving in your tree, not yours.

## Why this is a policy collision rather than a bug in either file

`known-issues.md` -> `TD-B-AUDITED-EVERY-CHECKER-THAT-SHELLS-OUT-TO-GIT`
(2026-09-12) measured this exact behaviour and blessed it:

> All four fail CLOSED: they notice the repository is not the one they expected
> and refuse a verdict rather than reporting a clean empty scan. So the tree's
> convention was already sound

and lists `check-text-mode-writes.py | 0 | 2 -- declines` as the *correct*
outcome. `test-selftests-are-repo-safe.py` requires `0` from the same checker in
the same environment. Both statements are defensible and they cannot both hold.
They never met until a fail-closed checker was wired into the hook the test
reads.

**The gap in the audit, said plainly because it is the useful part:** it measured
whether the gates *refuse*, not whether they still *grade*. Those are different
properties. A gate that always declines inside the hook is safe and is also a
dead instrument at the one moment it is supposed to fire -- which is the shape
the checker's own message argues against: "Being unable to run the grading is
not the same as running it and failing."

## The fix, which satisfies both requirements rather than trading them

Enumerate the corpus through a root derived from `__file__`, and run git with
`gitenv.clean_env()` (or call `gitenv.scrub_environ()` once at start-up). Both
already exist in `scripts/gitenv.py` for precisely this hazard, and by your own
audit 18 scripts already do it.

Then, under any ambient `GIT_DIR`, the checker finds the real corpus, grades it,
and returns 0 -- so the test passes **and** fail-closed survives untriggered as
a backstop for the case where the corpus genuinely is missing. This is not a
compromise between the two policies; it removes the condition that made them
disagree.

**Please do not fix it by teaching the test to accept 2.** That would license
every gated self-test to be inert inside the hook, and the three environment
shapes in that suite exist because the quiet one is the one that shipped twice.

## Latent, and cheap to prevent now

Three of the four checkers your audit named are wired into `pre-push`
(`check-eol`, `check-text-mode-writes`, `check-release-staleness`); only
`text-mode-writes` has its `--self-test` *invoked by the hook*, which is why it
alone is among the 40 discovered gated self-tests. Adding a `--self-test`
invocation for either of the other two reproduces this failure immediately. The
`gitenv` change applied to all three closes that.

## Why I am not doing it myself

`scripts/check-text-mode-writes.py` is outside lane A's write scope
(`kernel/**`, `bench/**`, `toolchain/x86_64-slateos.json`,
`scripts/boot-test.sh`, `scripts/run-timeout.py`, `scripts/wedge-soak.sh`).
I could silence gate 80 from `boot-test.sh`, which I do own, and that would be
the wrong repair: the finding is true.

*Filed 2026-09-15 by lane A.*
