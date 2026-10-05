## B-LOGIND-IMPLEMENTS-THE-WRITE-SIDE-AND-EXPOSES-NONE-OF-IT — MOSTLY CLOSED 2026-09-13

**Lane:** B. **Severity:** medium — nothing is wrong with what runs; what runs
can never do anything.

**What it is.** `userspace/logind` implements session, seat and inhibitor
management in full, and no message can reach any of it. `bus::dispatch` offers
eight methods and they are all read-or-modify: `ListSessions`, `GetSession`,
`LockSession`, `UnlockSession`, `ForceUnlockSession`, `TerminateSession`,
`AuthenticateSession`, `SetIdleHint`. Anything else answers
`ERR_UNKNOWN_METHOD`. So these are implemented, tested, and unreachable:

| Method | What cannot happen |
|---|---|
| `Daemon::create_session` | no session can ever exist |
| `Daemon::allocate_session_id` | — |
| `Daemon::add_inhibitor`, `remove_inhibitors_by_pid`, `is_inhibited_any` | nothing can hold a sleep/shutdown lock |
| `Daemon::switch_vt` | no VT switch |
| `Daemon::create_seat`, `remove_seat` | one hardcoded seat, forever |
| `Daemon::update_idle_state` | the idle timer never advances |

The consequence is that every query answers truthfully about a world that is
permanently empty: `ListSessions` returns nothing because there is nothing,
not because nobody is logged in.

**And nothing outside would call it either.** `userspace/login` and
`userspace/getty` contain no reference to logind — not a stub, not a TODO, no
`SERVICE_NAME`. On a real system those are precisely the programs that
register a session. So the gap is two-sided: a method that is not exposed, and
callers that do not know the daemon exists.

**How it surfaced.** Not by reading logind. It fell out of removing the
crate-level `#![allow(dead_code)]`: 22 items, and once grouped they were one
thing. The blanket allow had made a whole unreachable subsystem look like
scattered lint noise.

**Closed for sessions, 2026-09-13, and not for the rest.**

`CreateSession` is on the bus (administrator-only, checked before any
argument is decoded), and `login` calls it after authentication and calls
`TerminateSession` when the shell exits. `getty` needed no change: it
builds a `login` command and hands off, including for autologin, so
`login` is the one process on every path.

**What has NOT been demonstrated, and this is the part to read.** The
tests prove the *failure* path: on any target that is not `slateos` the
bus syscalls return ENOSYS, so `connect` fails, the caller is told on
stderr, and the login proceeds. They do not prove the success path,
because `logind::serve` is `#[cfg(unix)]` and this host cannot boot the
target. **No session has ever actually appeared in `loginctl`.** A green
suite here means "login survives logind being unreachable", not "session
registration works", and the difference is exactly the kind a passing
test invites a reader to skip over. Confirming it needs a boot test,
which is lane A's.

**Still open:** the inhibitor, VT-switch and seat methods
(`add_inhibitor`, `remove_inhibitors_by_pid`, `is_inhibited_any`,
`switch_vt`, `create_seat`, `remove_seat`) remain implemented and
unreachable. They are marked in the source with the reason. Nothing calls
them because nothing has needed to yet, which is a weaker justification
than sessions had and is why they are listed rather than done.

**What the original fix was.** Expose the write side and give it callers:

1. `CreateSession` on the bus, with `bus::authorize` deciding who may — that
   function exists and already has the `Required` vocabulary for it.
2. `login` and `getty` call it after authentication, and release on exit.
3. `AddInhibitor`/`ReleaseInhibitor`, `SwitchVT`, and seat add/remove, in that
   order of usefulness.

All three crates are lane B's, so nothing here is blocked on another lane.
It is not started because it defines a new bus contract and deserves its own
work rather than being folded into a lint cleanup — and because `serve` is
`#[cfg(unix)]`, so it cannot be exercised end-to-end on this host, only unit
tested. Shipping a session lifecycle that cannot be run once is how a program
comes to report work it never did, which is the failure this file already has
an entry about.

**Not to be confused with** the daemon event loop, which a comment in
`main.rs` claimed was missing until 2026-09-12. It exists. That comment also
cited a todo.txt note that has never existed.
