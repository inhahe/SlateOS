## 521. The frame clock is a local, one-shot deadline on the loop's own park — not a compositor callback, not a fixed-rate timer

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Nothing on the desktop could move by itself. A menu could not
slide open, a caret could not blink, a progress spinner could not spin — not
because that code was missing, but because there was no heartbeat to step it
with. The shell woke only when the user did something. This adds the heartbeat:
a window can ask the event loop to wake it once, at a time it names, and gets
an `Event::Tick` carrying how long really passed. The four choices below are
about *where* the heartbeat comes from, *how often* it beats, *who stops it*,
and *what number* the tick carries.

### What was actually missing

The surprising part, and the reason this entry exists at all: **neither end of
the mechanism was missing.** The bounded wait already existed —
`Socket::set_wait_timeout(Option<Duration>)` (`gui/remote/src/socket.rs`)
capped `park()`'s `set_read_timeout`, refused a zero duration with an
explanation, and its own doc comment named the use case: *"Only useful to a
client that has something to do on a timer — an animation, a blinking caret"*.
The event already existed too — `guitk::event::Event::Tick { elapsed_ms }` is in
the shared vocabulary, `gui/remote/src/input.rs` encodes and decodes it on the
wire in both directions, and **50 references to it exist across the toolkit and
the applications**: `modal.rs` (three), `grid.rs`, `asteroids`, `breakout`,
`dots`, `benchmark`, `diskimager`, `credmanager` and more all had an
`Event::Tick { elapsed_ms } =>` arm ready to receive one.

A tree-wide search for anything that *constructed* one outside a test found
nothing. So this was a **missing producer in the middle**, with a working
consumer vocabulary on one side and a working bounded wait on the other, and no
line of code joining them. Every one of those 50 arms was dead code its author
had no way to notice was dead, because `handle_event(Event::Tick { elapsed_ms: 16 })`
written by hand in a unit test passes perfectly. See `known-issues.md` →
`TD-C-THE-SHELL-HAS-NO-FRAME-CLOCK`, and §520's closing note for the same shape
one task earlier.

### 1. The tick is synthesised locally, not requested from the compositor

`EventLoop` reads its own clock, bounds its own park, and constructs the
`Event::Tick` itself. The compositor is not involved and does not know an
animation is running.

*Alternative rejected:* a compositor frame callback — the client asks for a
frame, the compositor answers at vsync, the client draws. This is what Wayland
does and it is genuinely better for one thing: an animation that must be *in
step with the display* cannot be, from a local clock. That option stays open —
the wire can already carry a `Tick` in both directions, so adding
`RequestBody::RequestFrame` later is additive and breaks nothing.

*Why not now:* it would put the compositor in the middle of every blinking caret
on the desktop. A text field asking "wake me in 500 ms" would become a socket
round trip, a scheduling decision in another process, and a wake-up of the
compositor — to obtain a number the asking process could have read from its own
clock. The cost is paid per animation per frame, by every client, including the
ones whose animation has nothing to do with the display's refresh.

### 2. Wake-ups are one-shot, so an animation re-arms every frame

`wake_at(window, deadline)` fires exactly once. An animation that wants a next
frame calls `wake_after` again from inside its own tick handler.

*Alternative rejected:* a repeating interval timer — `every(window, 16ms)`,
cancelled when the animation ends. Fewer calls, and it reads more naturally.

*Why not:* it inverts the default. With a repeating timer, *continuing* is free
and *stopping* is the thing you must remember; a handler that returns early, an
animation that finishes on a branch nobody tested, a window torn down by a path
that forgets to cancel — each leaves a timer waking the process sixty times a
second for ever, and none of them looks like a bug in code review. One-shot
makes stopping the default and continuing the deliberate act, which is exactly
the property that keeps an idle desktop idle. The cost is one extra call per
frame in the handler that is already running.

