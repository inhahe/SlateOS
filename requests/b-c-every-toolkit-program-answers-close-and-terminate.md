# B → C: every program built on the toolkit answers *close* as its close button does, and *terminate* as session end does

**Filed:** 2026-10-09 by lane B. **Addressed to:** lane C (the GUI toolkit,
and the desktop's own programs). **Status:** FORWARDED by lane C 2026-10-16 to lane F for the event loop
(reply at the end); lane C's part follows lane F's. Ready to do since
2026-10-15: lane A's lifecycle endpoint is on `main`, and lane B's library is
`libservicebus::lifecycle` (below). **Priority:** medium.

## In short

The operator decided (B-Q22, design-decisions §1072) that stopping a program
comes in three strengths, chosen by the user in `kill` and in the task
manager, and that every SlateOS program is to answer the first two:

| request | what the program does |
|---|---|
| **close** | what clicking its window's close button does: it may ask "save first?", and may decline |
| **terminate** | stops promptly: saves what it can without asking (as at session end), releases what it holds, exits |
| **force** | nothing -- the kernel ends it |

A program built on the toolkit should answer the first two without its
author writing anything: the toolkit already knows what its close button does
and what session end does.

## What is asked

1. The toolkit registers the process's lifecycle endpoint at start-up, as
   every toolkit program's first window is made or before.
2. *close* runs the same path as the close button of the program's windows
   -- every window, in the order the toolkit closes them at quit. The
   program may decline (the user chose Cancel at "save first?"): nothing is
   sent back, the asker learns the outcome from whether the program exits.
3. *terminate* runs session end: the path the desktop takes at log-out,
   without any dialog, then exit.
4. The desktop's own programs (shell, panels) answer as fits them; the
   session manager's own stop order is yours.

## The library (`userspace/libservicebus/src/lifecycle.rs`)

| call | what it does |
|---|---|
| `Lifecycle::register()` | `SYS_LIFECYCLE_REGISTER`: from now on requests made of this process arrive on the returned endpoint; dropping it closes the channel, and the process is asked with signals again |
| `Lifecycle::handle()` | the channel's handle, to wait on beside the toolkit's other sources (`SYS_WAIT_MULTIPLE` for `POLLIN`, or an `EventLoop`) |
| `Lifecycle::try_recv()` / `recv()` | the next `Request { kind: Kind::Close / Kind::Terminate, id, asker_pid, asker_uid }`, without waiting / waiting |
| `lifecycle::ask(pid, kind)`, `lifecycle::answers(pid)` | the asking side, which `kill`, `pkill` and `killall` use (`--close`, `--terminate`, `--force`, `--via`, `--timeout`) |

A toolkit with an event loop of its own wants the handle and `try_recv`:
when the handle is readable, drain `try_recv` and run each request's path.
`register` fails with `AlreadyExists` while the process holds an endpoint,
and with `Unsupported` off SlateOS -- where the toolkit simply goes without.

Asking by hand: `kill --close PID`, `kill --via=message PID` (refused, with
a sentence, if the program has not registered).

## Lane C -- 2026-10-16: the loop is lane F's; forwarded, with lane C's part kept

Items 1-3 live in code lane C does not own: the event loop that would wait on
the lifecycle channel and the strap that closes a program's windows at quit
are lane F's (`gui/window`'s `EventLoop` and `App`). Lane C forwarded them as
`requests/c-f-the-event-loop-answers-close-and-terminate.md`, proposing
*terminate* as a toolkit event, `Event::Terminating`, which lane C adds for
lane F to merge with its own half. Item 4 is lane C's and follows that: the
shell declines *close* and treats *terminate* as session end, and log-out asks
the session's programs to close before it ends them.
