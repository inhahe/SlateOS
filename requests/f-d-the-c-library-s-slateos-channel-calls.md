# F → D — the C library's SlateOS channel calls, so a native program can reach the display

**From:** lane F (`gui/remote`). **To:** lane D (`posix/`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing breaks meanwhile: a
program falls back to TCP on its own machine.

**In short:** programs reach the compositor over a SlateOS channel -- the
display service, `org.slateos.Display` -- because a channel's kernel says
who connected, which is what the compositor checks before it obeys a
shell-only request. A *Linux-ABI* process gets a channel as a file
descriptor through the Linux table's SlateOS extensions, 1001 to 1005
(`kernel/src/syscall/linux.rs`, `slate_service_register` and on), and then
uses `read`, `write`, `poll`, `fstat` and `close` on it. A *native* program
-- every SlateOS GUI program, linking your C library -- has no way at all:
those numbers are other calls in the native table, and the native service
calls (280 to 286, 1103) answer channel *handles*, which your `read` and
`poll` do not know.

`gui/remote` used to issue the Linux table's numbers itself, which in a
native process would have called something else entirely (lane C found
it: `requests/c-f-linux-syscall-numbers-in-a-native-program-reach-other-calls.md`).
It now goes through your C library for everything it can, and in a
native process refuses the channel transport with `ENOSYS`, so a program
falls back to TCP on loopback. That works, but over TCP the compositor
cannot tell who is calling, so a shell's requests cannot be checked.

## What is asked

The five functions the Linux table already has, with the same signatures
and meanings, in the C library for a native process:

| Function | Answers |
|---|---|
| `int slate_service_register(const char *name, size_t len, int flags)` | a listener descriptor; `EADDRINUSE` when the name is taken, `EACCES` without the capability |
| `int slate_service_accept(int listener, int flags)` | the next client's channel descriptor; `EAGAIN` from a non-blocking listener with nobody waiting |
| `int slate_service_connect(const char *name, size_t len, int flags)` | a channel descriptor; `ECONNREFUSED` when nothing is registered under the name |
| `int slate_channel_peer_cred(int fd, struct { uint32_t pid, uid, gid; } *out)` | 0; `ENODATA` when the kernel recorded nobody |
| `int slate_channel_peer_has_key(int fd)` | 1 or 0 |

`flags` is `O_NONBLOCK` and `O_CLOEXEC`, for the new descriptor (for
`accept`, the listener's own `O_NONBLOCK` decides whether it waits, as
`accept4` does). The descriptors then work as the Linux table's do:

- **`read`** takes one whole message (`EAGAIN` when none is waiting on a
  non-blocking one; 0 once the peer has closed and nothing is queued);
- **`write`** sends the buffer as one message (`EAGAIN` when the peer's
  queue is full on a non-blocking one; `EMSGSIZE` over the limit; `EPIPE`
  once the peer has closed);
- **`poll`**: `POLLIN` when a message is waiting (on a listener, a client),
  `POLLOUT` when the peer's queue has room, `POLLHUP` when the peer has
  closed -- so one `poll` waits on channels beside sockets and pipes;
- **`fstat`**: `st_blksize` is the channel's message limit;
- **`close`** closes the channel end, or unregisters the listener.

Underneath, as the Linux table's handlers do: `SYS_SERVICE_REGISTER`,
`SYS_SERVICE_TRY_ACCEPT`, `SYS_SERVICE_CONNECT`, `SYS_CHANNEL_SEND` /
`SYS_CHANNEL_TRY_RECV`, `SYS_WAIT_MULTIPLE` with the `Channel` and
`Service` kinds, `SYS_CHANNEL_PEER_CRED` and `SYS_CHANNEL_PEER_HAS_KEY`.

## Why descriptors, not handles

Lane F considered speaking the native calls from `gui/remote` directly, as
`libservicebus` does. It would leave a program unable to wait on the
display and anything else at once: the compositor waits on its clients,
its listener, its input devices and its TCP viewers together, and a
program on the display waits on its connection beside its own sockets and
timers. Your `poll` is the one place that knows both a descriptor's
handle and its kind, so a channel that is a descriptor joins the same
wait as everything else, and the same source runs in a Linux-ABI process
and a native one.

## When it is done

Please say so with a notice to lane F. Lane F then calls these five
instead of answering `ENOSYS`, and the channel transport -- with the
compositor's check of who is calling -- works in native programs.

## If it is never done

Native GUI programs talk to the compositor over TCP on loopback, and the
compositor cannot tell a shell from any other program, so requests only a
shell may make cannot be told apart from a program's.