*Corollary made explicit:* arming a window that already has a wake-up
**replaces** it rather than adding a second, so a handler that re-arms
unconditionally cannot accumulate timers. And `close()` cancels — a destroyed
window's wake-up would otherwise hold the process at a frame rate for a window
nobody can see, with every tick landing in `unrouted`.

### 3. Not a fixed-rate frame timer

With no wake-ups registered, `next_wakeup()` is `None`, the park is unbounded,
and the loop blocks for ever exactly as before. Nothing beats unless something
asked.

*Alternative rejected:* run the loop at 60 Hz and deliver a tick every pass,
letting each window decide whether it has anything to do. That is how most game
loops are written and it makes the API smaller — no `wake_at`, no `cancel_wake`.

*Why not:* a shell that wakes sixty times a second to discover it has nothing to
draw is `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING` in a different place. That
entry is already filed as debt against the compositor; introducing the same
tradeoff, resolved the same wrong way, in the client library one directory over
would be building the bug we are tracking.

### 4. `elapsed_ms` is measured, and it partitions wall time

The tick carries `now − since`, where `since` is the moment of the *previous
tick for that window* — carried across the handler that re-armed — not the
moment the wake-up was armed, and not the interval that was requested.

*Alternative rejected (a): report the requested interval.* Simplest, and wrong
in a way that hides itself: an animation would be told it is running at its
nominal rate no matter how far behind it had actually fallen, so the symptom of
a slow frame becomes a slow animation rather than a dropped one — visible to the
user, invisible to every measurement.

*Alternative rejected (b): measure from the moment of arming.* This is the
tempting one, and it looks right. It loses the time the handler itself spent:
arm for 16 ms, handler takes 4 ms, every tick reports 16 while 20 ms of wall
time passed. That matters more than 25% because **every consumer in the tree
accumulates `elapsed_ms`** — `benchmark` adds it to a progress total,
`credmanager` adds it to a clock, `login` steps a timeout with it. A delta that
omits the handler does not merely run slow; it drifts from the wall clock
without bound, and a clock built that way loses minutes per hour.

*The exception that is not an inconsistency:* when an animation **stops** — a
tick is delivered and the handler does not re-arm — the reference point is
dropped. Restarting later reports the length of the first new frame, not the
length of the pause. Without that, a paused game would resume by advancing every
object by however long it was paused. The rule is therefore "partition the time
the animation was *running*", which is what a consumer accumulating dt actually
wants, and cancelling is the explicit way to say so.

### The trait default is sound, not lax

`Transport::set_wait_timeout` defaults to `Ok(())`. That looks like a transport
may ignore a deadline it was given, but the default `wait()` returns
immediately, and a wait that never blocks trivially returns within any bound.
The obligation is the one `wait` already carries: a transport that really blocks
must implement this too. `Socket`'s inherent method became the trait
impl, unchanged.

### What made this testable without sleeping

The policy — expiry, ordering, the elapsed arithmetic, the dropped reference
point — lives in `EventLoop::due_at(&mut self, now: Instant)`, and `wake_at`'s
cold-start reference comes from a private `arm(window, deadline, now)`. Both
take the clock as a parameter, so every rule above is checked against synthetic
instants derived from one `Instant::now()` base. A test that has to sleep to
observe a deadline is a test that will eventually be flaky about it, and a
flaky timing test is worse than none: it trains its reader to re-run rather than
to look.

The one thing a synthetic clock cannot check — that the park really is bounded
and really is a park — is proved in `guiremote::socket` against a live, silent
peer, with a lower bound as well as an upper one, so a `wait` that ignored the
timeout by returning at once fails just as a `wait` that ignored it by blocking
would.

### Verification: every test was proved to be a regression test

Seventeen tests were added. A test named after a defect is not evidence it
catches that defect — the standing doctrine here (see §520) is that **a test
which does not fail against a marker naming its own defect is not a regression
test for it, whatever it is called.** So each defect was put *back* into the
real tree, one at a time, the affected crates' suites were run, and the defect
was taken straight back out. Each patch was guarded twice: the string being
replaced had to occur exactly once, and the reversal had to restore the file
byte for byte (SHA-256 against a baseline taken before the first patch).

