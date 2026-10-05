## B-A-CHECKER-THAT-CANNOT-BE-LAUNCHED-IS-REPORTED-AS-A-RESOURCE-SHORTAGE (lane B)

**Status: FIXED 2026-09-03** (lane B). `scripts/run-checker.sh` now reads the
exit code before advising, and every arm quotes the checker's own first line of
output. Cases in `scripts/test-pre-push-run-checker.py`, group 4b.

**The fix deviates from the one filed below, because the filed one was half
wrong.** It says 126 and 127 both mean "could not be launched", so both should
be routed away from the memory explanation. 126 is that. **127 is not** — on
this host a `fork()` refused by the Windows commit limit surfaces as exit 127,
which is measured, not supposed: `scripts/boot-history.py`'s
`HARNESS_ABORT_EXITS` records it. Implementing the entry as written would have
replaced one confidently-wrong message with another, telling the reader their
invocation was malformed while the machine was out of memory.

So 127 offers *both* readings and hands the reader the discriminator rather
than guessing for them: the checker's own first line, which every arm now
quotes inline. If it names a command as not found, the invocation is wrong; if
it names a resource or says nothing, re-running it alone is the right move.
That the message got longer is the point — it is the only place the two causes
can be told apart, and the reader is standing in front of a failed push with
nothing else to go on.

The load-bearing assertions are the negatives: that the 126 arm does *not*
carry the contention advice, and that an ordinary code still does. A test that
only checked the new text would pass against a message that had gained a
paragraph and kept the wrong one.

The original report follows unchanged.

**In short:** when a push gate's checker exits non-zero without reaching a
verdict, `scripts/run-checker.sh` prints a paragraph telling the reader the
usual cause is another memory-hungry build running at the same time, and to
re-run it alone. That is good advice for the failure it was written for. It
is wrong, and expensively wrong, for a checker that was never launched at
all: exit 126 with `Argument list too long` on stderr is a hard limit, not
contention, and "re-run it alone" costs a sixteen-minute push to arrive at
exactly the same wall.

**Where:** `scripts/run-checker.sh`, the note printed on a non-zero exit.

**The proper fix:** distinguish "the checker ran and fell over" from "the
checker could not be started". Exit 126 (found but not executable / could not
be launched) and 127 (not found) are the shell's own codes for the second,
and neither is ever a checker's own verdict — no checker in this tree returns
either. Print the resource-contention advice for other codes, and for these
two say what actually happened and point at the invocation rather than at the
machine's memory. The checker's captured stderr is already saved to a log
file, so the message can also say "its first line is …", which is where
`Argument list too long` was sitting the whole time.

**If it is never fixed:** every future launch failure sends its reader to
investigate the wrong thing first. Bounded, but it wastes exactly the time of
whatever run it advises repeating — which at the push boundary is the most
expensive run in the project.
