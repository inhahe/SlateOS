## TD-B-GUARDED-PROGRAMS-OPEN-FILES-WITHOUT-OPEN-SAFER (lane B, 2026-10-07)

**Status:** OPEN (lane B). Nothing is known to fail today; this is a
structural guarantee upstream has and we do not.

**In short:** a program can be started with its standard output or error
closed (`cp -v a b >&-`). The descriptor guard (`guard_std_fds!`) keeps them
closed, as GNU's programs see them, so that `write error` is reported where
GNU reports it. But then the next file the program opens takes the lowest
free descriptor -- 1, or 2 -- and anything written to "standard output" while
that file is open goes *into the file*. GNU cannot do this: gnulib's
`fcntl-safer` module makes every `open` in coreutils `open_safer`, which never
returns 0, 1 or 2. Ours opens with plain `OpenOptions`, so the only thing
keeping a `-v` line or a diagnostic out of a user's file is the order in which
the program happens to write and open.

**Where.** Every guarded program that opens a file and also writes to standard
output or error while it is open. Measured 2026-10-07 for the case that looked
most exposed, `cp` (whose copies all go through `coreutils::copy`):

* `cp -v` over 300 files with standard output closed -- 15 KB of `-v` lines,
  so `Stream`'s 4096-byte buffer is flushed many times -- leaves every copy
  intact and ends `write error: Bad file descriptor`, status 1, as GNU's does.
  It is safe because `emit_verbose`'s line is written before each copy opens
  its source and destination (GNU's order, `copy.c:2630`), never while one is
  open.
* `cp -p`/`--preserve=ownership` with standard error closed: also intact, but
  only because a non-root ownership failure is not reported at all, here or
  upstream. A preservation failure that *is* reported -- printed at the site,
  with the destination still open -- would go into the destination. None was
  found that an unprivileged harness can provoke.

So the hazard is latent: one future line of output moved inside an open file's
lifetime, in any of `cp`, `mv`, `install`, `ln`, `tac`, `split`, `sort -o`,
`tee`, would silently write into user data, and only when a descriptor was
closed -- exactly the run nobody looks at.

**The proper fix:** upstream's. A `coreutils::stdfd::open_safer` (an open
followed by `fd_safer`, which already exists and which `tac` already uses for
its temporary file) and every file-opening helper in the crate --
`copy::open_new`, `copy::open_truncating`, the source open in `copy_reg`,
`dirfd`'s opens, `stdio`'s -- routed through it, with a unit test per helper
that opens with descriptors 0-2 closed in a child process and checks the
descriptor it got. A ratchet like `check-raced-globals.py` could then refuse
a new `OpenOptions::open` in `src/bin/` that bypasses it.
