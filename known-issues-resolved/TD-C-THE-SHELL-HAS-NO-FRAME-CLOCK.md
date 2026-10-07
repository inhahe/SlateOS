## TD-C-THE-SHELL-HAS-NO-FRAME-CLOCK (lane C, 2026-08-22) — RESOLVED 2026-08-22

**Status:** FIXED 2026-08-22 (lane C). Filed among the resolved on 2026-10-05; what follows is the entry as it stood.

**In short:** Nothing on the desktop can move by itself. A menu cannot slide
open, a window cannot fade, a progress spinner cannot spin — not because the
code for those is missing, but because there is no heartbeat to step it with.
The shell only wakes up when the user does something; if nobody touches the
keyboard or mouse it goes to sleep in the middle of any animation and stays
there. Roughly a thousand lines of animation code exist and none of it has ever
run.

**Where.** `gui/window/src/lib.rs:914`, `EventLoop::run`:

```rust
while self.running && self.conn.is_open() {
    let mut dispatched = false;
    while let Some((window, event)) = self.poll()? { ... }
    if !dispatched && self.running {
        self.conn.wait()?;      // blocks until a byte arrives on the socket
    }
}
```

`Connection::wait` (`gui/remote/src/socket.rs:373`) blocks on socket
readability and takes no timeout. There is no timer, no deadline, no frame
callback and no "wake me in 16 ms" anywhere in `oswindow`'s API surface, so a
loop with no input pending simply stops. That is the correct design for an
*event-driven* client — it burns no CPU on an idle desktop, which is exactly
what a polling loop would get wrong (compare `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`,
which is the same tradeoff resolved the other way and filed as debt for it).
What is missing is the second wake-up source, not the blocking.

**What it costs today, in order of size.**

1. **`gui/desktop/src/animations.rs` is 1036 lines with no caller.** Six
   `tick()` methods, a `tick_render`, easing curves, an animated-rect
   interpolator and a per-window batch stepper — and a tree-wide search for
   `animations::` outside the module itself returns nothing. `session.rs` and
   `lib.rs` call no `tick` of any kind. This is the same defect as
   `TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS`, a module larger than
   half of that one, and it is *not* fixable by wiring: there is nothing to
   wire it to.
2. **The overview's open fade was deleted rather than shipped broken.**
   `OverviewState` had an `animation_progress: f32` stepped by `tick_animation`,
   `show()` set it to `0.0`, and every draw path began
   `if progress <= 0.0 { return }`. With no clock, the first genuinely working
   build of the overview drew **a blank, un-clickable, fullscreen overlay** —
   the feature not working, not a missing polish detail. Keeping the field to
   preserve the *possibility* of a fade would have meant shipping the blank
   version in order to protect an animation that does not exist. It is gone;
   see `design-decisions.md` §520.
3. **Anything else with a `tick(dt)` is in the same position** —
   `notif_pane.rs:600` (slide-in), `login_screen.rs:419` — and is currently
   saved only by not gating its rendering on progress the way the overview did.
   They render at their final state, which looks like a missing animation
   rather than a missing feature. That is luck, not design: the next module
   that writes the obvious `if progress <= 0.0 { return }` reproduces defect 2.

The `tick`s that take an explicit `now_ms` and are driven by an *event* rather
than a frame — `osd.rs:249`, `power.rs:601`, `a11y.rs:664` — are fine and are
not part of this entry. The distinction is whether the caller has a reason to
call at all when nothing happened.

**Corrected 2026-08-22 — the gap is higher up than first written.** This entry
originally proposed adding a `wait_until(deadline)` at the socket layer. That
was wrong: **`SocketTransport` already has one.** `set_wait_timeout(Option<Duration>)`
(`gui/remote/src/socket.rs:204`) caps how long `park()` blocks, by way of
`set_read_timeout`, and it already refuses a zero duration with an explanation.
Its own doc comment even names the use case — *"Only useful to a client that has
something to do on a timer — an animation, a blinking caret"*. Nothing above it
has ever called it.

**And the event type already exists too.** `guitk::event::Event::Tick { elapsed_ms }`
is in the shared vocabulary, `gui/remote/src/input.rs` encodes and decodes it on
the wire in both directions, and **50 references to it exist across the toolkit
and the applications** — `modal.rs` (three), `grid.rs`, `asteroids`, `breakout`,
`dots`, `benchmark`, `diskimager`, `credmanager` and more all have a
`Event::Tick { elapsed_ms } =>` arm ready to receive one. A tree-wide search for
anything that *constructs* one outside a test finds nothing. The compositor
never sends a `Tick`; `EventLoop` never synthesises one.

So this is not a missing mechanism at either end. It is a **missing producer in
the middle**, with a working consumer vocabulary on one side and a working
bounded wait on the other, and no line of code joining them. That makes it
markedly cheaper to fix than first assessed, and markedly worse to leave: every
one of those 50 arms is dead code that its author had no way to notice was dead,
because a `handle_event(Event::Tick { elapsed_ms: 16 })` written by hand in a
unit test passes perfectly.

**Proper fix:** `EventLoop` synthesises the tick locally.

1. `Transport` grows `set_wait_timeout(Option<Duration>)`, defaulting to
   `Ok(())`. The default is sound rather than lax: the default `wait()` returns
   immediately, and a wait that never blocks trivially honours any bound. The
   rule is the same one `wait` already states — every transport that really
   blocks must implement it. `SocketTransport`'s inherent method becomes the
   trait impl.
2. `EventLoop` grows `wake_at(window, Instant)` / `wake_after(window, Duration)`
   and `cancel_wake(window)`. Wake-ups are **one-shot**, so an animation re-arms
   from its own handler: that makes stopping the default and continuing the
   deliberate act, which is what keeps an idle desktop idle.
