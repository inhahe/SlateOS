### [D] TD-D-SIG-IGN-DOES-NOT-SURVIVE-EXEC-OR-SPAWN — 2026-09-25 — OPEN

**Status:** OPEN — both halves are written: lane A's record of the ignored set
(`SYS_SIGNAL_SET_IGNORED`/`GET_IGNORED`, 1098/1099, on `lane-a`) and lane D's
report and seed on it (2026-10-07). It closes when lane A's kernel is on `main`
and `services/ctest-sigign-exec` passes in a lane D boot. `posix_spawn`'s
`POSIX_SPAWN_SETSIGDEF`, which should set some of the set back to default in
the child, is still `TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES`.

**In short:** a program that tells the system to ignore a signal and then runs
another program should pass that on: `nohup cmd` ignores the hang-up signal so
that `cmd` survives the terminal closing, and shells ignore `^C` for background
jobs the same way. On SlateOS the next program always starts with every signal
back at its default, so `cmd` dies on hang-up anyway and a background job dies
on `^C`. Separately, `SIGCHLD` set to "ignore" is supposed to stop dead children
lingering as zombies, and cannot, because only the kernel could do that.

**Where:** `posix/src/signal.rs` — the dispositions are a static table in the
libc, consulted by `dispatch_self_signal` after the kernel has delivered every
catchable signal to the trampoline. A new image's table starts all-default, so
`SIG_IGN` never crosses `exec` or `posix_spawn` (`fork` copies the table with
the address space, so it is fine there).

**Reproduce (by reading; no rung exercises it):** `signal(SIGHUP, SIG_IGN);
execvp("sleep", ...)`, then send `SIGHUP` to the `sleep` — it dies, where Linux
keeps it alive.

**Lane D's half, done 2026-10-07:** `posix/src/signal.rs`: every change to a
disposition -- `sigaction`, `signal` and its kin, `SA_RESETHAND`'s reset --
reports the whole ignored set, with `SIGCHLD`'s `SA_NOCLDWAIT` as its flag,
to the kernel (`ignored_changed`), again for as long as the table changes
while it is reported, as the blocked mask is written; and start-up seeds the
table from the kernel's set (`seed_inherited_ignored`), so a program `nohup`
started finds `SIGHUP` at `SIG_IGN`. A kernel without the calls says so once,
and the table alone works as before. `services/ctest-sigign-exec` checks the
two halves together -- `SIGHUP` ignored across an `exec` and survived, a
handler gone default, an ignored `SIGCHLD` leaving no zombie -- and passes on
Linux.

**What was asked of lane A:** the kernel holds the ignored set per process, keeps it across
`exec`, copies it on `fork` and spawn, drops an ignored signal at send time, and
auto-reaps children when `SIGCHLD` is ignored; the libc keeps it current from
`signal`/`sigaction` and seeds its table from it at start-up. Details and the
syscall shapes in the request above.
