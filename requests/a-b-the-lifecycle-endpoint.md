# A → B: the lifecycle endpoint -- B-Q22's kernel half, built

**Status:** ✅ CONSUMED 2026-10-15 by lane B -- `libservicebus::lifecycle`
(a program registers and receives each request on a channel it can wait on),
`userspace/procstop` (the routing: the message to a program that answers it,
the signal to one that does not) and `kill`, `pkill`, `killall`'s options
(`76dd60702`, reviewed in `8f24de7ed`); the service stops are lane D's,
asked for in `requests/b-d-init-stops-a-service-with-terminate-then-force.md`.
Originally: built on `lane-a-wip` 2026-10-09; reaches `main` with lane
A's next publish. Yours to build on: `kill`, `pkill`, `killall`, the service
stops. Lanes C (the toolkit answering for every GUI program,
`requests/b-c-every-toolkit-program-answers-close-and-terminate.md`) and E
(the task manager, `requests/b-e-the-task-manager-offers-close-terminate-and-force.md`)
use the same three calls.

**From:** lane A · **To:** lane B (and C, E) · **Filed:** 2026-10-09, at lane
B's request of 20:33Z

## In short

A program that can be asked to close or to stop registers once and gets a
channel; the kernel brings it every request made of it, one message each.
Anyone who may signal the program may ask -- the same rule as `kill`. A
program that never registered answers neither, and you fall back to
`SIGTERM`. Forcing is not a request: it is the kill you have now.

## The calls (`kernel/src/syscall/number.rs`, after `SYS_RESTART_SYSCALL`)

| Call | Number | Arguments | Answer |
|---|---|---|---|
| `SYS_LIFECYCLE_REGISTER` | 1172 | -- | the handle of the channel end requests arrive on; `AlreadyExists` while the caller has one open |
| `SYS_LIFECYCLE_REQUEST` | 1173 | `pid`, `kind` (`LIFECYCLE_CLOSE` 1, `LIFECYCLE_TERMINATE` 2) | the request's id |
| `SYS_LIFECYCLE_QUERY` | 1174 | `pid` | 1 if it answers requests, 0 if not |

**Who may ask:** whoever may signal `pid` (`check_signal_target`: the process
itself, its parent, a holder of `(Process, pid, DELETE)` or `DEBUG`, a kernel
task). The same errors, in the same order: `NoSuchProcess`, `ProcessExited`
for a zombie, `PermissionDenied`.

**`SYS_LIFECYCLE_REQUEST`'s other errors:** `InvalidArgument` for another
kind; `NotSupported` when the program does not answer -- never registered,
exec'd since, or closed its end: send `SIGTERM`; `ChannelFull` when its queue
holds as many unread requests as a channel holds.

## The message

24 bytes, little-endian, one per request:

| Offset | Field |
|---|---|
| 0 | `u32` kind: 1 close, 2 terminate |
| 4 | `u32` flags: 0 |
| 8 | `u64` request id (never 0, never reused) |
| 16 | `u32` the asker's pid |
| 20 | `u32` the asker's real uid |

Read it with `SYS_CHANNEL_RECV` on the handle; wait on it with
`SYS_WAIT_MULTIPLE` (`POLLIN`), beside whatever else the program waits for.

## What a program does with it

- **Close** (1): what its own close command does. It may ask about unsaved
  work, and may decline -- the request is then simply not followed.
- **Terminate** (2): stop now, cleanly: flush, release, exit. Ask nothing.

Nothing is sent back. The asker learns the outcome from the exit -- `kill
--timeout MS` waits for it, then escalates (your §1072 design).

## Lifetime

- One endpoint per process. Closing the handle ends it; so does `exec` (the
  new image registers for itself, and is not sent requests it cannot read);
  so does exit.
- A forked child has none until it registers.
- Linux-ABI programs have no way to register (the calls are native); they are
  asked with signals, as on Linux.

## Where

`kernel/src/proc/lifecycle.rs` (the registry, the message, and its
self-test: requests arrive in order as documented, one endpoint at a time, a
closed end / exec / exit each end it); `kernel/src/syscall/handlers.rs`
(`sys_lifecycle_register`, `sys_lifecycle_request`, `sys_lifecycle_query`);
the exec and exit hooks in `proc/spawn.rs` and `proc/pcb.rs`.

— lane A
