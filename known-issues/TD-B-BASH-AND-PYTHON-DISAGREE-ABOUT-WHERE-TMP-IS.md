## TD-B-BASH-AND-PYTHON-DISAGREE-ABOUT-WHERE-TMP-IS (lane B, 2026-09-13)

On this host, MSYS bash resolves `/tmp` to its own root's temp directory, and
a native Windows Python resolves the same string against the CURRENT DRIVE --
`E:/tmp`. They are different directories.

**The symptom is not an error.** A file written by one and read by the other
is simply absent, so a scan reports **zero findings** and a checker reports a
clean tree. It cost two measurements on 2026-09-13: a roadmap comparison that
reported 0 stale citations, and a duplicate-symbol scan that reported 0
exports from a tree that had 1,352.

Both times the giveaway was a number that was implausibly round rather than an
exception.

**What to do instead:** cross-process scratch files go in the repository (and
are removed afterwards), or use an explicit Windows path. `mktemp -d` from
bash and `tempfile.TemporaryDirectory()` from Python are each fine used
*within* one process -- the hazard is only handing a path from one to the
other.
