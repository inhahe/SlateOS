## 1027. Quote a file name in a diagnostic only when it needs it

**Date:** 2026-09-16
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `bc` could not open a file and said `File 'nosuch.bc' is
unavailable.` where GNU says `File nosuch.bc is unavailable.` — the same
sentence with the name in quotes. It now prints the name bare when the name is
ordinary, and still quotes it when the name contains a space, a newline or a
control character.

**Why quoting was there at all, and why that reason survives.** It is not
tidiness: a file called `x⏎bc: /etc/shadow: Permission denied` prints, on a
program that quotes nothing, as two lines, the second of which looks like a
diagnostic `bc` never wrote. Quoting is what stops a file name forging output.
The eliding form (`quotef_os`) keeps that exactly where it matters — a name
with a newline is still quoted and escaped — and drops it only for names that
cannot forge anything. So this is not a trade of safety for compatibility; the
forgery argument was never an argument for quoting *clean* names.

**What actually settled it** was neither compatibility nor safety but
self-consistency. Since the syntax-error prefix started naming its source file
(§1023's plumbing, added the same day), `bc` printed the same file name two
different ways in the same program:

    File 'prog.bc' is unavailable.
    prog.bc 1: syntax error

One program spelling one file two ways is worse than either convention
consistently applied, and of the two, the eliding form is the one that also
matches upstream.

**The alternatives** were recorded in the tracker entry as (a) print bare
always, (b) keep quoting always, (c) elide. (a) loses the forgery protection
outright. (b) keeps the divergence and the inconsistency. (c) was the entry's
own recommendation and is what dominates: the common case matches GNU byte for
byte, the dangerous case stays protected, and `bc` agrees with itself.

**Why this was decided here rather than asked.** The entry said it was
"written down rather than quietly changed" because it is a user-visible
message, and that caution was right when it was written. Two things changed:
the inconsistency above is new information the entry did not have, and it makes
one option strictly better rather than leaving a genuine fork; and the
operator's queue already holds nine unanswered lane-B questions, where adding a
tenth about message punctuation would push the ones that matter further down.
It is one function call to reverse.

**Verified both ways**, since a change justified by a safety property should
demonstrate the property rather than assert it: `nosuch.bc` prints bare and
matches GNU, and a name containing a newline prints as `'a'$'\n''bc: forged'`
— quoted and escaped, forging nothing. Both are harness rows, the second
marked `differs_by_design`, because a deviation nothing exercises is one nobody
will notice losing.
