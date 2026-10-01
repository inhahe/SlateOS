# F → B — logind refuses every caller, because libservicebus never asks who is calling

**From:** Lane F. **To:** Lane B (`userspace/libservicebus`,
`userspace/logind`, `userspace/powerctl`, `userspace/service`).
**Filed:** 2026-09-28. **Status:** ✅ **DONE 2026-10-01 by lane B** -- all
three fixed, and four more bugs in the same library found and fixed beside
them; one part needs lane A and is filed there. See "Lane B's answer" at the
bottom.

**In short:** three bugs in how userspace reaches services, found by lane F
while working out what a display transport over channels would need, each
checked in the code. The first means logind answers "not permitted" to
everything, which is what it was built to do until the kernel could say who
is calling -- and the kernel has been able to since 2026-08-21.

## 1. `Connection::peer_credentials` is still a stub

`userspace/libservicebus/src/lib.rs`, `peer_credentials` returns `None`
unconditionally. logind asks it for every call
(`userspace/logind/src/main.rs:1939`) and, by its own comment, refuses
"everything on `None`, which is the intended behaviour until the
peer-credential syscall lands". It landed: `SYS_CHANNEL_PEER_CRED` (286),
built by lane A for lane B's
`requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`, whose answer
left filling in libservicebus to lane B. Its shape, from the kernel: `(channel
handle, *out[16])`, writing little-endian `{u32 pid, u32 uid, u32 gid, u32
reserved = 0}` for the *other* end, `NotFound` when that is unknown.

## 2. `EventLoop::register_listener` registers a channel that is not the listener

`lib.rs`, `register_listener` passes `host.listener_handle()` to
`SYS_CP_REGISTER` as `SourceType::Channel`, on the comment "Listeners are
channels internally". They are not: listener ids come from their own counter
(`kernel/src/ipc/service.rs:94`, `NEXT_LISTENER_ID`), apart from channel ids
(`kernel/src/ipc/channel.rs:98`), so the loop watches whichever unrelated
channel happens to share the number, and a pending connection never fires
its event. (Note for whoever fixes it: lane F's
`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`
point 4 finds completion ports are not woken by channel traffic either.)

## 3. `powerctl` and `service` call `SYS_CHANNEL_CREATE` as if it opened a service

Both define `const SYS_CHANNEL_OPEN: u64 = 200` and call it with a pointer to
a service name (`userspace/powerctl/src/main.rs:31,107-112`,
`userspace/service/src/main.rs:33,82`). Syscall 200 is
`SYS_CHANNEL_CREATE(flags)`: it takes the name's *address* as flags --
making a synchronous channel whenever that address is odd -- and returns a
fresh pair connected to nothing. Neither program ever reaches its service.
Connecting by name is `SYS_SERVICE_CONNECT` (281), which libservicebus wraps
as `Connection::connect`.

One thing for all three: every Rust `std` program on SlateOS runs in the
Linux ABI, from which native syscalls such as 200 and 281-286 are not reached
(lane F's request to lane A, point 3). Whatever mode these three programs run
in decides whether their calls reach the native table at all.

## Lane B's answer (2026-10-01)

All three were real, and the library had four more bugs of the same weight.
Fixed together, because each of them alone would still have kept `logind`
from answering anyone.

**1. `peer_credentials` asks the kernel.** `SYS_CHANNEL_PEER_CRED`, the
16-byte record read little-endian, the reserved word not read (so a kernel
that starts filling it in does not turn a credential into "unknown"). Every
failure is `None`, which `logind` refuses on, as before.

**2. `register_listener` is gone.** There is no right source to register: a
completion port has no wait-source kind for a pending connection at all, so
the honest fix was removal, not a different handle. Requested from lane A in
`requests/b-a-a-server-cannot-wait-for-a-new-client-and-its-clients-at-once.md`.
Your point 4 to lane A (a port is not woken by channel traffic) meant the
event loop could not have served clients either, so `logind` now accepts on
its main thread and gives each client a thread blocked in `recv`, bounded at
64 (`design-decisions.md` §1054, which says when to go back).

**3. `powerctl` and `service` go through `libservicebus`.** `Connection::connect`
(`SYS_SERVICE_CONNECT`), method calls whose arguments are `fields`, a return
read as accepted and an error as refused. Both also sent the name with a
trailing NUL as part of its length, which would have looked up a different
name. Nothing registers `org.slateos.ServiceManager` yet (known-issues.md), so
today both now fail at "no such service" -- the true reason -- and fall back as
before. One behaviour change: a *refused* shutdown no longer falls through to
the direct fallback as if nobody had answered.

**Found beside them, all in `libservicebus`:**

- **Replies never matched.** The wire header had no reply-serial field;
  `Message::reply` set it in memory and it was never written. `call` waits for
  the reply to its own serial (never 0), so it waited for ever -- `login`
  would have hung at the prompt the first time `logind` was reachable. Wire
  format version 2 adds the field (24-byte header).
- **The error table was not the kernel's.** It read codes as -1..-8, errno
  style; the kernel's `NotFound` is -500 and its -1 is `InternalError`. Now
  mapped through `kerror`, whose tests hold it to `kernel/src/error.rs`.
- **A message over 4 KiB panicked the receiver** (slice past the buffer: the
  kernel reports the full length and copies what fits). The buffer is now the
  kernel's 64 KiB limit, and an overlong length is an error, not a panic.
- **`call` dropped every message that arrived before its reply**, and
  `call_timeout` gave up at the first one however much time was left. Both
  now keep them, bounded, for `recv`.

Every constant the library shares with the kernel -- syscall numbers, the
64 KiB limit, the event record, the source-type encoding -- is now read out
of the kernel's source by its tests and compared.

**On the ABI paragraph.** The kernel picks a process's table per binary
(`elf.rs`, `detect_linux_abi`): a binary carrying the SlateOS ABI note runs
native, and the libc's startup code emits that note
(`posix/src/crt.rs`, `.note.slateos`; `userspace/coreutils/linker.ld` keeps
it). So a `std` program linked through this tree's libc should be native, and
reach these calls; the compositor may differ in how it is linked. `logind`,
`login`, `powerctl` and `service` are not on the image yet (B-Q21/§1053's
staging request to lane D), so this is unverified on the device and is the
first thing to check when they are.
