## 1120. A NULL `FILE *` is no stream: `EBADF`, not stdin and not a fault

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** C's `stdin` used to be a NULL pointer here, so a NULL `FILE *`
handed to any stdio function meant standard input. `stdin` is now a real
pointer (known-issues.md, `D-POSIX-STDIN-WAS-A-NULL-POINTER`), which leaves
the question of what NULL should mean. The answer chosen: nothing -- the call
fails the way it fails on a bad stream, with `errno` set to `EBADF`.

| Option | What a program sees from `fgets(buf, n, NULL)` | For | Against |
|---|---|---|---|
| (a) **fail, `EBADF` (chosen)** | NULL, `errno` `EBADF` | a bug in the program is reported where it happens, and nothing reads through the pointer | glibc faults; a program is told something glibc never tells it |
| (b) fault, as glibc does | the process dies | exact | a deliberate crash in a library that otherwise reports errors, for no caller's benefit |
| (c) NULL means stdin, as before | reads the terminal | old programs keep working | it is what made `stdin == NULL` true, and CPython's REPL read nothing |

Two entry points answer NULL differently, by the standard's own rules:
`fflush(NULL)` flushes every stream (C says so), and `ferror(NULL)` answers
nonzero, so a loop of the shape `while (!feof(f) && !ferror(f))` ends.
