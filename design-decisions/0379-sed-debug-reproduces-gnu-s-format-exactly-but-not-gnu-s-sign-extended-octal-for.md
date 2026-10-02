## §379 — `sed --debug` reproduces GNU's format exactly, but not GNU's sign-extended octal for high bytes

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)

**In short:** `sed --debug` prints your script back to you in a tidied-up form
and then narrates what it does to each line. Ours now matches GNU's version
character for character — with one deliberate exception. When the script
contains a byte above 127 (anything outside plain ASCII: an accented letter in
a Latin-1 script, say), GNU prints it as `\o37777777600` — a nine-digit number
that is simply wrong, the result of a C sign bug in GNU's own printer. We print
`\o200`, the three-digit number that is actually the byte. So a `--debug` dump
of a script containing high bytes reads differently from GNU's, and the
differential harness marks that one case as differing on purpose.

### What the option is, and why it had to be copied so closely

`--debug` has two halves. The first is a **program dump**: the parsed script,
written back out in one canonical spelling, under a `SED PROGRAM:` header. It
is not an echo — `\,x,p` comes back as `/x/ p`, `s/a/b/gp3iI` as `s/a/b/igp3`,
`bx` as `b x`, and a block is re-indented two spaces per level. The second is a
**runtime trace**: for every cycle, the input file and line number, the pattern
space, each command *before* its address is tested, the capture registers of a
successful `s`, whichever of the pattern and hold spaces the command changed,
and an `END-OF-CYCLE:` marker that sits *before* the automatic print.

Both halves are output that scripts and humans read, and GNU's shape is full of
details no rule predicts: `r` takes a space before its file name and `w` does
not; `b x` gains a space that `:x` does not; `g` overwrites the pattern space
and then reports the *hold* one; `v` and a comment produce no dump line at all,
not even a blank; the trace does not flush the pending missing newline. Every
one of those was measured against GNU sed 4.9 rather than reasoned about, and
every one is reproduced. `scripts/sed-diff.sh` compares forty-odd `--debug`
invocations byte for byte, which is what keeps it that way.

### The one thing not reproduced

GNU's `debug_print_char` ends in

```c
printf ("\o%03o", ch);
```

where `ch` is a plain `char`. On x86 `char` is signed, so byte `0x80` is the
value `-128`, promoted to `int` as `-128`, and `%o` prints it as an unsigned
32-bit `37777777600`. The `%03o` width does nothing. Every byte from `0x80` up
prints as an eleven-character escape whose value is the byte plus 2^32 − 2^8.

**Alternatives considered.**

| | *What changes* |
|---|---|
| Copy the bug | A `--debug` dump of a Latin-1 or UTF-8 script is unreadable in exactly the way GNU's is, and the harness reports no difference. |
| **Print the byte (chosen)** | The same dump shows `\o200`, `\o303`, `\o251` — the numbers you would find in `od`. One harness case is marked as differing on purpose. |

The case for copying it is the case for every other detail above: `--debug`
output is read by people who know GNU's, and a difference is a surprise. The
case against is that this particular difference is not a *format* — it is
arithmetic that is wrong. A user comparing our dump against `od -b` of their
script finds our number and not GNU's. A user copying an escape out of the dump
back into a script finds that ours round-trips (`\o200` is a byte we accept)
and GNU's does not (`\o37777777600` is not a valid escape for anything). And
the divergence is confined: it can only appear for a byte ≥ 0x80 *inside the
script text*, which almost no script has.

The deciding consideration is that copying it would mean writing code whose
comment has to say "the value here is deliberately the wrong one." The bug is
not load-bearing for anything — no script can depend on the escape, because the
escape does not parse. So it is not copied, and `scripts/sed-diff.sh` carries
one `xfail_stdin` naming this section.

**If GNU ever fixes it**, that `xfail` turns into an `XPASS` and fails the run,
which is how the marker is prevented from outliving its reason.

### A second GNU defect not copied: `e` corrupts its own stored text

Found by the same harness, in the same tranche. GNU's `e COMMAND` hands the
command to `popen`, and before it does it truncates the trailing newline out of
its own stored copy:

```c
if (cmd_length && cur_cmd->x.cmd[cmd_length - 1] == '\n')
  cur_cmd->x.cmd[cmd_length - 1] = 0;
```

The buffer is never restored, and `--debug` prints it by length rather than by
`strlen`. So the *first* cycle dumps `e echo X` followed by a blank line, and
every cycle after it dumps `e echo X\0` — a NUL in the middle of the trace,
put there by the run itself. `sed --debug '1e echo X'` over three lines shows
the change happening between the first cycle and the second.

Not copied, for the same reason and with less argument: the corrupted form is
not a spelling anyone could depend on, it is visibly a defect the moment you
look at the bytes, and reproducing it would mean writing a mutation whose only
effect is to make our own diagnostic output wrong. `scripts/sed-diff.sh` carries
a second `xfail_stdin` for it.
