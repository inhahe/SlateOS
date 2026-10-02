## 748. Gate integrity is measured in three independent arms — can it refuse, does anything ask it, do its own cases run — and enforced as a ratchet rather than a gate

**Date:** 2026-09-02
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `scripts/` holds thirty-two small programs that read the codebase
and answer yes or no; a build runs them and stops if one says no. A program like
that can stop enforcing anything in three unrelated ways — it may be incapable
of ever saying no, nothing may ever run it, or the fixtures that prove it can
still spot a defect may never execute — and **all three look identical in the
log to a program that ran and found nothing wrong.** The decision is to check
each of the three separately, with its own tool and its own verdict, and to
enforce the second as a *ratchet* (today's gaps are pinned by name with a
reason; only a change to the set is a failure) rather than as a gate that would
turn the other two lanes' unfinished work red overnight.

### Why the evidence is an absence, and what that costs

Every other check in this repository fails by producing something: a finding, a
diff, a non-zero exit. These three fail by producing *nothing* — and "nothing"
is the same byte sequence as success. That is why all three had to be found by
accident or by deliberate hunting rather than by anything going red:

| Failure | How it was found | Fix |
|---|---|---|
| `check-doc-links.py` could not fail at all — every refusal sat behind `--check`, and a bare run fell through to `ap.print_help(); return 0` | a log ended in a usage message | `165766dbf` |
| `check-option-refusal.py` was wired and running while its own fixtures had never executed | `check-gates-are-wired.py`'s third arm, on its first run | `db691d1b0` |
| eight gates were run by nothing that blocks | measuring, because the second arm was being written | pinned; filed to A and C |

The first of those had been in the tree scanning for 412 seconds per run,
finding dead links, and reporting success.

### Why three tools and not one

They share a subject and nothing else. `check-gates-can-refuse.py` is an AST
question about one file at a time ("is any non-zero exit reachable from a bare
run?"). `check-gates-are-wired.py` is a shell-parsing question about the *call
sites* ("does any `run_checker` line name this?"). The third arm — does a gate
that ships a `--self-test` have anything that runs it — is a question about both
at once, which is why it lives inside the second tool rather than in a third.

Merging the first two was considered and rejected: they disagree about what an
input even is. Aim the first at a historical file out of `git show` and it
answers usefully; aim the second at one and the question is meaningless, because
wiring is a property of the tree, not of the file. That difference is not
cosmetic — being able to aim the first tool at a known-bad file is the only
reason its own false negative was ever found (it modelled `if args.flag:` but
not `if args.flag is not None:`, so on its first run it was green and wrong
about the exact defect it was written for).

### Why a ratchet, not a gate

Six of the eight unwired gates are lane C's and one is lane A's. A hard gate
would stop all three lanes' builds on work none of them had scheduled, which is
the shape of thing that gets worked around — commented out, `|| true`'d — rather
than fixed. The ratchet pins each by name with a reason and a filing date, so:

- **nothing is red today**, and
- **the set cannot quietly grow**: a new unwired gate is a finding, and
- **the list cannot quietly rot**: a pinned entry that is now wired is *also* a
  finding, because an exemption list nobody prunes stops describing the tree it
  exempts.

| Option | Why not |
|---|---|
| Hard gate on all thirty-two | Reds two lanes for work they did not schedule; see above. |
| Wire the seven myself | They read `gui/**` and `kernel/src/**`. Writing outside my lane is the failure mode the three-lane split exists to prevent. Filed as requests instead. |
| Wire all eight | One of them **must not** be wired as things stand: `check-libc-shape.py` returns 2 ("I could not look") when `libc.a` is stale, and `run_checker` aborts the whole build on any exit but 0/1. Wiring it would stop everyone's build the first time that path was taken. Its pin says so, in those words. |
| No exemption list; just a count that must not rise | A number cannot be pruned and does not say whose file it is. Eight names with eight reasons is the artifact someone can act on. |

### Running a gate's own cases is not running the gate

The second tool's first version counted a `--self-test` call as wiring, and a
planted mutant survived because of it: deleting the real `run_checker
check-tick-wiring …` line left the verdict clean, since the adjacent self-test
call still named the script. So the audit certified a *deleted* check as
present — the same "appears enforced, isn't" defect, inside the tool written to
catch it. The two are now tracked separately and a self-test call never counts
as wiring. Re-measuring after the fix left the counts unchanged, which is the
evidence that no gate was wired *only* through its self-test.

Both spellings `--selftest` and `--self-test` are live here, so the exclusion
matches either.

### Why the fifteen gates with no self-test are not pinned

They are the fourth failure mode — the *detector* half untested — and the answer
is a policy, not a list. Fifteen pins all reading "nobody has written one yet"
is a list nobody reads, and it would be a lane-B tool demanding retrofits in two
other lanes' trees. The version worth having gates only *new* gates, which stops
the debt growing; and because that is a rule over all three lanes' files it was
**filed** (`c2ab2c30c`) rather than implemented. Silence on it is an acceptable
outcome: lane B applies it to its own new gates and the ratchet stays lane-B's.

### The parsing is fussy on purpose

Three obvious ways to measure "is this gate wired" were tried and each returned
a confidently wrong number: a basename grep over-counts (a gate named in prose
or in a comment); stripping comments still over-counts (broken by this same
session's own `echo "as scripts/check-doc-links.py now does."`); and matching a
literal path on the `run_checker` line under-counts (backslash continuations,
and the push hook binding `doclinks=…` two hundred lines before the call). The
committed version joins continuations, strips comments, resolves shell
variables, and **reports calls it could not resolve** rather than dropping
them — an unresolvable call is the one case where guessing either way is worse
than saying so.
