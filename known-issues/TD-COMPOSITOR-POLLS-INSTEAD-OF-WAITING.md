## TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING (lane C, 2026-08-17) - **fixed 2026-09-25 (lane F)**

> **Fixed 2026-09-25 (lane F).** The server loop now *waits*: between ticks it
> blocks on its listener, every client, the display's input devices and a
> deadline — the next owed frame, a hotplug probe, a key repeat, a slow key's
> threshold, an idle watch — through `guiremote::WaitSet`
> (`gui/remote/src/wait.rs`: `poll` on SlateOS and Linux; event-select, a
> high-resolution timer and a message-queue wake on Windows). A request is read
> the moment it arrives, frames stay paced to one per refresh, and on the
> development host an idle desktop wakes twice in the 300 ms the old loop woke
> eighteen times (`an_idle_desktop_does_not_wake_until_something_happens`).
> `IdleBackoff`, mitigation 3 below, is gone: waiting subsumes it.
> design-decisions.md §1302.
>
> **Not yet on SlateOS, for one reason that is not this entry's:** SlateOS's
> `poll` never reports a connection waiting on a *listening* socket, so there
> the loop still asks for connections once a frame and waits no longer than
> one. Requests on existing connections are read at once everywhere. That gap
> is tracked separately — `known-issues.md` → `[F]` 2026-09-25, and
> `requests/f-a-poll-never-reports-a-connection-waiting-on-a-listening-socket.md`.
> The description below is the state before the fix.

**What.** `gui/compositor/src/server.rs`'s `run()` is a polling loop. Once per
display frame interval it wakes, asks the listener whether anyone is trying to
connect, asks every open socket whether it has bytes, composes, and sleeps the
remainder of the interval. Nothing in it *waits* for work; it asks repeatedly
whether work has appeared.

Two costs, one small and one not:

* **Latency.** A request that arrives just after a tick waits until the next
  one — up to one frame interval, 16.7 ms at 60 Hz. That is invisible for
  drawing, which is paced by the same clock anyway, but it is a real floor
  under anything request-shaped: a window that asks for its title to change
  waits a frame for the reply, and an app that does a create-then-draw at
  startup pays two.
* **An idle desktop never idles.** With no clients connected and nothing on
  screen, the loop still runs 60 times a second, doing a `TcpListener::accept`
  that returns `WouldBlock` and then sleeping. On a laptop that is a wakeup
  source that prevents the CPU from reaching a deep idle state, which is
  battery burned to discover nothing happened.

**Why it is written this way.** The standard library has no readiness
primitive. `std::net` gives blocking sockets, non-blocking sockets and
timeouts, and nothing that can block on *several* file descriptors at once —
no `poll`, no `epoll`, no `kqueue`, no IOCP. With only those parts, a server
that must watch a listener and N client sockets has exactly two shapes
available: one thread per socket blocking in `read`, or one thread polling
them all. The polling loop was chosen because a thread per client makes every
piece of compositor state shared-mutable — and the compositor's state is a
window list, a focus stack, a damage set and a framebuffer, all of which are
touched by nearly every operation, so "shared" would in practice mean one big
lock held across composition, which is a worse design wearing a better name.

**The proper fix**, in the order the dependencies force:

1. **On SlateOS, this dissolves.** The design calls for an IOCP-like
   completion port (`design.txt`; `CLAUDE.md`'s performance table lists
   "IOCP-like completion wait" as a critical path). The moment the compositor
   runs on SlateOS, its transport is a channel and its wait is a port wait: one
   blocking call that returns when *any* client has a message or the frame
   timer expires. `run()`'s shape survives — accept, serve, route, compose —
   with the sleep replaced by that wait.
2. **On the hosted build, take the dependency.** `mio` (the readiness layer
   under `tokio`) wraps `epoll`/`kqueue`/IOCP and is the obvious answer if the
   hosted build is ever more than a development harness. Not taken now because
   lane C may not edit the workspace-root `Cargo.toml`, and because a
   dependency added for a build that exists to test another build is a poor
   trade.
3. ~~**Cheap mitigation available today:** back the tick rate off when there
   are no clients *and* nothing composited.~~ **Done 2026-09-07 (lane C).**
   `IdleBackoff` on `Server`: after `SETTLE_TICKS` (60, about a second)
   consecutive ticks with no client connected, no input and no frame composed,
   the wait becomes `IDLE_INTERVAL` (100 ms). Any one of the three signals
   resets it, so the rate snaps back rather than easing up.

   Three things worth knowing about the shape:

   - **The condition is "no clients", not "nothing composed".** A desktop
     sitting still with clients connected composes nothing either, and backing
     off there would add up to 100 ms to every request those clients make. The
     frame timer paces composition; polling at frame rate keeps request
     latency under one frame. Only with no sockets at all does the second
     reason disappear.
   - **It settles rather than switching.** One quiet tick is normal; toggling
     on it would make the interval jitter across every gap in activity.
   - **The idle wait never goes below the frame interval**, so a display
     slower than 10 Hz is not polled *faster* for being idle.

   The latency floor in the first bullet of this entry is untouched, and steps
   1 and 2 remain the fix for it. **This entry stays OPEN for those.**

   The decision lives on `Server` rather than inside `run_with` specifically so
   it can be tested: a loop that ends only when its window closes is not
   something a unit test can drive, so `Server::settle` takes the tick's
   findings and returns the wait, and the tests drive *that*. Nine tests. The
   one that matters most is `a_connected_client_keeps_the_loop_at_frame_rate`,
   because the client-count term is the one a pure `IdleBackoff` test cannot
   reach; deleting that term fails it.

**Severity.** Low. It is waste and a latency floor, not a defect: every
request is answered, in order, within a frame. It is logged because the
polling shape is the kind of thing that gets copied into the next server
written in this tree if nobody has written down that it was a constraint of
the standard library rather than a preference.
