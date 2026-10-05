## 632. kshell's `printf` complains after the data, not where the fault is, because kshell has exactly one output stream

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A)
**Lane:** A

**In short:** When you mistype a number — `printf '%d-%d' 1 abc` — the shell
now says so. The question was *where in the output* the complaint goes. On
Linux the data goes to one place ("standard output", the stream a `$(...)`
captures) and complaints go to another ("standard error", which is *not*
captured), so a complaint can appear at the exact moment the fault is noticed
without ever getting mixed into the data. The kernel shell has only one place
to write, so a complaint printed at the moment of the fault would land *inside*
the data — `1-`, then the sentence, then `0`. The decision is to hold the
complaints until the whole line has been printed and put them after it, so the
data stays in one piece.

**The constraint is real and is recorded elsewhere.** kshell has
`shell_println!` (goes to whichever of the capture buffer or the console is
active) and `crate::console_println!` (always the console, never captured), and
no `shell_eprintln!` — see `known-issues.md`,
`A-KSHELL-2334-COMMANDS-PRINTED-PAST-THE-CAPTURE`, where the conclusion was
that the split between the two macros was never a stdout/stderr distinction but
drift, and that terminal interaction (prompts, `read`, xtrace) is what
legitimately bypasses the capture. A diagnostic is *not* terminal interaction:
kshell's self-test §17 deliberately asserts that command diagnostics appear in
`$(...)` captures. So `printf`'s complaint must go through `shell_println!`,
into the same stream as its data, and the only freedom left is ordering.

**The options.**

| | *What changes:* |
|---|---|
| **A. Complain where the fault is found** | `printf '%d-%d' 1 abc` prints `1-`, then `printf: \`abc': expected a numeric value`, then `0`. A captured result contains a sentence wedged into the middle of the value. |
| **B (chosen). Collect and complain after the format** | The same command prints `1-0`, then the sentence on its own line. The data is contiguous; a capture that greps for the value still finds it. |
| **C. Complain on the console only** | The sentence never appears in a capture — so a script can never see why it failed, and the self-test could not assert it either. Rejected: it recreates exactly the bug `A-KSHELL-2334` was filed about, from the other direction. |

**Why B and not A.** Neither is what GNU does, because GNU is not forced to
choose — its two streams are interleaved on a terminal and separated in a
capture, so it gets both properties. With one stream the properties are in
conflict and the data's integrity is worth more than the complaint's position.
A caller who runs `x=$(printf '%d' "$n")` and gets a value with an English
sentence in the middle of it has a corrupted value; one who gets the value
followed by a sentence has a value with a suffix, and a non-zero exit status
telling them to look. The cost of B is real and is accepted: with a reused
format (`printf '%d\n' a b c`) all the complaints arrive together at the end,
so the reader must match them to their operands by the quoted word rather than
by position. The word is in every sentence for that reason.

**A smaller decision recorded in the same place: the quotation marks diverge
from lane B's `printf` and the sentences do not.** The three sentences —
`expected a numeric value`, `value not completely converted`, `numerical result
out of range` — are lane B's `userspace/coreutils/src/bin/printf.rs` wording
byte for byte, because a script that greps one shell's diagnostic must find the
other's. The quoting is not: kshell uses `` `word' `` where coreutils uses the
locale's quotes, because a reader of kshell's output sees this file's other
diagnostics beside it and a single command quoting differently from its
neighbours reads as a bug. Sentence and exit status are the contract; the
punctuation around the operand is house style.

**How to reverse:** if kshell ever grows a real second stream, option A becomes
available at no cost and is then the better one — the `faults` vector in
`cmd_printf` exists only to defer the printing, so the reversal is to print at
the `printf_note_fault` call site and delete the vector.

**Where it is:** `cmd_printf` and `printf_note_fault` in `kernel/src/kshell.rs`;
pinned by self-test rung 100, which asserts the exact bytes
`0\nprintf: \`zzabc': expected a numeric value\n` so that the ordering cannot
be changed without the assertion failing.
