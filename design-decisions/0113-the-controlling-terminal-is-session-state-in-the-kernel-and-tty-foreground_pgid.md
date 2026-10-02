## §113 — The controlling terminal is session state in the kernel, and `tty::FOREGROUND_PGID` had to die for it

**Date:** 2026-08-12
**Decided by:** Claude (autonomous)

### Context

`tcsetpgrp`/`tcgetpgrp` and `ioctl(TIOCSPGRP/TIOCGPGRP)` are how a
job-control shell hands the terminal to a job and takes it back. Before
this change the "foreground process group" existed in three unrelated
places:

1. `posix`'s `FG_PGRP` — a per-process userspace static. A shell that
   foregrounded a job wrote its own copy; the job read its own, different
   copy. Neither could see the other.
2. `kernel/src/tty.rs`'s `static FOREGROUND_PGID: AtomicU64` — the value
   that `^C`/`^\` actually signalled. Only the Linux shim's `TIOCSPGRP`
   ever wrote it.
3. Nothing at all for the native ABI.

So a glibc program and a slateos-libc program disagreed about which job
owned the terminal, and *both* could disagree with the group that would
actually receive `^C`. The roadmap had recorded (1) as "local rather than
wrong — no other process can observe or contradict it"; that rationale
was false the moment (2) existed.

### Decision

One table in `pcb`, keyed by session ID:
`static CTTY_FG_PGRP: Mutex<BTreeMap<ProcessId, ProcessId>>` (sid → fg
pgid). Everything else becomes a derived read of it:

- Native ABI: `SYS_TTY_GET_PGRP`/`SET_PGRP`/`ACQUIRE_CTTY`/`RELEASE_CTTY`
  (537-540).
- Linux shim: `TIOCGPGRP`/`TIOCSPGRP` call the same `pcb` functions;
  `TIOCSCTTY`/`TIOCNOTTY` are implemented rather than silently accepted.
- `tty::foreground_pgid()` returns `pcb::ctty_console_fg_pgrp()` and owns
  no state of its own.
- `posix`'s `FG_PGRP` is deleted; on the host it is replaced by a
  *thread*-local test double, per §110.

Locking order: `CTTY_FG_PGRP` is never taken while `PROCESS_TABLE` is
held. Every accessor reads what it needs from the process table, drops
that lock, then takes the ctty lock.

### Alternatives considered

**Keyed by session vs. a single global `Option<(sid, pgid)>`.** We have
exactly one console, so a single `Option` would be sufficient today and
simpler. Rejected: the map costs nothing, and the *shape* of the state is
the thing being got right — "the foreground group belongs to a session"
is the invariant that the old code violated, and a global `Option` states
"the foreground group belongs to the machine", which is the same category
error one step further out. The one place the single-console assumption
does leak is `ctty_acquire`, which treats "the map is non-empty" as "the
console is taken"; that is documented at the call site and is the only
line that needs revisiting when a second terminal exists.

**Reusing `TIOCSPGRP`'s existing `tty.rs` atomic and having libc call
into it.** Rejected: it has no notion of a session, so it cannot answer
`ENOTTY` (a process in a session with no terminal), cannot reject a pgid
from another session, and cannot be cleaned up when a session exits. It
was the wrong shape, not merely the wrong location.

**New syscall numbers (537-540) vs. multiplexing an existing one.**
Consistent with §112: a distinct operation gets a distinct number rather
than a mode argument on a neighbour. Four numbers because the get/set
pair and the acquire/release pair genuinely do four things.

**Leaving `TIOCSCTTY`/`TIOCNOTTY` as silent no-ops.** They were
previously accepted and ignored, which was harmless only while
`tcgetpgrp` could never fail. Once `ENOTTY` became reachable, a program
that did `setsid(); ioctl(fd, TIOCSCTTY, 0); tcsetpgrp(...)` would have
been told its `TIOCSCTTY` succeeded and then denied the terminal. Silent
acceptance is a lie that only pays while nothing checks.

**`ctty_acquire` resetting the foreground group when called twice.**
Rejected. Some programs call `ioctl(TIOCSCTTY)` defensively at startup;
if a redundant call reset the foreground group to the caller's own, a
shell would yank the terminal back from a job it had just foregrounded.
A repeat acquire by the session that already owns the terminal succeeds
and changes nothing.

### Consequences

- The group that receives `^C` and the group userspace believes is
  foreground can no longer diverge, because they are the same read.
- `setsid()` drops the caller's terminal (which is why the daemon idiom
  is `setsid()` and not `TIOCNOTTY` — the latter hangs up the caller's
  own group).
- `TIOCNOTTY` by the session leader does SIGHUP-then-SIGCONT across the
  foreground group, matching Linux's `disassociate_ctty(on_exit=0)`. The
  hangup lives in `handlers::hangup_released_ctty` and is shared by both
  ABIs so it cannot drift.
- `SIGTTIN`/`SIGTTOU` enforcement is *not* included: the predicate it will
  use (`pcb::ctty_is_background`) is here, but nothing calls it yet. The
  enforcement points are the console read path, the console write path
  (gated on `TOSTOP`), and `tcsetpgrp`/`tcsetattr`. See `todo.txt`.
- **`Ctrl-Z`, by contrast, started working the moment this landed**, and an
  earlier draft of this entry wrongly listed it as deferred alongside
  `SIGTTIN`/`SIGTTOU`. `tty::feed` has always turned `VSUSP` into
  `LineStep::Signal(20)` under `ISIG`, exactly the way it turns `VINTR`
  into `SIGINT`, and `linux.rs`'s `deliver_console_signal` has always sent
  that to every member of `pids_in_group(tty::foreground_pgid())`. What was
  missing was not the generation or the delivery but the *target*: before
  this change `foreground_pgid()` read `tty.rs`'s own atomic, which only
  the Linux shim's `TIOCSPGRP` ever wrote, so a `^Z` was delivered to
  whatever group last happened to be written there. Now that it is a
  derived read of the session's table, `^Z` reaches the job the shell
  actually foregrounded. This is the clearest illustration of why the
  three-copies bug was worth fixing: the job-control *mechanism* was
  complete and correct, and was simply aimed at the wrong group.
