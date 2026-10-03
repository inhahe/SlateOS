## TD-C-A-SPLICE-WITH-TWO-SEARCHED-BOUNDS-DUPLICATED-52000-LINES -- METHOD 2026-09-16

**In short:** I edit these documents with small Python scripts. One of them
silently copied fifty-two thousand lines of `known-issues.md` into the middle
of itself. Nothing errored, the commit succeeded, and the only reason it was
caught is that a pre-push check refused to push the result.

**Date:** 2026-09-16. **Lane:** C.

**The shape.** To replace a region I used:

    i = s.index(start_marker)
    j = s.index(end_marker)
    s = s[:i] + new + s[j:]

`str.index` returns the **first** match. The end marker -- `**What was done
now.**` -- was a phrase I had used in an earlier entry too, so `j` landed
*before* `i`. Then `s[j:]` re-appended everything between them, and the file
grew by 52,422 lines instead of shrinking. Both markers were real, both
searches succeeded, and the arithmetic is only wrong for an ordering nobody
checked.

**Why it survived to a commit.** The obvious guard is the one I had already
been using everywhere else: `assert s.count(old) == 1` before a `replace`. It
does not apply here, because a two-bound splice never calls `replace`. A
technique I had made safe in one form was unsafe in its other form, and the
safety did not carry over with it.

**The two guards, both one line.**

    assert i < j, "end marker precedes start marker"
    assert len(out) == len(s) + len(new) - (j - i)

The second is the general one and is worth preferring: **state the expected
length and check it.** For a pure insert it is `len(s) + len(entry)`, which
catches a duplicated tail, a truncated write and a mis-ordered splice with the
same line, without needing to know which of them happened.

**What actually caught it:** `scripts/check-known-issues-index.py`, which
refuses a push when two `## TD-` headings share a slug. It reported 223
duplicate headings *all at a constant offset of 52,358 lines* -- and that
constant is what identified the cause immediately, because a real duplicate
entry would not be at a fixed distance from its twin. A check written to keep
a triage grep honest diagnosed a corrupted write it was never aimed at.
