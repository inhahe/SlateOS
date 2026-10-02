## TD-C-A-BROKEN-PATTERN-COUNTED-EVERY-LINE-AND-LOOKED-EXACTLY-LIKE-THE-BUG -- METHOD 2026-09-16

**In short:** I checked whether some files I had written had Windows line
endings, the check said every single line did, and I was about to repair three
files that were already correct. The files had none. The pattern I searched for
had been mangled into an empty one, which matches every line, so the number it
returned was the file's length.

**Date:** 2026-09-16. **Lane:** C.

**What it looked like.**

    apps/explorer/src/search.rs: 516 CR line(s)
    gui/toolkit/src/osbytes.rs:   68 CR line(s)

Both numbers are exactly the line count of the file. That is what a wholly
CRLF file looks like -- *every* line ends CRLF -- so the reading "these files
are entirely Windows-ended" fits the evidence perfectly. It was also
corroborating: three files, all written by the same tool, all reporting the
same defect. A consistent wrong answer is more convincing than an
inconsistent right one.

**Why the pattern broke.** The shell form `$'\r'` passes through a layer that
collapses backslash escapes before the shell sees it, so what ran was an empty
pattern. `grep -c ''` matches every line. This is the same collapse that has
produced a silent `str.replace` no-op, a Python `SyntaxWarning`, a literal TAB
inside a comment and a `git commit -m` that executed a backtick this session --
but every previous instance corrupted *an edit*, and this one corrupted *a
measurement*, which is worse: an edit that goes wrong usually fails to compile,
while a measurement that goes wrong just tells you something false in a
confident tone.

**The discriminator, and it is cheap.** A second measurement by a different
mechanism. Python read the bytes and counted `13`s directly: zero in all three
files. One line, no shell quoting involved, and it settled the question that
two `grep` invocations had agreed on incorrectly.

**The rule worth keeping: when a count comes back exactly equal to the size of
the thing being counted -- every line, every file, every entry -- suspect the
pattern before the data.** A defect that is present in literally 100% of cases
is possible but rare; a pattern that matches everything is the commonest way to
produce that number. The same tell would have caught this in a second: 516 CRs
in a 516-line file is not evidence of CRLF, it is evidence of a predicate that
is always true.

**What it nearly cost.** Rewriting three correct source files and committing
the result, with a commit message explaining a problem that did not exist --
which is the shape already recorded in `TD-C-FOUR-CLAIMS-WALKED-BACK-IN-ONE-SESSION`
and in the `safeio` fix that fixed nothing. The cost is not the wasted edit; it
is that the false explanation gets written down and believed later.