3. `run` computes the nearest pending deadline each pass, bounds the wait to it,
   and delivers `Event::Tick { elapsed_ms }` — measured, not assumed — to each
   window whose deadline has passed. With no registrations it blocks for ever
   and burns nothing, exactly as now.

This is deliberately **not** a fixed-rate frame timer. A shell that wakes 60
times a second to discover it has nothing to draw is
`TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING` in a different place.

It is also deliberately **local**, not a compositor frame callback. The wire can
already carry a `Tick`, so a compositor-driven, vsync-locked version remains
open later for anything that needs to be in step with the display; but routing
every animation frame through a socket round trip to get a timer the client can
read from its own clock would put the compositor in the middle of every blinking
caret on the desktop.

Rendering also has to become re-entrant from the loop rather than from an input
event, which is the part that touches `session.rs`: `paint_chrome` is called
today only in response to something the user did.

**Owned by lane C** — `gui/window` and `gui/remote` are both in lane C's tree,
so no cross-lane request is needed.

**If never fixed:** no visual transition on the desktop can ever work, and the
1036 lines of `animations.rs` stay dead. It gets slowly worse rather than
staying still: each new module that writes an animation writes it against an
API that cannot run, and every one of them will pass its own unit tests,
because a `tick(dt)` called directly by a test advances exactly as designed.
That is the trap — the tests are not wrong, they are just answering a question
nobody in production asks.

---

**RESOLVED 2026-08-22.** `EventLoop` now synthesises the tick locally, exactly
as the three-step fix above proposed. See `design-decisions.md` §521 for the
four choices behind it (local vs. compositor, one-shot vs. repeating, on-demand
vs. fixed-rate, measured vs. assumed `elapsed_ms`) and for the verification.

**What shipped.**

- `Transport::set_wait_timeout(Option<Duration>)` (`gui/remote/src/client.rs`),
  defaulting to `Ok(())`; `Connection::set_wait_timeout` forwards it. `Socket`'s
  inherent method became the trait impl, unchanged. (This entry called the type
  `SocketTransport` above — it is actually named `Socket`, aliased to `Link` in
  `oswindow`.)
- `EventLoop::wake_at(window, Instant)`, `wake_after(window, Duration)`,
  `cancel_wake(window)`, `is_waking(window)`, `next_wakeup()`
  (`gui/window/src/lib.rs`). One-shot; arming twice replaces rather than
  accumulates; `close()` cancels.
- `EventLoop::wait()` — now public — bounds the park by the nearest deadline,
  floored at 1 ms so an already-past deadline never reaches the socket as a zero
  timeout (which the platform reads as *never* time out, the exact opposite).
  With nothing armed the bound is `None` and the loop blocks for ever, burning
  nothing, exactly as before.
- `poll()` mixes the due ticks into the ordinary pending queue, so a manual loop
  driver gets them too, not just `run()`. `elapsed_ms` is measured from the
  previous tick for that window, carried across the handler that re-armed, and
  dropped when an animation stops so a restart does not report the pause.
- `ShellSession` now parks through `EventLoop::wait` rather than
  `Connection::wait`, and exposes `events_mut()` so a shell can register its own
  wake-ups. **This was a real bypass, not a precaution**: without it the frame
  clock would have worked everywhere except in the shell, and the only symptom
  would have been an animation that stops.

Seventeen tests added across `oswindow`, `guiremote` and `desktop`; fourteen
defects were reintroduced one at a time into the real tree to prove sixteen of
them fail against the defect they name, with every touched file verified
restored byte-for-byte. `an_idle_loop_has_no_deadline` is recorded as additional
coverage rather than a regression test — no marker made it fail. Full table in
`design-decisions.md` §521.

**What this does NOT close.**

1. ~~**`gui/desktop/src/animations.rs` still has no caller.**~~ **Closed
   2026-08-22**, in the follow-on task this entry named as "where the fix is
   actually cashed in". `ShellSession::step_frame` steps the manager and the
   overview's backdrop fade from `Event::Tick`, and arms the next one-shot
   wake-up only while `anything_moving()`. `animations.rs` was changed first:
   it counted *frames* (`duration_ticks`/`current_tick`, "one tick = one
   frame"), which under a clock that is allowed to be late makes a busy moment
   silently *lengthen* every animation rather than drop frames — it now counts
   milliseconds (`duration_ms`/`elapsed_ms`, `tick(dt_ms)`). The overview's
   fade came back with the shape that makes §520 structurally impossible:
   `Option<Animation>` where `None` means *fully open*, so a caller that never
   ticks sees a finished overlay rather than a blank one. See
   `design-decisions.md` §522.
2. ~~**Rendering is still driven from input, not from the loop.**~~ **Closed
   2026-08-22** by the same change: a tick reaches `pump`'s repaint through
   `self.dirty`, and the decision to repaint is taken *before* the step so the
   frame that finishes the last animation is still painted
   (`the_frame_that_finishes_the_fade_is_still_painted`).
3. **Nothing is vsync-locked.** Still open, and deliberate — see §521 §1. An
   animation that must be in step with the display still cannot be; the wire
   can already carry a `Tick`, so a compositor frame callback remains additive
   later.
4. **Most of the shell's chrome is still not animated.** Only the overview's
   backdrop fade is wired. The start menu, calendar, Alt-Tab, notification pane
   and login screen are all drawn by the shell and so *could* be, and are not
   yet. `animate_window` and `animate_desktop_switch` are a different case and
   are not blocked on the shell at all: a window's geometry belongs to the
   compositor (§519), so the shell throws the stepped rectangles away and those
   two are for callers that render the result themselves.
