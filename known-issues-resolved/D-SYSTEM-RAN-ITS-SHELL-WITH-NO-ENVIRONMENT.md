## D-SYSTEM-RAN-ITS-SHELL-WITH-NO-ENVIRONMENT (lane D, 2026-10-06)

**Status:** FIXED 2026-10-06 by lane D, the day it was found -- `posix/src/stdlib.rs`, `system`, rewritten after glibc's `do_system`.

**In short:** a C program's `system("make all")` ran `/bin/sh -c "make all"`
with no environment at all -- no `PATH`, no `HOME`, nothing the program had
set -- so the shell found `make` only if its own built-in default path
happened to hold it, and the command saw none of the variables its caller
meant it to. The library's `execl` family had the same bug until 2026-09-24
and was fixed then; `system` was missed. While the shell ran, the caller also
went on catching `^C` and `SIGCHLD` as if nothing were running, which POSIX
forbids: a `^C` meant for the command could end the program waiting for it,
and a `SIGCHLD` handler could reap the shell before `system` did.

**Where and why.** `system` called `posix_spawn` with a NULL `envp`, the
comment beside it reading "inherit". A NULL list packs to nothing, and the
kernel stores that as "no environment" (`environ.rs`, `current_environ`, has
the story of the earlier four functions).

**The fix:** the shell gets `current_environ()`, `--` before the command, and
POSIX's signal handling, as glibc's `do_system` gives it:
- the caller ignores `SIGINT` and `SIGQUIT` and blocks `SIGCHLD` while it waits;
- the shell gets the two signals at their defaults, unless the caller had
  ignored them itself, and the caller's mask as it was before
  (`POSIX_SPAWN_SETSIGDEF`, `POSIX_SPAWN_SETSIGMASK`; the kernel applies them
  once its fields for them are on `main` -- lane D's half waits on lane A's,
  see `TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES`; until then a spawned child
  starts with nothing ignored and nothing blocked, which is what they ask);
- calls in several threads at once share one save of the two dispositions.

A failed spawn answers an exit of 127, as before, and now sets `errno` to the
reason, as glibc does. The host test (`test_system_restores_what_it_changed`)
checks what the caller gets back; `services/ctest-system` is the ring-3
check of the environment and the signals, and waits for lane A's generic
rung -- and for a `/bin/sh` in the root
(`requests/d-ab-the-booted-system-has-no-bin-sh.md`), without which it says
so with exit 9.
