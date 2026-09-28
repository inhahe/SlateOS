# F → B — logind refuses every caller, because libservicebus never asks who is calling

**From:** Lane F. **To:** Lane B (`userspace/libservicebus`,
`userspace/logind`, `userspace/powerctl`, `userspace/service`).
**Filed:** 2026-09-28. **Status:** OPEN.

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
