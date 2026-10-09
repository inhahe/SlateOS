## 1072. One `kill`, three ways to stop a program -- close, terminate, force -- sent the way the program understands

**Date:** 2026-10-09
**Lane:** B
**Decided by:** Operator (answering B-Q22 with a proposal of their own, which
goes past the options the question offered; Claude had recommended option A,
signals only, with the message protocol as a later project). Relayed verbatim
by lane A from the operator's answers file (`open-questions/answers.txt` in
the integration tree). The command-line spelling and the defaults below are
Claude's within that answer (`Claude (operator-approved scope)`), and are
marked as such.

**In short:** there were two programs called `kill`. One sent Linux-style
signals; the other sent a "please shut down" message to a service that was
never written, so it always fell back to ending the program on the spot,
with no chance to save. The operator's answer: one `kill` that does all of
it. The user picks how hard to ask -- *close* (the program may ask "save
first?"), *terminate* (stop now, ask nothing) or *force* (the system ends it)
-- and the system picks the way to deliver it: the SlateOS message to a
program that understands it, a Linux signal to a Linux program. The task
manager offers the same three. And every SlateOS program is to answer the
message, which was in the design all along and never built.

**The operator's answer, verbatim:**

> How about one kill command that does it all, and the user specifies
> whether to politely kill through the IPC mechanism or via the Linux way
> (or force kill), via command-line parameters? And/or the OS could choose
> which polite mode automatically based on wehther the program is a Linux
> program? The same choices should also be available when killing via the
> task manager. I think maybe there should be a third option for killing:
> don't force end the process, but send a signal to the program to stop but
> don't ask the user anything first (such as whether to save a file, etc.)?
> And yes, teach every program to answer the kill message(s) - this should
> already have been done, but I guess the problem was that it was in
> design.txt and not in roadmap-detailed.md...

### The operator's questions, answered

1. **One `kill` that does it all, chosen by command-line options?** Yes. It
   is procps-ng's `kill` -- the one every Linux script was written against,
   ported as the rest of procps has been -- with SlateOS's ways of asking
   added to it. The second `kill` (`userspace/kill`) goes.
2. **Should the system choose the polite route by itself, from whether the
   program is a Linux program?** Yes, and that is the default: a program
   that answers the SlateOS message gets the message, and one that does not
   -- every program ported from Linux -- gets the signal Linux would have
   sent it. An option forces either route.
3. **A third way: stop, without force, but without asking the user
   anything?** Yes. It is what Linux's `SIGTERM` and Windows' end-of-session
   message mean, and it is the step between the other two:

| request | what the program does | Linux equivalent | Windows equivalent |
|---|---|---|---|
| **close** | what closing its window does: it may ask "save first?", and may decline | none for a program with no window (`kill --close` says so); a window's close button, for one with a window | `WM_CLOSE` (Task Manager's "End task") |
| **terminate** | stops promptly: saves what it can, releases what it holds, asks nothing | `SIGTERM` (`kill PID`) | `WM_ENDSESSION` |
| **force** | nothing -- the system ends it | `SIGKILL` (`kill -9 PID`) | `TerminateProcess` ("End process") |

On the guess about why the message was never built: it was in
`roadmap-detailed.md` too (section 1.6, "Structured shutdown via IPC message
(not Unix signals)", unchecked) -- listed, and never picked up, because no
lane's roadmap carried it as a task. It is on lane B's roadmap now, with the
other lanes' parts filed with them.

### What `kill` takes (Claude's spelling, within the answer)

procps-ng 4.0.4's command line, unchanged (`-s`, `-q`, `-l`, `-L`, `-SIGNAL`),
plus options of SlateOS's:

| you type | request | route |
|---|---|---|
| `kill PID` | terminate | the message if the program answers it, else `SIGTERM` |
| `kill --close PID` | close | the message if the program answers it, else its windows' close request (through the compositor); with neither, refused with a sentence saying `kill PID` stops it without asking |
| `kill -9 PID`, `kill -KILL PID`, `kill --force PID` | force | the kernel ends it |
| `kill -s SIG PID`, `kill -SIG PID`, `kill -N PID` | exactly that signal | the Linux way, always -- what a script that names a signal asked for |
| `--via=message` / `--via=signal` | as asked | forces one route; `--via=message` refuses a program that does not answer |
| `--timeout MS REQUEST` | the next request, if the program is still there after MS milliseconds | e.g. `kill --close --timeout 10000 terminate --timeout 5000 force PID`; util-linux's `kill` spells its escalation this way, procps-ng's has none |

`kill PID` is *terminate*, not *close*, because that is what it has meant on
every Unix for fifty years: a script that runs it expects the program gone,
not a dialog waiting on a person who is not there. `pkill` and `killall`
take the same options.

### Who builds what

| part | lane | state |
|---|---|---|
| the kernel half: a lifecycle endpoint each process registers at start (Fuchsia's `PA_LIFECYCLE` shape), found by PID under the same permission rule as a signal | A | offered by lane A 2026-10-09; accepted, request with a draft interface to follow from lane A |
| the userspace half: a small library a program registers and answers with (`on_close` may decline, `on_terminate` may not), and `kill`, `pkill`, `killall` sending it | B | on lane B's roadmap |
| services: `init` and the service manager stop a service with *terminate*, then *force* after its stop timeout | B | on lane B's roadmap |
| every program built on the GUI toolkit answers *close* as its close button does and *terminate* as session end does | C | requested |
| the task manager (`apps/sysmonitor`) offers close, terminate (as "End without asking") and force; it sends `SIGKILL` today | E | requested |
| a Linux program's windows: *close* as the compositor's close request (Wayland's `xdg_toplevel.close`) | F | requested when the toolkit half exists |

Order: procps-ng's `kill` first (it needs nothing from anyone, and fixes the
forced-kill default at once -- `kill PID` becomes `SIGTERM` everywhere), then
the library and the routes as lane A's half lands.

### Where it lives

`userspace/coreutils/src/bin/kill.rs` (to be replaced by the procps-ng port);
`userspace/kill/` (to be deleted, with `killall`'s line in
`scripts/rootfs-bin-manifest.txt`, which is lane D's, until psmisc's
`killall` is ported); `userspace/coreutils/src/pgrep.rs` (`execute_kill`,
`pkill`'s sender); `roadmap.md` (lane B's items); `known-issues/
TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-KILL.md`.

### How to reverse

The operator's call; nothing here is irreversible. Dropping the message
route leaves procps-ng's `kill`, which is option A of the question.
