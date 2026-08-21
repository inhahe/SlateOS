# B → A — three new `ResourceType`s, so "set the clock", "bind port 80" and "raise my own rlimit" have something to derive permission from

**Filed:** 2026-08-21 by Lane B. **Action needed:** add three variants to
`ResourceType` in `kernel/src/cap/mod.rs`, one new `Rights` bit, and the boot
grants that hand them to `init`. Nothing in your tree changes behaviour on the
day it lands — these are objects nobody checks yet. Lane B then writes the
`project()` rules and flips the libc gates.

## In short

libc used to tell every process it held every Linux privilege. §312 (the
operator's answer to Q44) replaced that invention with a *projection*: each
`CAP_*` is computed from the `(ResourceType, Rights)` handles the process
actually holds, and a `CAP_*` with no matching rule reads **false**.

Steps 1 and 2 landed (your `SYS_CAP_QUERY` enumerates; libc turns handles into
capability words). **Step 3** makes libc's remaining gates consult the
projection. When it does, seven operations become impossible for every process
on the system, permanently — not because anyone denied them, but because there
is no object for the rule to name:

| Operation | libc gate | Linux name it needs |
|---|---|---|
| set the absolute time | `posix/src/time.rs` `clock_settime`, `settimeofday` | `CAP_SYS_TIME` |
| slew the clock | `posix/src/sys_timex.rs` `adjtimex` | `CAP_SYS_TIME` |
| notice a clock step | `posix/src/epoll.rs` `timerfd_create` (`TFD_TIMER_CANCEL_ON_SET`) | `CAP_SYS_TIME` |
| listen on a port below 1024 | `posix/src/socket.rs` `bind` | `CAP_NET_BIND_SERVICE` |
| raise a *hard* resource limit | `posix/src/resource.rs` `setrlimit` | `CAP_SYS_RESOURCE` |
| lock memory past the quota | `posix/src/mman.rs` `check_mlock_caps` | `CAP_IPC_LOCK` / `CAP_SYS_RESOURCE` |

The operator answered `open-questions.md` **Q48 = B** on 2026-08-21: build the
objects rather than accept the breakage. Written up as `design-decisions.md`
**§350**, which also records why B was taken for the port case where the
recommendation had been to drop the rule entirely.

**Why these six and not the other forty-odd gates.** §314 already deleted
libc's guess everywhere the kernel checks again itself — there, libc guessing
"no" costs nothing, because the program asks and the kernel decides. What is
left is by construction the opposite case: libc is the only thing deciding, so
its "no" is final. `setuid`/`setgid` was the seventh and is already handled —
it hangs off `Process` + `SET_CREDENTIALS`, which you added on 2026-08-16.

## What I am asking for, concretely

### 1. Three `ResourceType` variants

Next free discriminant is **27** (`Pty = 26` is the current last).

```rust
/// The system clock, as an object.
///
/// There is exactly one, so `resource_id` is reserved and is 0. A process
/// needs this type with `Rights::WRITE` to set the absolute time
/// (`clock_settime`, `settimeofday`) or to slew it (`adjtimex`). Reading the
/// clock needs nothing and never will.
SystemClock = 27,

/// Authority to bind a local port that the system reserves.
///
/// `resource_id` is a specific port number, or **0 for the class** — the
/// `resource_id == 0` convention this file documents for `Process`/`Thread`.
/// Port 0 is "pick one for me" in the sockets API and is never a bindable
/// address, so the two readings cannot collide, exactly as PIDs starting at 1
/// keeps `Process` unambiguous.
PrivilegedPort = 28,

/// A process's own resource limits.
///
/// `resource_id` is the target PID, or 0 for the class. Needed with
/// `Rights::WRITE` to raise a *hard* limit; lowering a soft limit is
/// unprivileged and is not gated.
ResourceLimit = 29,
```

**A note on `PrivilegedPort`'s granularity, which is the one part I would
argue about.** Per-port is what makes the object worth having over a boolean —
a web server should hold port 80 and not port 22. But it means the grant site
has to know the port at spawn time, which for a daemon read from a config file
it does not. The class grant (`resource_id == 0`) is the escape hatch and will
be what `init` actually uses at first. If you would rather ship only the class
form and add per-port later, say so and I will write the projection to accept
either — but please keep `resource_id` *meaning* the port rather than reserving
it, so adding the fine-grained form later is a grant change and not an ABI
change.

### 2. One new `Rights` bit

`WRITE` covers the clock and the limits cleanly ("may write this object"). The
memory-lock case does not fit — `mlock` past the quota is not a write to
anything — so:

```rust
/// Authority to lock memory beyond the per-process quota.
///
/// Required on a [`ResourceLimit`](crate::cap::ResourceType::ResourceLimit)
/// capability to be projected `CAP_IPC_LOCK`. Locking *within* the quota
/// needs nothing.
pub const MEMORY_LOCK: Self = Self(1 << 19);
```

Bits 16–18 are taken (`IO_REALTIME`, `DEBUG`, `SET_CREDENTIALS`); 19 is the
next free one in the subsystem band, unless something has landed since I read
`rights.rs`.

I am proposing a distinct bit rather than reusing `METADATA` or `WRITE`
deliberately, on the argument `SET_CREDENTIALS`' own doc comment makes at
length and that `METADATA`'s makes again: there are 52 free bits, and a bit
that means two things is a bit that will be granted for one of them and
silently confer the other. `CAP_IPC_LOCK` and `CAP_SYS_RESOURCE` are separate
privileges on Linux and ported software drops them separately.

### 3. The boot grants

§350 records the default I took in the absence of anyone deciding otherwise:
**`init` holds all three (class-wide) and passes them down explicitly.** A
token nobody holds is indistinguishable from having left the operations denied,
which is the outcome the operator rejected.

That means in `kernel/src/proc/spawn.rs`, wherever `init`'s `SpawnOptions`
capability list is built:

```rust
(ResourceType::SystemClock,    0, Rights::WRITE),
(ResourceType::PrivilegedPort, 0, Rights::WRITE),
(ResourceType::ResourceLimit,  0, Rights::WRITE | Rights::MEMORY_LOCK),
```

All three want `Rights::TRANSFER` too if `init` is to delegate them, but I do
not know whether your delegation path reads that bit at the grant or at the
transfer, so I have left it out rather than guess. Add it if it is needed.

**No other fixture needs any of these.** I audited the gate sites against the
ring-3 fixtures the same way as for the `SET_CREDENTIALS` request: nothing in
`services/init`, the shell, `ctest-jobctl`, `ctest-pgroup`, `self_test_cctty`
or any `fastpy-*` fixture sets the clock, binds a low port, or raises a hard
limit. So unlike that request, this one is not boot-test-urgent — it does not
gate anything currently green.

## What Lane B does once it lands

1. Add the three types and the bit to `posix/src/sys_capability.rs`'s mirrored
   definitions.
2. Write the `kernel_view::project` predicates: `CAP_SYS_TIME` ← `SystemClock` +
   `WRITE`; `CAP_NET_BIND_SERVICE` ← `PrivilegedPort` + `WRITE` (class or the
   specific port); `CAP_SYS_RESOURCE` ← `ResourceLimit` + `WRITE`;
   `CAP_IPC_LOCK` ← `ResourceLimit` + `MEMORY_LOCK`.
3. Flip the six gate sites from the invented words to the projection, which is
   §312 step 3 and closes `known-issues.md` →
   `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.

Steps 1–3 are a couple of hours and I will do them as soon as the types exist.

## Deliberately not asked for

`sethostname` stays denied with no object. §312 already declined to invent one
for it and §350 does not revisit that; if the same argument applies to it later
it is a fourth row in the table above, not a revision of this request. I would
rather ask twice than add a `ResourceType` speculatively.

---

**Status:** OPEN — filed 2026-08-21, not yet acted on.
