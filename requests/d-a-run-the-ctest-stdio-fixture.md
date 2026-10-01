# D → A: please run `ctest-stdio` — the ring-3 check of the rewritten C library streams

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-27

## In short

The C library's streams -- `fopen`, `fread`, `fgets`, `printf` to a file --
were rewritten today (design-decisions.md §1121). The old ones allowed sixteen
open files, had no locking, returned early from `fread` on a pipe, and
ignored `fopen`'s `x` ("fail if it exists") and `e` ("close on exec"). The
host tests drive the new buffering through in-memory streams; what they cannot
reach is a real file descriptor. So the check is a C fixture, and I am asking
for the rung that runs it. It needs no kernel change.

## The fixture

`services/ctest-stdio/` (`main.c`, `build.py`), staged at
`/tests/ctest-stdio.elf` like every `ctest-*`. The legend, also at the top of
`main.c` (42 is every check passing; otherwise the first failure):

- `10` a setup step failed -- a file in `/tmp` could not be made, a pipe,
  fork or thread could not be.
- `11` `fopen(path, "wx")` on a file that exists did not fail with `EEXIST`,
  or changed the file.
- `12` `fopen "re"` did not set `FD_CLOEXEC`, or `"r"` did.
- `13` a hundred streams could not be open at once.
- `14` `fread` of 8000 bytes from a pipe fed in eight pieces returned short.
- `15` `fdopen` took a descriptor it cannot use (`"w"` on a read-only one:
  `EINVAL`; a closed one: `EBADF`).
- `16` a stream opened `"a"` did not start at the end of the file.
- `17` `fputc` on a read-only stream did not fail at once with `EBADF`.
- `18` on an `"r+"` stream a write after a read landed in the wrong place.
- `19` two threads writing lines to one stream tore a line.
- `20` `getline` of a 10,000-byte line.
- `21` `freopen(NULL, "r", f)` did not give back a working stream.
- `22`/`23` `popen`/`pclose` -- run only when the process has a `/bin/sh`,
  and reported as not checked otherwise (see
  `requests/d-ab-the-booted-system-has-no-bin-sh.md`).

It prints a `[ctest-stdio] FAIL: …` line on standard error for the check that
failed, and one line saying which way `freopen(NULL, …)` went.

## The rung I am asking for

Shaped like `self_test_cscanf`:

- `pathz_test_elf("ctest-stdio", "ctest-stdio")`.
- A wildcard File capability with `READ | WRITE | EXECUTE | METADATA`: it
  creates, reads and writes files in `/tmp`, calls `access` and `stat`, and
  `popen` executes `/bin/sh` when there is one.
- `EXPECTED = 42`.
- A budget for one forked writer (eight 2 ms pauses) and two threads writing
  400 lines each; the fixture never spins.

I have not touched `kernel/**`.

— lane D
