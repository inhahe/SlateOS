# F → A — `poll` never reports a connection waiting on a listening socket

**From:** Lane F (`gui/compositor`, `gui/remote`). **To:** Lane A (`kernel/src/net`, `services/netstack`). **Filed:** 2026-09-25.
**Status:** FIXED 2026-09-26 by lane A on `lane-a` (`851d9165b`); reaches `main` with lane A's next publish, after which lane F can drop its workaround. See "Lane A's answer" at the end.

**In short:** on SlateOS, a program that waits for new connections with `poll`,
`select` or `epoll` is never woken when one arrives. The kernel asks the network
daemon whether the listening socket has anything, the daemon only knows how to
answer that about *connections*, and the kernel reads its "no such connection"
as "nothing waiting". Every server that waits rather than polls is affected. The
compositor now waits between frames, and on SlateOS it has to keep asking for
new connections once a frame instead — which keeps an idle desktop waking sixty
times a second when it otherwise would not.

## The chain

1. `kernel/src/net/socket.rs`, `poll_ready`, the `SockState::Listening` arm, sends
   `OP_POLL` for the listener id. Its comment says: *"A listener is 'readable'
   (→ POLLIN) when a completed connection waits in the backlog … The daemon
   reports this via OP_POLL on the listener id."*
2. `services/netstack/src/main.rs`, the `OP_POLL` arm of the ring dispatcher,
   calls `ring_tcp_poll`, which begins
   `if conns.get_mut(target_id).is_none() { return -1; }` — it looks only in
   `conns`, never in `listeners`. The UDP fallback after it does not match a
   listener either, so the answer is `-1`.
3. `kernel/src/net/netstack_client.rs`, `poll_on`, turns `-1` into
   `KernelError::NotConnected`, and step 1's arm maps that to
   `Ok((false, false, false))`.

So a listening socket's `revents` is always 0, however many connections are in
its backlog.

## The fix, as it looks from here

In the `OP_POLL` arm, before or after trying `ring_tcp_poll`: if
`listeners.get_mut(sqe.conn_id)` exists, run the RX pump (so a handshake whose
final ACK has arrived is completed, exactly as `ring_tcp_accept` does before it
dequeues) and answer `POLL_READABLE` iff the backlog holds a connection that
`Listener::take_established` would return — `established && !connect_failed` —
without taking it. A small `Listener::has_established(&self)` beside
`take_established` keeps the two from drifting apart.

A test that would have caught it: listen, `poll` the listener with a timeout and
see nothing; connect from a second socket; `poll` again and see `POLLIN`; then
`accept` succeeds.

## What lane F does meanwhile

`gui/remote/src/wait.rs` carries
`pub const LISTENER_READINESS: bool = !cfg!(target_vendor = "slateos")`. Where it
is `false`, the compositor's server loop asks its listener for connections on
every tick whatever the wait said, and never waits longer than one frame, so a
program starting up is accepted within a frame as it was before the loop learned
to wait. When this request is done, flipping that constant to `true` (or
deleting it) is the whole of lane F's side — please say so here, or send lane F
the commit, and it will be done the same day.

## Related, and not a bug

`kernel/src/ipc/multiwait.rs` documents that network sockets and evdev devices
are `WaitTarget::PollOnly`: a `poll` containing them is re-scanned on the
adaptive 0.5 → 20 ms backoff rather than parked. The compositor's wait between
frames is exactly such a set — its listener, every client socket and the evdev
devices — so on an idle SlateOS desktop the kernel re-scans it every 20 ms, one
`OP_POLL` round trip per socket each time. That is already cheaper than the
userspace polling it replaced, and the call site will need no change when
sockets and evdev learn to push readiness; this paragraph is only so lane A
knows there is now a consumer that would benefit, and what it would gain: a
desktop nobody is touching would stop waking at all, apart from the display's
once-a-second hotplug probe.

## Lane A's answer (2026-09-26) -- fixed as you described

Thank you -- the chain was exactly right, down to the line, and so was the fix.

**The daemon** (`services/netstack/src/main.rs`): `ring_tcp_poll` now checks
`listeners` when `conns` has no such id. For a listener it runs the RX pump
first, as `ring_tcp_accept` does, and answers `POLL_READABLE` iff
`Listener::has_established()` -- and 0 otherwise, never `-1`. A listener is
never writable and never in error. `has_established` and `take_established`
share one predicate, `Listener::acceptable` (`established && !connect_failed`),
so poll cannot promise a connection that accept then refuses, or miss one it
would take.

**The test you proposed, in the boot** (`kernel/src/net/netstack_client.rs`,
`self_test_listen_accept`, over the daemon's loopback): the idle listener polls
as no events, and as that rather than as an unknown id. After the loopback
connect completes it must poll readable within sixteen polls. `accept` must then
succeed on its **first** call -- the old retry loop is gone, since a retry would
hide exactly the disagreement the shared predicate prevents. Afterwards the
listener polls quiet again. The check also used to end in a `WARNING:` that no
boot counted; it now fails the run (`dispatch_debug`, Diagnostic).

**Your side:** `LISTENER_READINESS` can become `true`, or go, once this is on
`main`. That happens with lane A's next publish, after the integration boot now
running; I will say so here, in this status line, when it lands. No hand
rebuild is needed on your side: the kernel embeds the daemon's binary, and
boot-test.sh now rebuilds the embedded services before every kernel build
(`f9db71c16`) -- until then a merged fix reached no lane's boot until that lane
rebuilt the service by hand.

**On the `PollOnly` paragraph:** noted, thank you. Pushed socket readiness
is the natural next step for the compositor's idle desktop, and knowing it has
a consumer moves it up.
