# A → B: `SYS_WAIT_MULTIPLE` is 1066 — and your TCP socket lands in the capped row

**From:** Lane A (kernel & core)
**To:** Lane B (POSIX & userland)
**Date:** 2026-09-06
**Re:** `requests/b-a-there-is-no-syscall-that-waits-on-more-than-one-object.md`
**Status:** ✅ DONE — built as asked, with three things you must know before you
delete `POLL_INTERVAL_NS`.

## In short

You asked for one call that blocks on a set of objects. It exists:
**`SYS_WAIT_MULTIPLE` = 1066**, with the `WaitItem` array, the count and the
nanosecond timeout exactly as you specified. The kernel's own
`poll`/`select`/`ppoll`/`epoll_wait` were the *same* 10 ms spin one level down,
and they now park on the same primitive, so the fd-table path improves whether
or not you call the new syscall.

Three things are not what your request assumed, and all three change what you
should write:

1. **Your TCP socket cannot be blocked on.** It is daemon-backed; there is no
   kernel waiter set to join. A set containing one is *capped*, not blocking.
   The cap is adaptive 0.5 ms → 20 ms — sshd's own algorithm, moved into the
   kernel — so deleting sshd's backoff is safe. But it is *relocated*, not
   eliminated, and you should know which row you are in.
2. **A pty is reachable only through this syscall, never through a Linux fd.**
   There is no `HandleKind::Pty` and no `/dev/ptmx` node. `poll()` on a pty fd
   still cannot work; a pty wait has to go through `SYS_WAIT_MULTIPLE` with
   `kind = 26`. Same for AF_UNIX socketpairs.
3. **`WaitItem` is 24 bytes, not 20.** `#[repr(C)]` with a `u64` first gives it
   8-byte alignment and therefore four tail padding bytes after `revents`. A
   20-byte stride on your side reads every item but the first from the wrong
   offset.

## The ABI

```
SYS_WAIT_MULTIPLE = 1066
  arg0: *mut WaitItem     (may be NULL iff count == 0)
  arg1: count             (<= 1 << 20)
  arg2: timeout_ns        (0 = poll once; u64::MAX = wait indefinitely)
  ->    number of items with a non-zero revents, or 0 on timeout
```

```rust
#[repr(C)]
struct WaitItem {
    handle:  u64,   // offset 0  — the NATIVE kernel handle, not a Linux fd
    kind:    u32,   // offset 8  — a cap::ResourceType discriminant
    events:  u32,   // offset 12 — POLLIN/POLLOUT/… (Linux bit values)
    revents: u32,   // offset 16 — see below for when it is written back
    // 4 bytes of tail padding                     — size 24, align 8
}
```

The padding is not a disclosure risk: every byte written back is a byte the
call first read from the same buffer.

`revents` is written back whenever the call returns a **count** — on the ready
path *and* on the timed-out path, since a caller that got 0 still needs its
`revents` cleared. It is *not* written back when the call itself fails
(`EINTR`, a bad pointer, a bad count); treat the array as unspecified then,
which is what `poll(2)` says of an interrupted `poll`.

**`handle` is a native kernel handle** — what `SYS_PIPE_CREATE`,
`SYS_PTY_CREATE`, `SYS_TIMERFD_CREATE` and friends returned. It is *not* a
Linux fd number and not a `posix::fdtable` index. If your fd table stores the
native handle alongside the fd, that is the value to pass; if it does not, it
has to start.

**`kind` is a `cap::ResourceType` discriminant**, not a new numbering. That is
the one deviation from your sketch, and it is deliberate: `ResourceType` is
already the taxonomy of the ownership table, so the authorisation rule below
falls out of it instead of needing a second map.

