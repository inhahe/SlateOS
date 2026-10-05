## 275. A shell command that can fail to *look* gets three exit statuses, not two: `grep`, `cmp` and `diff` now answer 0/1/2

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When you run `grep needle myfile` in the shell, it can end in
three different ways: it found the word, it read the whole file and the word
isn't there, or it never managed to open the file at all. Our shell only had
two answers for that — "found it" and "didn't find it" — so the third one was
being reported as the second. A script that says "if grep doesn't find the
word, do X" would do X because the *file was missing*, which is a completely
different situation and often means something is broken. The decision is to
give these commands a third answer, matching what the standard Unix tools have
always done: 0 = found / same, 1 = looked and didn't find / different, 2 =
couldn't look.

### The distinction

Exit status (the number a command leaves behind for `&&`, `||` and `if` to
read) is how one command tells the next one what happened. Two-valued status —
"worked" or "didn't" — is right for most commands, because most commands only
have those two outcomes.

Search and compare commands do not. They have an outcome that is a *finding*,
not a failure:

| Status | `grep` | `cmp` / `diff` |
|---|---|---|
| **0** | matched | the files are the same |
| **1** | searched it all, no match | the files differ |
| **2** | could not search | could not compare |

1 is a successful run reporting a negative result. 2 is a run that produced no
result at all. Folding 2 into 1 does not merely lose detail — it replaces "I
don't know" with a confident, wrong "no", and does so in the exact form the
caller is waiting for. That is the same shape as the silent-success bugs this
lane has been sweeping, arriving from the other end: the command *does* report
something, it just reports the wrong one of two answers a caller cannot tell
apart.

Before this change:

- `grep pat missing-file || echo "not present"` reported a missing *file* as
  missing *text*.
- `grep -r pat /typo` reported a mistyped directory as a clean search of it —
  and printed nothing at all on the way past, because the walk's three failure
  paths were bare `return`s.
- `cmp a b && cp a b` was told "identical" whenever the files differed, because
  every path in `cmp` left the status at the 0 the dispatcher sets.
- `diff` was worst: silence is how it says "identical", so a run that *refused*
  to compare (unreadable file, or a file past the 2000-line cap) was
  byte-identical **and** status-identical to a successful comparison finding no
  difference. Nothing distinguished them.

### The alternatives

**(a) Keep two values, and make "could not look" exit 1 (status quo).**
Simplest, and consistent with every other command in the shell. It is what we
had, and it is wrong for exactly the reason above: 1 already means something
specific here, so overloading it destroys the meaning rather than extending it.

**(b) Three values, GNU's assignment — chosen.** 0/1/2 as tabulated. Matches
POSIX and GNU for all three commands, so a script written against real `grep`
behaves the same here, and anyone reading the code recognises the convention
without being taught it. Costs: callers that treat "non-zero" as one thing are
unaffected; callers that specifically test `== 1` now get 2 in the
could-not-look case, which is the entire point.

**(c) Three values, but a distinct number per cause** (3 = unreadable file, 4 =
bad flag, …). More informative, and gratuitously incompatible: no existing
script or habit reads those numbers, and the distinction a caller actually acts
on is "did you get an answer or not", which two failure values already carry.

### The consequences worth knowing

**Usage errors move from 1 to 2** in all three commands. This is the part that
diverges from the rest of the shell, where a usage error is 1. Inside these
three, 1 is a reserved, *meaningful* answer, so `grep -Z pat f || echo absent`
returning 1 for a bad flag would assert that the pattern is absent from a file
grep never opened. The general rule this instantiates: **a command that gives
a finding a status number must not spend that number on anything else.**

**A failure to look outranks an empty result.** `grep` tallies matches and
unreadable targets side by side (`GrepTally`) and reports 2 if anything was
unreadable, even if other targets were searched successfully. A run that could
not read one of three files has not established the pattern is absent.

**"No matches" is withheld when anything was unreadable.** The sentence
`grep: no matches for 'p'` is a claim about a file's *contents*; printing it
after a failed open asserts something never established. Same for `cmp`/`diff`,
which now say nothing about sameness on a path that read nothing.

**This is a template, not a one-off.** Any command whose failure to act is
distinguishable from a negative finding belongs here. Candidates not yet
converted: `test`/`[` (already 0/1 by POSIX and correct as-is), and `find` when
a subtree is unreadable. The trigger for extending it is the question: *can
this command's "no" mean either "I checked" or "I couldn't check"?* If yes, it
needs the third value.

**If a caller only ever tested for zero, nothing changes.** The change is
strictly a refinement of the non-zero space.
