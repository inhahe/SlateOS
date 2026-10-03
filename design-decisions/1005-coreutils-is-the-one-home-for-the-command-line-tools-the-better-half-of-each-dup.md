## 1005. `coreutils` is the one home for the command-line tools; the better half of each duplicate pair survives inside it

**Date:** 2026-09-07
**Lane:** B
**Decided by:** Operator (answering `open-questions.md` B-Q7)

Supersedes §8, and un-suspends §359.

June's §8 made "one tool = one project = one file" canonical and retired the
bundle. Its stated reason -- that a per-tool crate is a smaller blast radius
than a multi-call binary -- turned out to be false of this tree: the four
BusyBox-shaped projects it preferred are themselves multi-call, and 41 names
were being built by *both* homes, so which implementation a user got depended
on build order. That cost a day of chasing a bug in the copy nobody was
editing.

**The decision is option B: `coreutils` is the single home.** For each of the
41 overlapping pairs the better implementation wins and moves into
`coreutils`; the duplicate crate is deleted. The 45 names that exist only in
the bundle stay where they are rather than being exploded into 45 new crates.

The operator's own reasoning is worth recording, because it corrects a
misreading this entry invited: the argument "it would create a dependency
shape that exists nowhere in the tree yet" reads like an argument about
*effort*, and effort is explicitly not a reason to choose a worse design here
(`E:\visual studio projects\CLAUDE.md`). It is not an effort argument. A
standalone `userspace/bc` would have to import the `coreutils` *library* --
the per-tool crate depending on the bundle is precisely the shape §8 set out
to retire, so option A cannot be reached without building the thing it was
trying to avoid. B is right on the security argument, the shared library, the
45 non-duplicated tools and the test harnesses independently of it.

**What changes for a user:** nothing visible, except that each of the 41 tools
stops alternating non-deterministically between two implementations. The
better one wins permanently.

**Follow-through:** merge the 41 pairs by hand -- the survey
(`scripts/dup-bins-survey.py`) is a triage aid, not a verdict, and every pair
is read before it is merged. `coreutils-canonical-answer.md` carries the false
premise and is corrected as part of this. `known-issues.md` ->
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES` closes when the last pair
lands.
