## B-STAT-DIFF-ALSO-VARIES-BETWEEN-RUNS (lane B, 2026-09-12)

**What.** `scripts/stat-diff.sh` reported **76 passed, 12 differed** inside a
full `all-diff.sh` sweep and **77 passed, 11 differed** run by itself a few
minutes later, with no edit and no rebuild in between.

**Why, and why it is a different fault from `cp`'s.** `stat -f` reports free
blocks. The two sides are run one after the other, so anything that allocates
or frees on that filesystem in between moves the answer — and during a sweep
the other 58 harnesses are doing exactly that. Run alone on a quiet machine the
two calls see the same figure and it passes.

**The harness already has half of this fix, which is what makes it
instructive.** Its header records that `%f`/`%a` differed by one because the
harness's own capture files consumed the blocks it was counting, demonstrated
rather than theorised, and its scratch was moved to `/dev/shm` — deriving the
rule *a harness must not write to the thing it measures*. That rule is true and
it was not enough: it removed the harness's own writes and nothing else's. The
stronger form is **a harness must not measure a quantity anything else can
change between the two sides' calls** — free space, load, the clock, a shared
directory's enumeration order.

**Fix.** Either give `stat -f` a filesystem nothing else is touching (a private
loopback or tmpfs mount made for the run), or exclude the free-space specifiers
from comparison and assert only their *shape*, saying so. The first is better:
`%f`/`%a` are among the fields an implementation is most likely to get wrong,
which is the harness's own stated reason for comparing them at all.

**Both entries share a consequence worth stating once.** `all-diff.sh`'s bottom
line — "60 green, 8 red" — is not reproducible, and neither is any single
harness's. Two of the eight reds are these. A sweep is a sample; a difference
that appears in one run and not the next has not been shown to be a defect, and
one that disappears has not been shown to be fixed.

**First thing to measure next.** Whether `mv-diff.sh` has the same shape —
known-issues already records that `cp-diff.sh` and `mv-diff.sh` must stay
byte-identical across the sections they share, so if this is in one it is
likely in the other. And whether any other harness compares unsorted output
from a directory walk: `ls`, `du`, `find` and `tar` are the candidates.