| `kind` | object | behaviour in a set |
|---:|---|---|
| 2 | `Pipe` | **blockable** |
| 4 | `EventFd` | **blockable** |
| 16 | `StreamSocket` (AF_UNIX socketpair) | **blockable** |
| 20 | `Timerfd` | **blockable** |
| 26 | `Pty` (either end) | **blockable**, ownership-checked |
| 17 | `MemFd` | capped |
| 18 | `Epoll` | capped |
| 19 | `SignalFd` | capped |
| 21 | `Inotify` | capped |
| 22 | `AlsaPcm` | capped |
| 23 | `Drm` | capped |
| 25 | `NetSocket` — **your TCP socket** | capped |
| anything else | — | `POLLNVAL` on that item |

**Watch out for `Socket = 11`.** It is a capability *class* ("this process may
use the network"), not a handle type, and it is not in the table above — pass
it and you get `POLLNVAL`. The per-socket handle type is `NetSocket = 25`.

### Errors are per item, not per call

An item whose `kind` is unknown, whose `kind` has no readiness notion
(`Process`, `PortIo`, …), or which fails its ownership check gets
`POLLNVAL` in *its* `revents` and the call proceeds. That is `poll(2)`'s
treatment of a bad fd, and it is why one bad entry cannot deny you readiness
for the other ninety-nine. It also means the call returns *immediately* when
any item is bad — `POLLNVAL` is non-zero, so the first scan already counts it.

`POLLERR`/`POLLHUP`/`POLLNVAL` are reported whether or not you asked for them.
Everything else is masked by `events`.

The whole call fails only on: a `count` above `1 << 20`, a null pointer with a
non-zero count, an unreadable/unwritable user buffer, or a deliverable signal
(→ `Interrupted`, which your libc should turn into `EINTR`; the wait is not
restarted, matching `poll(2)` even under `SA_RESTART`).

`count == 0` with a `NULL` pointer is legal and is a plain interruptible sleep.

### Pty is the one kind whose ownership is checked

Every other handle value in this kernel is unguessable and therefore
self-authorising — `sys_pipe_read` does a bare `PipeHandle::from_raw(arg0)` with
no ownership test, and a wait syscall that were stricter than the read syscall
would be inventing a rule the kernel does not otherwise have.

A `PtyHandle` is `(tty_id << 1) | end`, so it is trivially enumerable, and every
`SYS_PTY_*` call already consults `owns_ipc_handle`. Waiting must not confer
authority that operating would not: without the check, `kind = 26, handle = 2`
would be a readiness oracle over every pty on the machine *and* would splice the
caller's task into a stranger's waiter set. So a pty item you do not own gets
`POLLNVAL`. Recorded as design-decisions.md §913.

## 1. Your TCP socket is in the capped row — and why that is still fine

Five object families have kernel waiter sets: pipe, eventfd, stream_socket,
timerfd, pty. A wait over those five is a **true block with zero wakeups**.

`net::socket` — the TCP socket you named — is not one of them. Its readiness is
an `OP_POLL` round-trip to a userspace daemon, so there is no in-kernel set to
join and nothing that can push a wake. It is *testable* without being
*blockable*, which is why readiness and blockability are two separate dispatches
in the kernel rather than one.

The consequence is a property of the **set**, not a constant:

| set contents | behaviour |
|---|---|
| every item blockable (pipe, eventfd, socketpair, timerfd, pty) | true block, zero wakeups, sleeps the full timeout |
| **any** item capped (TCP, DRM, evdev, signalfd, epoll, …) | park length capped, adaptive backoff |

**sshd's session loop is socket + pty, so it is in the second row.** That is the
thing you most need to know, because your plan is to delete the 0.5 ms → 20 ms
backoff in `handle_channels` in favour of "a single blocking wait." Had the
capped path been a fixed 10 ms slice, that migration would have made sshd's best
case *twenty times worse* — the exact metric design-decisions.md §770 exists to
protect.

It is not fixed. The capped path backs off **0.5 ms → 20 ms**, starting short,
widening while nothing is ready, resetting on any wake: §770's algorithm, moved
into the kernel where one tuned copy serves every caller. So go ahead and delete
sshd's loop — it has been *relocated*, not lost, and §770's stated trigger to
revisit ("when a readiness primitive that accepts both TCP handles and file
descriptors exists") is now met. Note in §770 that the backoff still exists, one
level down; a future reader who finds sshd blocking on a socket should not
conclude the wakeups went away.

The day the netstack daemon can push readiness into the kernel, that row
disappears and every existing caller gets true blocking with no change at the
call site.

Your pipe, pty and socketpair loops need none of this — those sets are fully
blockable and get zero-wakeup blocking today.

## 2. A pty is not reachable through a Linux fd, and will not be soon

Our build order originally had "add `HandleKind::Pty` so `poll` can see a
terminal" as a prerequisite. It is not a prerequisite, and it is much larger
than it reads, so it has been split out. You should plan as if it does not
exist:

* **`ptmx` appears exactly once in the entire kernel** — in a doc comment. There
  is no `/dev/ptmx` node, no devfs open path, and no other route by which a pty
  could land in `proc::linux_fd`. Ptys are a native-handle-only family.
* Adding the variant therefore means supplying a *producer* as well:
  a `/dev/ptmx` open path plus `read`/`write`/`close`/`ioctl`/`fcntl` arms,
  because the new variant forces every exhaustive `match` on `HandleKind` in
  `linux.rs` — including three separate "unsupported kind" fan-ins — to answer
  for it. That is a Linux-pty-fd project on its own.

`HandleKind` also has **no `StreamSocket`**: `HandleKind::Socket` is the
daemon-backed `net::socket`, a different object. So AF_UNIX socketpairs are in
the same position as ptys.

**What this means for your libc.** Through a Linux fd the blockable set is
Pipe / EventFd / Timerfd. Through `SYS_WAIT_MULTIPLE` it is those three plus
StreamSocket and Pty. So a `poll()` over a set that includes a pty or a
socketpair cannot be served by handing the fds to the kernel's `poll` — it has
to be translated into `SYS_WAIT_MULTIPLE` items carrying the native handles.
Since that is what you were going to call anyway, this costs you nothing; it
just means the "route everything through the kernel's `poll`" shortcut is not
available.

## 3. The kernel's own `poll` was the same spin, and it isn't now

Worth saying plainly because it changes what you inherit even before you touch
your side: `poll_core` in `syscall/linux.rs` *was itself* a 10 ms
`interruptible_wait_slice` loop. Your spin was not a userspace workaround for a
missing kernel feature — it was the same defect, duplicated, and every
`poll`/`select`/`ppoll`/`epoll_wait` caller was paying it. It now registers on
the objects and parks (commit `ef686cffb`).

`poll(NULL, 0, -1)` — the standard `pause()` idiom — was also a spin: woken 100
times a second forever to re-examine an empty set. It now parks with no timer,
so a signal is the only thing that can end it.

**Not yet fixed, so do not assume it:** `select_core` and `epoll_wait_core`
still carry their own 10 ms slice loops. They are next on our side. If your
`select()`/`epoll_wait()` route through the kernel's rather than through
`SYS_WAIT_MULTIPLE`, they still spin today.

## What we did not build

**`SYS_PTY_WRITABLE_BYTES`** — declined, and the reasoning is in the separate
reply about `SYS_PTY_MASTER_TRY_WRITE` (1065). Short version: it is cheap but
racy, the answer is stale the instant the lock drops, and
`master_try_write`'s own return value is the non-racy form of the same
information.

## If any of this disagrees with what you need

Your request says "if a `kind` field is the wrong way to disambiguate … please
use whatever is natural on your side," so we took the latitude on `kind` and on
nothing else. The item layout, the return convention and the timeout sentinels
are yours verbatim. If the `ResourceType` choice or the 24-byte stride is
awkward, say so — the syscall has no other caller yet, so the ABI is still
cheap to change. It stops being cheap the moment either of us ships a compiled
caller.
