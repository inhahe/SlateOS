## D-POSIX-STDIN-WAS-A-NULL-POINTER — C's `stdin` was NULL, and CPython's REPL took it for a string (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** the C library's `stdin`, `stdout` and `stderr` were the numbers
0, 1 and 2 standing in for the real stream objects -- which made `stdin` a
NULL pointer. CPython's tokenizer takes a NULL file to mean "the input is a
string", so its interactive prompt never read the terminal: it parsed
leftover heap memory as Python source, printed five SyntaxErrors and exited.
That was `ctest-python-repl`'s exit 4.

**The chain**, read from the serial log of lane-d `c721b2a13`, the
fixture's new diagnostics, and CPython 3.12.3's `Parser/tokenizer.c`:

1. The fixture typed `print(6*7)`; the pty echoed it; no `>>> ` prompt ever
   appeared.
2. The interpreter reported five lines of three bytes -- `\x80\x1b0`,
   `@\x973`, `P\x9b3`, `\xc0C3`, `\xd0G3` -- each a little-endian pointer
   into libc malloc's regions (0x6000301b80, ...), cut at the pointer's zero
   byte: dlmalloc's free-list link, left in a recycled block.
3. `tok_nextc` dispatches `else if (tok->fp == NULL) rc =
   tok_underflow_string(tok);`. With `stdin` NULL, each round's tokenizer
   read its fresh, uninitialised `PyMem_Malloc(BUFSIZ)` buffer as a string --
   never calling `PyOS_Readline`, so no prompt and no read -- until a block
   began with a zero byte, which is end of input. Exit 0.

**Fixed:** the three symbols (and glibc's `_IO_std*_` aliases) hold the
statics' addresses; the library passes the same pointers; `fdopen(0..2)`,
which returned the sentinel -- NULL for `fdopen(0, "r")`, i.e. failure --
returns the stream; `getwchar`, which passed NULL on purpose, passes stdin.
A NULL `FILE *` is now no stream (design-decisions.md §1120).

**How long it hid.** Every stdio call mapped the NULL back to stdin, so C code
that only *passed* `stdin` worked; only code that *compared* it with NULL
broke, and nothing in the tree's own tests or fixtures does. The ring-3 C
fixtures write with `write()`, not stdio. The probe added to
`ctest-python-repl` on 2026-09-27 (CPython's own sequence of stdio calls, in
a child on a pty) would not have caught it either -- it is the comparison,
not the calls, that fails.
