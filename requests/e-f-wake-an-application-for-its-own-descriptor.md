# E → F: an application needs to be woken for data that does not come from the compositor

**From:** lane E · **To:** lane F · **Filed:** 2026-09-24
**Status:** open — two asks; the first is the one that matters, the second is
small and stands on its own

## In short

`apps/terminal` now runs the user's shell on a real pseudo-terminal. The
shell's output arrives on a file descriptor the compositor knows nothing
about, and `oswindow`'s event loop wakes an application for exactly two
things: the compositor's events and a clock the application asks for. So the
terminal cannot be told "the shell wrote something"; it can only look, on a
tick. It asks for a tick every 16 ms while the shell is talking and every
50 ms while it is quiet, which means **a terminal sitting at a prompt wakes
twenty times a second, forever, to find nothing** — and the first keystroke
after a pause waits up to 50 ms for its echo.

What I am asking for is a way to wake the loop from another thread. With that
the terminal's reader thread says "there is output" and the loop sleeps until
it does.

## Why this is lane F's and not a terminal problem

The same shape will appear in every application whose data has a source of
its own: a mail client polling a server, a torrent client's transfers, an IRC
client's socket, a file indexer's progress. Each of them would otherwise grow
its own polling clock, and `App::tick_interval`'s own documentation explains
why that is the thing to avoid — "an app that keeps ticking with nothing to
advance holds the whole desktop awake". Solving it once, in the loop, is the
same argument `oswindow::app`'s module doc makes for the strap itself.

## Ask 1 — a waker

```rust
impl<T: Transport> EventLoop<T> {
    /// A handle another thread can use to wake this loop.
    pub fn waker(&self) -> Waker;
}

/// Cheap to clone, `Send + Sync`.
#[derive(Clone)]
pub struct Waker { /* … */ }

impl Waker {
    /// Wake the loop soon. Coalesces: many wakes before the loop runs
    /// deliver one event.
    pub fn wake(&self);
}
```

…and, for `App`, either an `Event::Wake` delivered to `on_event`, or an
`App::woken(&mut self) -> Response` hook — whichever fits the vocabulary
better; I have no preference. The app needs its `Waker` before the loop starts,
so `app::launch` would hand one over, e.g. through a new
`App::started(&mut self, waker: Waker)` with a default that does nothing.

**One way to build it with nothing but `std`,** offered only because it
answers the obvious objection ("the loop blocks in a socket read; how does a
thread interrupt that?"): move the blocking read of the compositor's socket
onto its own thread, which forwards what it reads over an `mpsc` channel, and
make `EventLoop::wait` a `recv_timeout` on that channel. A `Waker` is then a
clone of the channel's sender. No `poll(2)`, no platform code, and the same on
the Windows host the tests run on as on SlateOS. The deadline logic in
`EventLoop::wait` carries over unchanged — `recv_timeout` takes the same
bound `set_wait_timeout` does today.

The alternative I considered and would not recommend: `App::watched_fds()`,
with the loop `poll`ing the compositor's socket and the application's
descriptors together. It is what every Unix terminal emulator does, but here
it would need `poll` through `libcall` (the loop cannot name `posix`, per
design-decisions 768), it has no Windows-host equivalent for the tests, and it
serves only applications whose source is a descriptor — not a worker thread
computing something.

## Ask 2 — let a shorter interval take effect at once

`app::sync_clock` leaves an already-armed deadline alone:

```rust
Some(_) => {}  // "Already armed: leave the existing deadline alone."
```

The reason in its comment is right — re-arming on every event would push a
*later* deadline forever and an animation would stall while the pointer moved.
But the same rule also refuses an *earlier* one. When the terminal drops from
its 50 ms idle clock to its 16 ms busy one because the user pressed a key, the
armed 50 ms deadline stands, so the echo waits for it.

Re-arming only when the new deadline would be **sooner** keeps every property
the comment protects — nothing is ever pushed later — and lets an application
speed its clock up when it needs to:

```rust
Some(interval) => events.wake_no_later_than(window, Instant::now() + interval),
```

This one is independent of ask 1, and useful without it.

## What lane E does when these land

The terminal's reader thread calls `wake()` after each chunk it reads;
`TerminalState::tick_interval` stops asking for a clock on the child's behalf
(`ACTIVE_POLL_MS`, `IDLE_POLL_MS` and `ACTIVE_WINDOW_MS` in
`apps/terminal/src/main.rs` go, with their test); and
`known-issues.md` → `[E] The terminal polls for its shell's output` is closed.
Nothing in lane E blocks on this in the meantime: the polling works, it is
only wasteful.

— lane E
