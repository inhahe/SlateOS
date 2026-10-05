## §338 — `human` is checked against a *committed measurement* of GNU, not a live diff

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)

**In short:** every other coreutils utility we've ported is tested by running
ours and GNU's side by side and diffing. `human` — the code that turns a byte
count into `1.0M` or `999 kB` — cannot be tested that way, because there is no
`human` command to run. The only way to make GNU print a given number is to own
a file that big, or to copy that many bytes. So the numbers were measured once,
and the answers are checked into the repo as a table.

**The problem.** `human` is a library module ported from gnulib's
`lib/human.c`. The existing harness (`scripts/test-diff.sh`) is built around
executing two binaries; there is no binary here. The obvious alternative — have
the test shell out to GNU on the fly — founders on how GNU is reached: nothing
accepts "render this number for me". Every caller renders a number it *already
has*, from a file it can see or bytes it has moved.

**What was chosen.** A committed fixture,
`userspace/coreutils/tests/data/human-gnu.txt` (~30k rows), produced by
`scripts/gen-human-fixture.sh` and consumed by `tests/human_gnu.rs`.

**Alternatives, and why not:**

| Option | Why not |
|---|---|
| Live diff, like every other utility | No binary to diff against. |
| Shell out to GNU from the test | Would require GNU coreutils installed to run *our* suite, several GB of scratch, and minutes of runtime. A unit test that stands up 3749 files is not a unit test. |
| More hand-written expectations | This is what existed, and it is what missed the bug below. |

**The cost, stated plainly:** the fixture can go stale against a newer GNU and
nothing detects that automatically — it is re-measured only when someone runs
the generator. That is a real downside, accepted because the alternative is a
test most checkouts cannot run at all. The fixture header records the GNU
version that produced it (8.32) so a disagreement can at least be attributed.

**Why a sweep rather than a few more hand cases.** gnulib makes two rounding
decisions twelve lines apart, `human.c:301` and `human.c:327`, written almost
identically — one compares against 2, the other against 0. The port had the
first transcribed over the second. Every hand-written rule test passed anyway:
the rules were all still obeyed, just with the SI carry 450 bytes early.
Measured on GNU, `999499` is `999 kB` and `999500` is `1.0 MB`; the bug moved
that boundary to `999050`. No hand-picked case was ever going to land in that
gap, because choosing the case requires already knowing where the boundary is —
which is the thing under test. A sweep whose expectations come from GNU rather
than from the author's understanding of GNU is the only construction that
catches this class of error.

**Three instruments, because one cannot cover it.** `ls -l` (ceiling,
autoscale, both bases); `du -a --apparent-size -B N` (ceiling with a block-size
divisor — a branch `ls` never reaches, because `ls` always passes from=to=1);
and `dd` (round-to-nearest, which as far as this sweep could establish no other
coreutils caller uses — `ls -h`, `du -h` and `df -h` all round *up*, so that a
file never renders smaller than it is).

**Two limits worth knowing.** The ls/du sweeps stand up sparse files, which is
free — but NTFS refuses lengths past ~16 TB, so the sweep stops at 5e12 and
reaches exponent 4 (`T`) in both bases; exponents `P E Z Y R Q` are unreachable
by measurement and rest on unit tests against the letter table alone. And `dd`
must actually move every byte it reports, so that sweep is capped at 20 MB —
enough to reach exponent 2 and both sides of the 999500 boundary.
