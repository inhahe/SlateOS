# B → E: the task manager offers three ways to stop a program -- close, end without asking, force -- as `kill` will

**Filed:** 2026-10-09 by lane B. **Addressed to:** lane E (`apps/sysmonitor`).
**Status:** DONE on `lane-e` 2026-10-16 -- both task managers, reply at
the end; it reaches `main` with lane E's next publish. **Priority:**
medium -- until then their one way to stop a program is the forced one.

## In short

The operator decided (B-Q22, design-decisions §1072) that stopping a program
comes in three strengths, and that "the same choices should also be
available when killing via the task manager":

| request | what the program does | Windows' Task Manager |
|---|---|---|
| **close** | what closing its window does: it may ask "save first?", and may decline | "End task" |
| **terminate** -- "end without asking" | stops promptly, saving what it can, asking nothing | (session end) |
| **force** | nothing: the system ends it | "End process" |

`apps/sysmonitor` sends `SIGKILL` for its "kill" (`signal_selected(libcall::SIGKILL, "kill")`)
-- the third row only.

## What is asked

1. **Now, with nothing from anyone:** offer *terminate* as well as *force*:
   `SIGTERM` (`libcall::kill(pid, libcall::SIGTERM)`) for "End", `SIGKILL`
   for "Force end". `SIGTERM` is what *terminate* is for every program that
   does not answer SlateOS's message, which today is every program.
2. **Now that the message exists (2026-10-15):** offer *close*, and send all
   three through `userspace/procstop`, the library `kill`, `pkill` and
   `killall` send them with, which picks the route itself -- SlateOS's
   message to a program that answers it, `SIGTERM` to one that does not:

   ```rust
   // Cargo.toml: procstop = { path = "../../userspace/procstop" }
   use procstop::{Request, Route, Via, deliver, System};
   match deliver(&System, pid, Request::Close, Via::Either) {
       Ok(Route::Message) => { /* asked; it may still decline */ }
       Ok(Route::Signal(_)) => { /* signalled */ }
       Err(why) => { /* why.sentence(), or the errno in Refusal::Errno */ }
   }
   ```

   *Close* to a program that does not answer is refused
   (`Refusal::CannotClose`) -- offer "end without asking" then, as
   Windows offers "End process" when a program does not respond. *Close*
   as the compositor's close request to a Linux program's windows is lane
   F's, still to come. `procstop::escalate` is `kill --timeout`'s "and force
   it if it is still there after N ms", if the manager wants that.
3. The default action is yours to choose. Windows' "End task" is *close*,
   with *force* offered when the program does not respond; that is a good
   model.

## Why

design-decisions §1072 has the whole design; the operator's words are quoted
there. `design.txt` asks that shutdown be done through IPC rather than
signals, and `roadmap-detailed.md` §1.6 listed "Structured shutdown via IPC
message" -- never built until now.

## Lane E -- 2026-10-16: done, in both task managers

`apps/sysmonitor` and `apps/procexplorer` (the second sent `SIGKILL` the
same way) offer all three, through `procstop::deliver` with `Via::Either`
on `procstop::System::SINGLE`:

| | sysmonitor | procexplorer |
|---|---|---|
| **Close** | Delete; the menu | Delete; the toolbar's "End Task"; the menu |
| **End without asking** | Shift+Delete; the menu | Shift+Delete; the menu |
| **Force end** | the menu | the menu |

Close is the default, as you suggested. A program that answers no close
request is sent nothing, and the status line says "cannot be asked to close
-- End Without Asking stops it", as Windows offers "End process". Stop/Pause
and Continue/Resume stay `SIGSTOP` and `SIGCONT`.

The two windows' wording, which each carried a copy of, is one crate now:
`apps/processctl` (the three ways, how each goes through `procstop`, and the
status line for each outcome). Their tests use `procstop`'s `fake` feature:
Delete is a close request, Shift+Delete a terminate, Force End `SIGKILL`.
`escalate` is not used: neither window waits on a process.
