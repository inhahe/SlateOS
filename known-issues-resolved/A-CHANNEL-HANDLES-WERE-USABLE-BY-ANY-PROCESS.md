### A-CHANNEL-HANDLES-WERE-USABLE-BY-ANY-PROCESS -- 2026-10-01 -- FIXED for every IPC handle type (lane A)

**In short:** any process could send on, receive from or close any other
process's channel, and ask who was on its other end. That includes logind's
and netstack's control channels. All it took was counting: a channel handle is
the channel's number shifted left with a side bit, and the numbers count up
from 1. Channels from the service registry were also never released when
their process died, and a service that died kept its name. Reported by lane F
(`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`).

**Cause.** The channel syscalls looked the handle up and checked only that the
channel was open. Nothing asked whether the caller held it. Only
`SYS_CHANNEL_CREATE` recorded its ends in the creator's `ipc_handles`. The
service registry's connect and accept paths did not, so their ends were
neither the process's nor released at its death. Its listeners were recorded
nowhere.

**Fix.**
- `require_ipc_handle(type, raw)` in `syscall/handlers.rs`: the caller must
  hold the handle, or the answer is `InvalidHandle`, the same answer as a
  handle that names nothing, so the error tells a process nothing about
  others' handles.
- Every channel syscall and every listener syscall now calls it first.
- Connect and the three accepts record the end they return; register records
  its listener (under `ResourceType::Service`).
- `ipc::cleanup_handles` unregisters a dead process's listeners, freeing the
  name and closing connections nobody accepted.
- The dispatch rung `test_dispatch_ipc_possession` checks every channel and
  listener call as a second scratch process, through
  `thread::self_test_as_process`, which lends a kernel task a process's
  identity for one call.

**Left open.**
- **Pipes, socket pairs, eventfds, completion ports and timers were fixed
  the same day.** Each of their syscalls checks first. A completion-port
  source is checked as its own syscalls check it (an io ring by
  `io_ring::owned_by_caller`). `SYS_WAIT_MULTIPLE` now checks every kind, not
  only ptys: it had treated the pty space as the only enumerable one. Spawn's
  `fd_map` refuses a pipe, socket pair or eventfd the parent does not hold, so
  a spawn cannot launder one into a child. The dispatch rung covers every call.
- **Semaphores were fixed the same day.** They had no `ResourceType`, so
  they were neither checked nor released when their process died.
  `ResourceType::Semaphore` (32) now records them, the five semaphore syscalls
  check possession, close deregisters, and a dead process's semaphores are
  closed (waiters get `ChannelClosed`). Not inherited across fork, like a
  channel.
- **Point 3 of lane F's request is a feature, on lane A's backlog.** A
  Linux-ABI process (every Rust `std` program) cannot reach channels at all.
  Point 4 -- waiting on a channel together with anything else -- was done the
  same day: `SYS_WAIT_MULTIPLE` takes channels and service listeners, and
  completion ports wake when their sources become ready (they had slept until
  a `notify()` that channels, pipes and eventfds never made).
- **Moving an end to another process is not supported.** Capability transfer
  in a message moves capability-table entries, and nothing makes a channel end
  one. A process can hand another process an end only through the service
  registry. Before this fix a process could pass a channel by number,
  insecurely, and nothing on the system did. Now it cannot at all, until
  transfer is built.
