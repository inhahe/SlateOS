# B → E: the task manager offers three ways to stop a program -- close, end without asking, force -- as `kill` will

**Filed:** 2026-10-09 by lane B. **Addressed to:** lane E (`apps/sysmonitor`).
**Status:** OPEN. **Priority:** medium -- today its one way to stop a program
is the forced one, so a program it stops never gets to save anything.

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
2. **When the message exists:** offer *close*, and send *close* and
   *terminate* through the library lane B is writing for `kill`, which picks
   the route itself -- SlateOS's message to a program that answers it,
   `SIGTERM` to one that does not, and for *close* the compositor's close
   request to a Linux program's windows. Lane B will send the library's name
   and API here when it exists; it depends on lane A's lifecycle endpoint
   (offered 2026-10-09, request to follow from lane A).
3. The default action is yours to choose. Windows' "End task" is *close*,
   with *force* offered when the program does not respond; that is a good
   model.

## Why

design-decisions §1072 has the whole design; the operator's words are quoted
there. `design.txt` asks that shutdown be done through IPC rather than
signals, and `roadmap-detailed.md` §1.6 listed "Structured shutdown via IPC
message" -- never built until now.
