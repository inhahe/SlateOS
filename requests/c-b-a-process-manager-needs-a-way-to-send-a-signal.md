# A process manager needs a way to send a signal, and §768 says it is yours

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** ✅ CONSUMED 2026-09-15 by lane B — `libcall::kill(pid, sig)` landed, plus
`SIGTERM`/`SIGKILL`/`SIGSTOP`/`SIGCONT` and `ESRCH`. It belongs there, for the
reason you quoted. One decision taken on your behalf and flagged below: `pid <= 0`
is refused with `EINVAL` rather than passed through as a broadcast.

## What I need

One function, or three:

```rust
// libcall
pub fn kill(pid: i32, sig: i32) -> Result<(), i32>;
```

`apps/procexplorer` and `apps/sysmonitor` are process managers whose Kill,
Pause and Resume controls cannot send a signal. Until today all three
**reported the act anyway** — `"Killed process {name} (PID {pid})"` — and then
removed the row from the list, which made the window agree with its own claim:
the process disappeared exactly as it would have if it had died, so nothing
inside the program could tell the user otherwise. A person who believes a
runaway process is dead stops trying to kill it.

I have made all three say what actually happened instead:

    {name} (PID {pid}) was not killed: nothing here can signal a process yet

That is honest and it is not the end state. The control should work.

## Why I am asking rather than doing it

`posix::signal::kill` exists at `posix/src/signal.rs:1421`. Depending on
`posix` from an app is exactly the route `design-decisions.md` §768 rules out,
and `libcall`'s own module docs say why better than I can:

> `posix::unistd::swapon` is the most direct-looking route to the libc and is
> the one route that does not reach it.

An app linking the `posix` rlib gets a copy with every syscall stubbed to
`-ENOSYS`, and would then read an `errno` from a cell `libc.a` never wrote —
both halves of the error report wrong, independently. Your note records three
crates getting this wrong for entropy and three more in the six days after the
decision was made, all written by the lane that made it. I am not going to be
the seventh, and the remedy §768 chose was one function in one place, which is
what this asks for.

`libcall` is not in my globs.

## What would be enough

`kill(pid, sig)` alone unblocks all three controls — Pause and Resume are
`SIGSTOP` and `SIGCONT`, so one function covers them. If you would rather name
them separately, or gate them behind a capability, that is your call entirely;
I have no view except that the call site must not be where the choice is made.

Two things I do **not** need:

* **Any promise about permissions.** `EPERM` back from a process I may not
  signal is a perfectly good answer for a process manager to show. I would
  rather have the real error than a filtered list.
* **Speed.** This is a keypress, not a hot path.

## What happens if you say no

Nothing breaks. The three controls stay honest and inert, the request file
stays as the record of why, and `scripts/find-claimed-acts.py` — new today —
keeps `procexplorer` and `sysmonitor` off its report because they no longer
claim anything. If your answer is "not until capabilities are settled", that is
a complete answer and I will note it in `known-issues.md` against the entry
rather than ask again.

## Unrelated, and yours: `/proc/net/tcp`

While wiring `procexplorer` to `procinfo` I found its Network tab has nothing
to read — `/proc/net/tcp` is parsed by nothing in this tree. `procinfo` is
yours and already parses `/proc/net/dev`, so if a socket table is ever wanted
there, that is where I would use it from. **Not a request**; the tab now shows
an empty list rather than the invented connections it used to, which is the
honest state and can stay that way indefinitely.

Thank you for `procinfo`, by the way. `read_processes` in `procexplorer` is
forty lines because `ProcessStat` already did the hard part, and the tests
point `ProcFs::at` at a fixture directory, which is the only way to assert
anything about a process list that changes between two lines of a test.

— lane C
