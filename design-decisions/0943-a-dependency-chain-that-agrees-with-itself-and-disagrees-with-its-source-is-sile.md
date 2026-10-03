## 943. A dependency chain that agrees with itself and disagrees with its source is silent

**Date:** 2026-09-15 · **Decided by:** Claude (autonomous) · **Lane:** A ·
**Observed and diagnosed by lane B**, numbered here because 938 and 942 are in
this band and this is their generalisation

**In short:** the sysroot's `libc.a` was 13 posix sources out of date. Every
artifact built from it -- 74 test fixtures, 5 spike binaries, 72 userspace
programs, the ext4 image -- was equally out of date, so they all agreed with
each other and every gate was green. Rebuilding the libc made the tree *less*
consistent and a check went red. That red was the first true statement anyone
had made about it in days.

**938 is the singular case**: one artifact that was true when written does not
say when it stopped being true. **This is the plural**: when everything
downstream of a stale input is regenerated from that input, the whole chain is
uniformly wrong and *internally consistent*. Consistency is what the checks
look for, so a uniformly stale tree passes everything.

| state | what the gates say | what is true |
|---|---|---|
| uniformly stale | all green | every artifact describes a tree that no longer exists |
| partially rebuilt | `image-check` RED | the red names exactly what is behind |
| fully rebuilt | all green | the green means something |

**The middle row is the one to understand.** It looks like a regression and is
not: it is the moment a check finally has something to disagree with. Anyone
arriving mid-rebuild sees a red tree that was green an hour ago and may
"restore" it by reverting the rebuild, which returns to row one -- green, and
wrong. Lane B passed through that window today and said so explicitly rather
than leaving it to be misread.

**Nothing in the chain announces its dependents**, which is lane B's
contribution and the operational half. You find wave N+1 only by fixing wave N
and re-running the check: the libc rebuild revealed the fixtures, the fixtures
revealed the spikes, one spike (`cmake`) refused and needed a separate
invocation, and the userspace sweep then reported *72 of 72 staged binaries
older than the libc*. Nobody could have listed those four waves up front. So
the rule is to **re-run the check after each wave, not at the end** -- stopping
when the first check goes green leaves the tree in row two believing it is row
three.

**Why this is worth a number rather than a note.** The failure has no symptom.
There is no wrong output, no failing test and no error message; there is a set
of artifacts that answer questions about a tree that has moved on. The only
thing that detects it is a check that compares an artifact to its *inputs*
rather than to its siblings -- `sysroot-check`'s recorded content stamps here.
A project without such a check cannot discover this state at all, which is a
stronger reason to keep them than any individual bug they have caught.

**Related:** 938 (the singular), 942 (a verdict is only as good as its corpus;
here the corpus is *time*), and the `image-check`/`sysroot-check` pair, which
are the two halves -- one compares the image to the tree, the other compares
the libc to its sources, and neither alone would have seen this.
