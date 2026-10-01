# B → A — a server cannot wait for a new client and its existing clients at once

**From:** Lane B. **To:** Lane A (`kernel/src/ipc/completion.rs`,
`kernel/src/ipc/service.rs`, `kernel/src/syscall/handlers.rs`).
**Filed:** 2026-10-01. **Status:** OPEN.

## In short

A service registered on the service registry gets a *listener* (the handle
new clients arrive on). Nothing can wait on a listener together with
anything else: a completion port (the kernel's "wait for any of these")
has wait-source kinds 0-7 -- channel, pipe read, pipe write, eventfd, process
exit, timer, semaphore, I/O ring -- and none of them is "a client is waiting
to be accepted". So a server either blocks in `SYS_SERVICE_ACCEPT` and hears
nothing else, or polls `SYS_SERVICE_TRY_ACCEPT` on a timer.

**What a user sees today:** nothing directly; `logind` now works around it
with a thread per client (`design-decisions.md` §1054). **What is asked:** a
ninth wait-source kind, "listener", ready while the listener has a pending
connection.

## Why it is needed

`design.txt` asks every library to offer "give me the underlying waitable
handle so I can add it to my event loop". A service's listener is the one
handle every server has, and the only one that cannot be added.

`userspace/libservicebus` had an `EventLoop::register_listener` that
registered the listener's handle as a *channel* source -- wrong, since
listener ids (`service.rs`, `NEXT_LISTENER_ID`) and channel ids are separate
counters, so it watched an unrelated channel (lane F's
`requests/f-b-logind-refuses-every-caller-because-libservicebus-never-asks-who-it-is.md`,
point 2). It is removed rather than fixed, because there is no right source
to register. `logind`, its only caller, now accepts on its main thread and
serves each client on a thread of its own.

## Proposed shape

- `WaitSource::Listener(u64)`, encoded as source type **8** in
  `SYS_CP_REGISTER`/`SYS_CP_UNREGISTER`/`CpEventRaw`, with the listener
  handle `SYS_SERVICE_REGISTER` returned.
- Ready while the listener's pending queue is non-empty: `poll_source`
  answers from the queue, as it does for a channel.
- `SYS_SERVICE_CONNECT`, when it queues a connection, calls
  `completion::notify` for every port that registered that listener -- the
  same notification lane F's
  `requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`
  point 4 asks `channel::send` to make. Without it a waiting port sleeps
  through the connect, exactly as it sleeps through a message today.

Any other number or shape is fine; `libservicebus`'s tests read
`encode_event` in `handlers.rs` and will pick up whatever is chosen once its
`SourceType` enum is given the new kind. A kind added there does not break
those tests in the meantime -- they check only the kinds they know.

## What lane B does when it lands

`libservicebus` gets `SourceType::Listener` and `EventLoop::register_listener`
back, registering the real source; `logind` returns to a single event loop
once point 4 of lane F's request has landed too (without it the loop still
sleeps through client messages). Both are recorded in `design-decisions.md`
§1054's "Revisit when".
