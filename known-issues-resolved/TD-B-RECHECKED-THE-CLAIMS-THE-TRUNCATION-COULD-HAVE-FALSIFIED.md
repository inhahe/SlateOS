## TD-B-RECHECKED-THE-CLAIMS-THE-TRUNCATION-COULD-HAVE-FALSIFIED (lane B, 2026-09-10) — closed, one was wrong

**In short:** `check-read-defaults` truncated every file at its first
`#[cfg(test)]`, hiding 35,706 lines across 17 files, and that produced a
**written record asserting a defect was absent** — `B-FDISK-CANNOT-PARTITION`
said fdisk "prints no 'partition table has been altered'" when it printed it
three times. Swept for others. **Exactly one entry was wrong, and it is
corrected.**

**The method, since the first two attempts were too broad.** Grepping
known-issues for absence claims returns 996, almost all about kernel or lane C
code the truncation never touched. Cross-referencing crate names returns noise,
because `at` and `last` match as substrings of ordinary English. What narrows
it is three conditions at once: an entry **dated today** (the truncating
checker is a day old), **naming one of the 17 hidden files**, and **making an
absence claim**. That is six entries.

| entry | verdict |
|---|---|
| `B-FDISK-CANNOT-PARTITION` | **wrong** — corrected, and the fabrication fixed |
| `B-CROND-AND-ATD-NEVER-RUN-A-JOB` | already corrected earlier today, separately |
| `TD-B-FOUR-MORE-PROGRAMS-RUN-A-SHELL-AS-THE-WRONG-USER` | claim holds — re-tested against full source |
| `TD-B-FIVE-PROGRAMS-STILL-TAKE-THE-CALLERS-IDENTITY` | closed; the file lost 79 lines, none relevant |
| `TD-B-HALF-THE-TREE-IS-NOT-SUBJECT-TO-THE-LINT-POLICY` | claims are about manifests, not source |
| `B-SIX-COMMAND-NAMES-HAVE-TWO-IMPLEMENTATIONS` | its gate reads whole files |

**The sudo re-test is the one worth describing**, because it went wrong twice
before it went right. The entry claims "No `setuid`/`setgid`/`CommandExt::uid`
anywhere in `src/`". A pattern search reports **two hits** — which reads as a
falsified claim. Both are false positives: `("stay_setuid", DefaultShape::Flag)`
is a *sudoers option name* in a table, and `record.uid()` is a getter on a
passwd record, not `CommandExt::uid`. The claim holds.

So the check that was hunting a measurement error produced one of its own, in
the same shape, and only reading the two lines settled it. That is the
twentieth such case today and the reason this entry records the *method*
rather than only the verdict.
