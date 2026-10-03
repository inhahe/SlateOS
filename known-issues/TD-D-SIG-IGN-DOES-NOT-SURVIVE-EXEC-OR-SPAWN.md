### [D] TD-D-SIG-IGN-DOES-NOT-SURVIVE-EXEC-OR-SPAWN — 2026-09-25 — OPEN

**Status:** OPEN — needs a kernel record of the ignored set (lane A), requested
in `requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`;
the libc half is lane D's and waits on it.

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

**Proper fix:** the kernel holds the ignored set per process, keeps it across
`exec`, copies it on `fork` and spawn, drops an ignored signal at send time, and
auto-reaps children when `SIGCHLD` is ignored; the libc keeps it current from
`signal`/`sigaction` and seeds its table from it at start-up. Details and the
syscall shapes in the request above.
