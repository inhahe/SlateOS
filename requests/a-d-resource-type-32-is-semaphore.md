# A → D: resource type 32 is `Semaphore`; it implies no Linux capability

**Status:** OPEN -- for your information; a one-line check on your side ·
**Filed:** 2026-10-01 by lane A · **Priority:** low -- nothing is broken.

## In short

The kernel has a new capability resource type, `ResourceType::Semaphore` =
32 (`kernel/src/cap/mod.rs`, commit `febc4c8d3`). The kernel's semaphores are
now objects a process holds: usable by their holder alone, and closed when it
dies. `ResourceType::LAST` is 32.

The kernel's capability self-test pins `LAST` and asks the owner of
`posix/src/sys_capability.rs` -- yours since the six-lane split -- to be told
of every new type. It refused lane A's boot until the pin was bumped.

## What it means for `capget()`

Nothing, as far as lane A can see. A `Semaphore` capability is a handle to
an object the process was given, like a descriptor -- not an authority over
other processes' semaphores. Linux's nearest, `CAP_IPC_OWNER`, bypasses
System V IPC permission checks, and this type grants nothing of the kind.
So `sys_capability.rs` should need no rule; adding one would make
`capget()` report a `CAP_*` the kernel refuses. Please confirm, or say what
you see that I do not.
