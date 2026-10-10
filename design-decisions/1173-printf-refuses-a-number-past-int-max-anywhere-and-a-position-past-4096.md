## 1173. printf refuses a number past `INT_MAX` wherever it is in a format, and an argument position past 4096

**Date:** 2026-10-05
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a printf format can hold numbers: a field width (`%5d`), a
precision (`%.3s`), and the position of the argument to print (`%2$d`).
Each can be written absurdly large. glibc refuses one past 2,147,483,647 --
`INT_MAX`, the largest `int` -- with "value too large" (`EOVERFLOW`), but
only while it is still in its first, simple pass over the format. Once
something has moved it to its second pass (an argument named by position,
or a conversion it does not recognise), it ignores such a number without
saying so: the field is printed as if the number were not there. SlateOS's
C library refuses the number in both places. It also refuses an argument
position past 4,096, where glibc would try to read that many arguments from
a call that passed far fewer.

| Option | `printf("%1$d %2147483648d", 5)` | `printf("%5000$d", 5)` |
|---|---|---|
| **A. Refuse both** (chosen) | -1, `errno` `EOVERFLOW` | -1, `errno` `EINVAL` |
| B. glibc exactly | `5 5`: the width ignored | reads 4,999 arguments that were never passed (undefined; may crash) |

**Why A.** For the large number: glibc's own positional parser says, where
it reads one, that "overflow is checked for and handled in vfprintf" -- but
that check lives only in the first pass, so the second never makes it. The
same specification in a format without a position fails in glibc too. So A
is what glibc does everywhere its check reaches. It also turns a silently
different output into an error the program can see. No program can rely on
B's behaviour: it changes with whether some other part of the format
happened to switch passes.

For the position: glibc has no limit. It allocates room for as many
arguments as the largest position names and reads them all from the
caller's argument list, past the ones actually passed. This library keeps
the kind of each argument in a table on the stack, so that a positional
format allocates nothing. That table holds `NL_ARGMAX` entries: 4,096,
glibc's own value of that constant. `EINVAL` is POSIX's error for "there
are insufficient arguments", which is what a position past the table always
means in practice.

**What it costs.** Formats that differ from glibc only in how they fail on
a number no real program writes.

**Reversing it.** Each half is a single test in `posix/src/printf.rs`:
`raw.overflow` in `Positional::scan` and `format_specs`, and `NL_ARGMAX` in
`Positional::scan`. The four large-number lines are pinned in
`printf_is_glibcs_for_every_length_modifier`'s `DEVIATIONS`. Copying glibc
would mean ignoring the number in the positional pass: a width read as 0, a
precision as none, a position as the next argument.
