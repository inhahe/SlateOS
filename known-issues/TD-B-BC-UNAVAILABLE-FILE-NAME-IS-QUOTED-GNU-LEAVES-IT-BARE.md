## TD-B-BC-UNAVAILABLE-FILE-NAME-IS-QUOTED-GNU-LEAVES-IT-BARE (lane B, 2026-08-24) -- **Status: FIXED** 2026-09-16

**In short:** ask `bc` to run a file that does not exist and both programs
complain and exit 1. GNU writes `File nosuch.bc is unavailable.`; we write
`File 'nosuch.bc' is unavailable.` — the same sentence with the name in quotes.
The quotes are there on purpose, and the question is whether that purpose
outranks matching GNU exactly.

**Where:** `userspace/coreutils/src/bin/bc.rs`, `Trouble::report`, the
`Self::Unavailable` arm, which calls `quoteaf_os` (the always-quote form).

**The case for the quotes** is the one the whole tree already accepts for every
coreutils diagnostic: a file name may contain spaces, newlines or control
characters, and an unquoted one in the middle of a sentence is then unreadable
or actively misleading. `quoteaf_os` was chosen here specifically because the
name sits mid-sentence, where an elided quote would blur into the words either
side of it.

**The case against** is that `bc` is not coreutils. It is a different upstream
with a different convention, and this tree's diagnostic policy was adopted to
match *GNU coreutils*, not to be applied to every program regardless of which
program it is imitating. Matching GNU `bc` is the stated goal of
`scripts/bc-diff.sh`.

**Options:**

* **(a) Match GNU bc: print the name bare.**
  *What changes:* the message reads `File nosuch.bc is unavailable.`, and a
  name containing a space or a newline goes into the sentence as-is.
* **(b) Keep the quotes.**
  *What changes:* nothing; the harness row becomes a permanent divergence and
  should move from `known_bug` to `xfail` with this reason attached.
* **(c) Quote only when the name needs it** (`quotef_os`, the eliding form).
  *What changes:* a clean name prints bare and matches GNU byte-for-byte; a
  name with a space or a control character is quoted and does not. GNU
  coreutils' own `quotearg` behaves this way.

**If it is never decided:** nothing gets worse and nothing is blocked; one
harness row stays yellow. Recommendation: **(c)** — it makes the common case
match GNU exactly and keeps the protection for the case that motivated the
policy. It is a user-visible message, so it is written down here rather than
quietly changed.

### Fixed 2026-09-16 — option (c)

Taken, and recorded as `design-decisions.md` §1027.

**What decided it was not in the options above.** Since the syntax-error prefix
started naming its source file, `bc` printed the same name two ways in one
program — `File 'prog.bc' is unavailable.` beside `prog.bc 1: syntax error`.
That inconsistency post-dates this entry and is what turned a matter of taste
into an obvious call: of the two spellings, the eliding one also matches
upstream.

**The forgery protection is intact**, which matters because it is the only
reason the quoting existed. `quotef_os` quotes a name containing a newline,
space or control character and leaves an ordinary one bare, so
`x⏎b c: /etc/shadow: Permission denied` still cannot forge a line. Verified
rather than asserted: `nosuch.bc` prints bare and matches GNU byte for byte,
and a newline-bearing name prints as `'a'$'
''bc: forged'`. Both are harness
rows, the second `differs_by_design`.

**Not put to the operator**, though the entry said it was written down rather
than quietly changed. The reasoning is in §1027: the new inconsistency makes
one option strictly better rather than leaving a fork, and the operator's queue
already holds nine unanswered lane-B questions. One function call to reverse.

**Evidence.** `bc-diff.sh` now reports **0 known bugs** — 159 -> 160 passed,
15 differ on purpose. This was the last tracked differential difference in the
shipped surface.