| Defect reintroduced | Tests that failed |
|---|---|
| the bounded wait exists, `Event::Tick` exists, nothing joins them | `a_tick_reaches_the_application_through_the_ordinary_event_path`, `a_tick_for_a_window_this_loop_does_not_own_is_counted_not_delivered` |
| wake-ups repeat instead of being one-shot | `a_wake_up_fires_once_and_is_then_spent`, `an_animation_that_stopped_does_not_report_the_pause_when_it_starts_again` |
| `elapsed_ms` reports the interval asked for, not the one that passed | `the_tick_reports_the_time_that_passed_not_the_time_that_was_asked_for`, `the_deltas_of_a_running_animation_partition_the_time_it_ran_for` |
| the reference point is not carried across the handler | `the_deltas_of_a_running_animation_partition_the_time_it_ran_for` |
| the reference point outlives the animation it belonged to | `an_animation_that_stopped_does_not_report_the_pause_when_it_starts_again` |
| every deadline is due, always | `a_deadline_not_yet_reached_does_not_fire`, `a_registered_wake_up_bounds_the_park`, `the_shell_s_park_is_bounded_by_a_wake_up_it_registered` |
| arming twice grows a second timer rather than replacing the first | `arming_a_window_twice_replaces_rather_than_accumulates` |
| several deadlines due at once fire in registration order | `several_deadlines_due_at_once_fire_earliest_first` |
| cancelling one window's wake-up cancels everybody's | `cancelling_a_wake_up_stops_that_window_and_no_other` |
| the park is never bounded — the defect this whole entry is about | `an_idle_loop_parks_with_no_bound_at_all`, `a_registered_wake_up_bounds_the_park`, `a_deadline_already_past_still_bounds_the_park_by_something_nonzero`, `the_shell_s_park_is_bounded_by_a_wake_up_it_registered` |
| a deadline already past yields a **zero** bound, which the platform reads as "never time out" | `a_deadline_already_past_still_bounds_the_park_by_something_nonzero` |
| a destroyed window keeps its wake-up | `closing_a_window_withdraws_its_wake_up` |
| the socket stores a token bound instead of the one it was given | `a_bounded_wait_parks_for_the_bound_and_then_comes_back` |
| the shell parks through the connection, past every deadline it registered | `the_shell_s_park_is_bounded_by_a_wake_up_it_registered` |

Fourteen defects, sixteen of the seventeen tests earned. **`an_idle_loop_has_no_deadline`
is recorded honestly as additional coverage, not as a regression test** — no
marker made it fail. It asserts that a loop nobody armed reports no deadline,
which is the *absence* of a behaviour; the defect that would break it is the
park-level one, and `an_idle_loop_parks_with_no_bound_at_all` is the test that
actually catches that.

Two of the markers were worth the exercise on their own. `shellparksunbounded`
is not hypothetical: `ShellSession::run` really did call `Connection::wait`
rather than `EventLoop::wait`, so the frame clock would have worked everywhere
except in the shell — the one place this entry exists for — and the only symptom
would have been an animation that stops. It was found by grepping for the bypass
*because* the marker list demanded one, and fixing it is why `EventLoop::wait`
is public and `ShellSession::events_mut` exists. And `everydeadlineisdue` shows
why "does it fire?" is not enough on its own: a wake-up that is always due still
delivers ticks, still animates, and only fails the *negative* tests.

*Note on the tool, for whoever writes the next one.* The reversal lives in a
`finally`, which covers an exception but not the process being killed — a run
that was interrupted mid-marker left the defect in `lib.rs`, where it looks
exactly like a hand edit. The prover now heals that on startup: a marker whose
bad side is present exactly once and whose good side is absent is unambiguously
one that was left applied, so no journal file is needed. A prover that damages
the tree when interrupted is worse than no prover.
