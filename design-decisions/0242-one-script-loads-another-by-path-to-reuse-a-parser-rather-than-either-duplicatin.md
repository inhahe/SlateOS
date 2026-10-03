## §242 — One script loads another by path to reuse a parser, rather than either duplicating the pattern or extracting a shared module

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** Two scripts in `scripts/` need to read the same line out of a
boot log — the `[hypervisor]` banner that says which emulator ran the kernel.
`bench-history.py` already knew how. `boot-history.py` now needs to as well.
Rather than copy the two regular expressions into the second file, it loads the
first file at run time (via `importlib`, by path, because the filenames have
hyphens and are not importable as modules) and calls its function. The cost is
an unusual-looking dependency between two command-line scripts; the benefit is
that there is exactly one parser, so the two files cannot come to disagree.

**The alternatives, and why not.**

*Copy the patterns.* Cheapest to write and the worst to own, for a reason
specific to this parser: the failure mode of a stale copy is **silent**. A
pattern that no longer matches returns `None`, and `None` already has a
meaning here — "this kernel predates the banner and cannot say". So a broken
copy in `boot-history.py` would not raise, would not print anything odd, and
would produce records that look exactly like the several hundred legitimately
unlabelled ones already in the file. `design-decisions.md` §240 forbids
restating a selector for the general version of this reason; this instance is
the aggravated one, because the restatement's failure is indistinguishable from
correct output.

*Extract a shared `scripts/serial_parse.py`.* The clean answer, and the one to
take when a third consumer appears. It is not taken now because the shared
surface is currently one function, and the extraction is not free: `parse_accel`
sits next to `parse_text_pad`, `ACCEL_RE`, `BARE_METAL_RE` and a long comment
explaining why there must be two patterns, none of which have an obvious home
that is not just "the new module", and moving them would touch a file whose
suite was mutation-checked yesterday. Splitting a module to serve two callers,
when one of them is satisfied by an import, buys nothing today and costs the
review of a file that is currently known-good.

*Have `boot-test.sh` pass the accelerator on the command line.* Rejected on the
same grounds `grade-positional.py` rejected it: a run that has to be **told**
which accelerator produced it can be told wrong, and the shell already has two
ways of selecting one (`QEMU_EXTRA`, and the default). The kernel itself
observed the answer and printed it; reading what the guest reported is the only
version that cannot disagree with what actually ran.

**The load is failure-tolerant on purpose, and that is not a hedge.**
`boot-history.py` catches any exception from the load and records `accel: null`
with a warning on stderr. This is the single place in that file where
swallowing an error is right: `boot-test.sh` calls the recorder from its EXIT
trap with `|| true`, so an exception does not surface — it silently drops the
record of the boot. Losing an accelerator label costs one row's grouping;
losing the row costs the evidence of a failed boot, which is the whole reason
the file exists. The asymmetry is what makes the catch correct rather than
lazy, and it is under test in both directions (the record survives; the warning
is printed).

**What would change this decision.** A third consumer of the banner, or any
second function that both scripts need. At that point the shared module pays
for itself and the `importlib` load should be replaced by a real import.
