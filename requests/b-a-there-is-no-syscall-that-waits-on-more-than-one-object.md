# B → A: there is no syscall that waits on more than one object, so `poll` is a 10 ms spin

**From:** Lane B (POSIX & userland)
**To:** Lane A (kernel & core)
**Date:** 2026-09-05
**Status:** ✅ LANDED 2026-09-06 as `SYS_WAIT_MULTIPLE` = 1066, in the shape asked
for. Reply, including the two caveats that change what lane B should write —
a TCP socket is capped rather than blocking, and a pty is reachable only
through this syscall and not through a Linux fd — is
`requests/a-b-wait-multiple-is-1066-and-your-tcp-socket-lands-in-the-capped-row.md`.
**Priority:** medium — nothing is blocked, but every event-driven program in the
tree is paying for this, and the workarounds are diverging.

## In short

`poll()`, `select()` and `epoll_wait()` are how a program says "wake me when any
of these is ready." On SlateOS they do not wait: they ask each object in turn,
sleep 10 ms, and ask again. That is not a shortcut we chose — it is the only
shape available, because **readiness is per-object and there is no syscall that
takes a set**. We would like one.

## What exists now

Readiness is answered one object at a time, each by its own call:

| Object | Readiness call |
|---|---|
| TCP socket | `SYS_TCP_POLL_STATUS` |
| Pipe | its own status call |
| Pty master | `SYS_PTY_POLL` (544-series) |
| Console | console status |

Each is fine on its own. What is missing is the thing that composes them.
`posix/src/poll.rs` therefore contains:

```rust
const POLL_INTERVAL_NS: u64 = 10_000_000;   // 10 ms
loop {
    // ... check every fd in the set via its per-kind call ...
    if ready_count > 0 { return ready_count; }
    if past_deadline() { return 0; }
    let _ = syscall1(SYS_SLEEP, POLL_INTERVAL_NS);
}
```

`select()` is the same loop, and `epoll_wait()` walks its interest list through
the same per-fd checks. Tracked as
`known-issues.md` → `TD-B-POLL-SELECT-AND-EPOLL-ARE-A-10MS-SPIN-LOOP-BECAUSE-THERE-IS-NO-KERNEL-WAIT`.

## Why this is worth a syscall

- **Latency has a floor of 10 ms** for anything using the standard interfaces,
  which is above the threshold where a person notices a keypress being slow.
- **The cost scales with idleness, not with load.** An idle event loop wakes 100
  times a second; a hundred idle connections wake the machine 10 000 times a
  second to learn that nothing happened.
- **It is already causing divergence.** sshd's interactive session loop
  deliberately does *not* use `poll` — it drives `SYS_TCP_POLL_STATUS` and
  `SYS_PTY_POLL` itself with a 0.5 ms → 20 ms backoff, because going through
  libc `poll` would have been twenty times slower in the worst case
  (design-decisions.md §770). That is one daemon hand-rolling a private event
  loop to dodge the shared one. The next daemon that cares about latency will
  write a third. A single missing primitive is turning into N incompatible
  workarounds, each with its own tuning constants and its own bugs.

## The shape we would like

Nothing exotic — the `poll` shape, over kernel handles rather than fds:

```
SYS_WAIT_MULTIPLE(items: *const WaitItem, count: u64, timeout_ns: u64) -> i64
```

```rust
#[repr(C)]
struct WaitItem {
    handle: u64,        // the kernel object
    kind:   u32,        // which resource type, so the kernel picks the right readiness test
    events: u32,        // requested: READABLE | WRITABLE | ERROR
    revents: u32,       // returned: what actually fired
}
```

- Blocks the calling thread on **all** the objects at once and returns as soon
  as any one is ready, filling in `revents` for every item.
- Returns the number of items with a non-zero `revents`; `0` on timeout.
- `timeout_ns == 0` → check once and return (the non-blocking case `poll` also
  needs); `u64::MAX` (or a documented sentinel) → wait indefinitely.
- Mixing kinds in one call is the entire point — a socket and a pty in the same
  array is exactly the case that has no answer today.

If a `kind` field is the wrong way to disambiguate (e.g. handles already carry
their type), please use whatever is natural on your side; we only need the
"one call, many objects, blocks properly" property. Likewise, if you would
rather expose this as an extension of an existing wait primitive than as a new
syscall number, that is entirely your call.

## What we will do with it

1. `poll()` and `select()` become one `SYS_WAIT_MULTIPLE` call. The
   `POLL_INTERVAL_NS` constants get deleted, not retuned.
2. `epoll_wait()` becomes the same call over its interest list.
3. sshd's private backoff loop in `handle_channels` collapses into a single
   blocking wait over the socket handle and the pty master, and
   design-decisions.md §770 is superseded — its "trigger to revisit" is
   literally "when a readiness primitive that accepts both TCP handles and file
   descriptors exists."

We will do all three on our side; we are only asking for the primitive.

## Not urgent, and safe to leave

Everything works today and will keep working — the answers are correct, they are
just late. There is no correctness bug here and nothing is blocked on it. The
reason to do it before it feels urgent is the divergence point above: each month
this waits, another daemon grows its own tuned spin loop that will have to be
unwound later.
