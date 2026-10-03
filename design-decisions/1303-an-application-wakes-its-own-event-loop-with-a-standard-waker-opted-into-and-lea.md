## 1303. An application wakes its own event loop with a standard `Waker`, opted into, and learns of it as a `Dispatch`, not an `Event`

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). An API shape inside lane F's crates with
no user-visible fork; recorded because every application that does work on a
second thread will be written against it, and because the rejected placement —
a new `guitk::event::Event` variant — is the one that looks more obvious.

**In short:** an application's window loop sleeps until something happens on
its connection to the compositor. Work the application does on another thread
— decoding a photograph, reading a file — had no way to say "finished, draw
again", so a result sat unseen until the user moved the mouse
(`TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`). Now it has
one: an application asks for a waker, gives it to its worker, and is called
back on its own thread when the worker uses it.

### The mechanism

`guiremote::wait::wake_channel` is the self-pipe trick: a receiving half that
goes into a `WaitSet` (§1302) and a `Send + Sync` sending half. A `Socket`
that has handed out a waker waits on its stream and the pipe together; one that
has not keeps its blocking `peek`. `Transport::waker` exposes it, defaulted to
`None`; `EventLoop::waker` wraps it so that a wake also sets a flag the loop can
read — the transport alone cannot tell a wake from a stray return of its wait.

### The choices with two sides

1. **A `std::task::Waker`, not a type of our own.** *For:* it is the standard
   handle for exactly "tell whoever is waiting to look again" — cloneable,
   `Send`, understood by any async executor an application might bring.
   *Against:* its `wake` cannot report failure. Nothing a waker can hit is the
   waker's to act on (a full pipe already holds a wake; a closed one has nobody
   left to wake), so nothing is lost.
2. **A pipe on Linux and SlateOS, a loopback UDP pair elsewhere.** A pipe is
   kernel-native and, on SlateOS, one of the few objects a `poll` truly parks
   on; sockets there go through the network daemon. Windows' wait takes only
   sockets.
3. **Opt-in (`App::wants_waker`), not always made.** *For:* nearly every
   application does everything on one thread and would be paying a pipe and,
   worse, a different wait path (see the `Socket` note above) for nothing.
   *Against:* a second method to implement. Accepted: the default is correct
   for the 140 applications that will never override it.
4. **`Dispatch::Woken`, not `Event::Woken`.** *For `Event`:* `EventLoop::run`'s
   per-event handler would see it without change. *Against, and decisive:*
   `guitk::event::Event` is what arrives from the compositor, addressed to a
   window — a wake does neither, and every `match` on `Event` across the tree
   (lane C's toolkit and 140 applications) would have to learn to ignore it.
   `Dispatch` is oswindow's own vocabulary for what the loop hands over,
   already carries a non-event (`Settled`), and is matched in two places.
   `run` users who need wakes use `run_batched`, or `take_woken` by hand.
5. **Delivered after the batch's events and before its `Settled`,** so a result
   picked up in `on_wake` is drawn in the same frame, and wakes that land
   together are one delivery — the application drains everything ready rather
   than one item per wake.
6. **`App::on_wake` defaults to `Redraw`,** which is right for the simplest
   integration (a worker leaves its result where `render` looks); only an
   application that opted in ever reaches it.

### How it is held

`WaitSet`'s wake tests on Windows and Linux (a wake from another thread ends a
wait; one sent before anyone waits is not lost; a drained receiver blocks
again; 200,000 wakes never block the waker); `Socket`'s (a waker ends a wait, a
consumed wake does not end the next, a socket with a waker still wakes for
bytes, one outliving its socket is harmless); oswindow's (a wake is handed over
before the frame it belongs to, wakes that land together are one, a real
socket's parked loop is woken from another thread); and `app`'s (a finished
result is handed back and drawn, `Idle` draws nothing, `Exit` ends it, an
application that wants no waker gets none).
