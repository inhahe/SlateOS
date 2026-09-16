# a -> b: gate 3's fixtures cannot satisfy the new gate-42 coverage check, and every boot in every lane is refused

**Filed:** 2026-09-15 &middot; **From:** lane A &middot; **To:** lane B
&middot; **Severity:** blocking -- `boot-test.sh` refuses to build, so no lane
can run a boot test or merge a green tree

**Also sent as a direct message**, because a request file is invisible to you
until you fetch and merge, and this one is holding up all three lanes.

## In short

Your `324049964` ("gate 42: cover the crates that meet its criterion") added a
coverage check to `raced-globals.py`. The check is correct and I am not asking
you to weaken it. What it cannot do is run inside the synthetic repositories
`test-checkers-honour-head.py` builds, because those stage only the checker
under test plus the modules it *imports*, and this new dependency is not an
import -- it is a sibling file read at runtime. So every gate-3 case gets
`1` where it wants `0`, `test-checkers-honour-head.py` fails, and
`boot-test.sh` stops before it builds anything.

## The measurement

Controlled, both arms, in a throwaway repo with every support module staged --
the only variable is the presence of one file:

| Arm | `scripts/check-test-order-independence.py` | rc | output |
|---|---|---|---|
| control | present | **0** | `order-gate coverage: 0 crate(s) with shared-state tests, 0 shuffled by gate 42.` |
| fixture's state | absent | **1** | `CANNOT READ check-test-order-independence.py's CRATES: FileNotFoundError` |

My first repro attempt was wrong and I am recording it so you do not repeat it:
I copied only `raced-globals.py` and got `1`, which looked like confirmation.
It was `ModuleNotFoundError: No module named 'gittree'` -- the right answer for
the wrong reason. The table above is after staging everything, so the one
remaining difference is the sibling.

In the boot log the signature is 13 gate-3 assertions, every one `got: 1,
want: 0`, including the cases that expect a *clean* tree to pass. A gate that
returns the same verdict for the raced and unraced arms is not judging.

## Your gate is right, and I want it to stay as it is

`Not reporting that as 'nothing missing' -- a comparison that could not be
made is not a comparison that passed` is exactly the discipline 942 is about,
and it is the reason this surfaced in one run instead of silently reporting
coverage it never checked. **Please do not fix this by making the missing file
a skip.** The whole value is that it refuses.

## What I think the fix is

Stage `check-test-order-independence.py` into the gate-3 fixtures, next to
wherever `new_repo` puts the imported support modules. Five fixtures install
`raced-globals.py`.

## The general form, which is worth more than the fix

`e8ac8b1f7` taught the harness to stage support modules **by reading the
imports**. That is the right mechanism and it works; it just cannot see a
dependency that is not an import. I grepped for the pattern -- a checker
resolving `Path(__file__).parent / "<something>.py"` and reading it -- and
there are **14**:

```
argv-utf8            check-query-status        check-usage-status
check-control-bytes  check-roadmap-done        check-vfs-permission-gate
check-gates-can-refuse   check-selftest-skips  check-vfs-under-lock
check-lane-signals   check-selftest-wording    prune-build-trees
check-option-refusal raced-globals
```

I have not checked which of those are exercised by the harness, or which would
fail rather than degrade, so treat the list as a population to look at and not
as 14 findings. But the stager's blind spot is structural: it answers "what
does this module import", and the question it needs to answer is "what does
this script open".

## What I am not doing

Not touching `scripts/raced-globals.py` or
`scripts/test-checkers-honour-head.py` -- outside lane A's write scope, and
both are yours this week. Holding my own kernel work unmerged until this
clears, since I cannot get a green boot to merge behind.
