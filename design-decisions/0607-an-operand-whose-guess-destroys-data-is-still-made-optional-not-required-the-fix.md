## 607. An operand whose guess destroys data is still made *optional*, not required — the fix is refusing the unreadable word, never withdrawing the documented default

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The shell's `splice` commands copy a range of bytes from one file
into another. You may say where in the destination the bytes should land
(`splice copy src dst 0 4096 1024`), and if you don't say, they land at the
start. Until now, a *mistyped* landing spot was treated the same as an omitted
one — so a single wrong character wrote over the beginning of the destination
file and reported success. The obvious hard fix is to stop letting you omit it
at all: always make the user state where the bytes go. That was rejected. The
operand stays optional; what changed is that a word the shell cannot read is
now refused by name instead of being rounded to the default.

### The tension

§600 says no command may substitute a guessed value for a word it could not
read. Thirteen burn-down batches applied that to *selectors* — ids, modes,
switches — where a guess makes the command act on the wrong object and say so.
`splice` is the first case where the guessed number is an address in a file
about to be written, so the guess does not merely mislead: it overwrites bytes
that existed before the command ran, and leaves a success line as the only
trace.

That raises a fair question the earlier batches never had to answer. If the
default is what makes the damage possible, should an operand this dangerous
have a default at all?

### The options

| | *What changes* |
|---|---|
| **A. Optional, refuse unreadable words** (chosen) | `splice copy a b` keeps working and still writes at the start. `splice copy a b 0 1O24 4` prints `` `1O24' is not a destination offset `` and writes nothing. |
| **B. Make the offsets required** | `splice copy a b` stops working and prints `missing destination offset`; every existing script and every line of the built-in help has to be changed. |

### Why A

**The default is not the bug.** Writing at offset 0 when no offset is given is
correct, documented, and the overwhelmingly common case — it is what `cp`
means. The bug is that an unreadable word was *routed into* that default. Once
the routing is fixed, the default is reached only by users who asked for it by
saying nothing, which is exactly the request it answers.

**B fixes the guess by removing the feature.** It would make every safe,
correct, documented invocation fail in order to stop an incorrect one, and the
incorrect one is already stopped by A. That is paying a permanent cost in
usability for a benefit already obtained.

**B also does not generalise.** There are ~560 counted sites left, and a rule
of "operands that can destroy data must be required" needs someone to decide,
per operand, whether it qualifies. `optional_num` needs no such judgement: it
refuses what it cannot read wherever it is used, and the caller only has to
know whether omission is legal. A rule that requires case-by-case judgement to
apply is a rule that will be applied inconsistently across 560 sites.

### The cost of A, stated plainly

A does nothing about a *readable* but wrong offset. `splice copy a b 0 1024 4`
where the user meant `10240` still overwrites four bytes at 1024 and still
reports success, and no amount of parsing strictness can catch that. §600 has
never claimed to; it is about words the shell could not read, not about
requests it read correctly and the user got wrong. Guarding against the latter
is a different feature — a confirmation prompt, or a dry-run flag — and if it
is ever wanted it should be decided as one, not smuggled in as a side effect of
an argument-parsing change.

**Where it lives.** `kernel/src/kshell.rs` `cmd_splice` (nine `optional_num`
calls and `splice_paths`), pinned by `kshell::self_test` rung 81, which asserts
the refusal leaves the destination file byte-for-byte unchanged *and* that the
omitted-offset form still copies to the start.

**Where it came from.** The fourteenth burn-down batch of
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
