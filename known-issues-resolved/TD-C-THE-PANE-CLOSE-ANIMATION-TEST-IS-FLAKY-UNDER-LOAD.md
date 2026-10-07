## TD-C-THE-PANE-CLOSE-ANIMATION-TEST-IS-FLAKY-UNDER-LOAD -- FIXED 2026-09-26

**Status:** FIXED 2026-09-26 (lane C). Filed among the resolved on 2026-10-05; what follows is the entry as it stood.

**Date:** 2026-09-14. **Lane:** C. **FIXED 2026-09-26** -- a real-time tick
inside the same pump, which the "ruled out" list below wrongly excluded; see
**Found, 2026-09-26** at the end.

**In short:** one test in the desktop suite fails occasionally and passes on a
re-run, which is the worst kind of failure: it teaches whoever sees it to run
the suite again instead of reading it. Nothing is wrong with the program -- the
test asks a question about timing that the test harness cannot answer reliably
when the machine is busy.

**What it is.** `session::tests::closing_the_pane_slides_it_out_and_it_stays_out`
(`gui/desktop/src/session/tests.rs:3119`), asserting
`"the close snapped instead of sliding"`.

The test opens the notification pane, steps 200 frames so the open animation
finishes, sends the close key, pumps, and then asserts the pane is **still
visible** -- that is, that closing *starts an animation* rather than vanishing.
It is a good thing to test and the assertion is the right one.

**What was measured, 2026-09-14:**

| run | result |
|---|---|
| full `-p desktop` suite, 35.9 s wall (compiling, machine loaded) | **FAILED** |
| same test alone, three consecutive runs | passed, passed, passed |
| full `-p desktop` suite again, 12.9 s wall | passed (2976) |

**Where the nondeterminism is not.** The pane animation is driven by an
explicit `pane.tick(dt)` with the delta handed in, and there is no
`Instant::now` or `SystemTime::now` anywhere in `notif_pane.rs`. So the
animation itself is deterministic given a frame count; this is not an
animation-on-wall-clock bug.

**Where it is not, corrected 2026-09-14.** The first version of this entry said
the cause was "how many frames the shell has processed by the time `pump()`
returns". **That is wrong and was written without reading `pump`.** `pump`
dispatches queued events and reconciles the window and tray revisions; it does
not call `step_frame` and does not advance a single animation. `step_frame` is
the only animator and takes its delta as an argument. So both halves are
deterministic and the frame count is fixed by the test.

**What the failure actually requires.** `is_visible()` is `!matches!(self,
Hidden)` and is true throughout the slide-out, so the assertion fails *only* if
the pane is `Hidden` at that moment -- not merely further along. A close that
had raced ahead would still be `SlideOut(p)` and still visible. So this is not
an animation that got too far; it is a pane that was never open, or was closed
twice. That points at input delivery or event coalescing in the harness, not at
animation timing, and it means the "obvious" fix below would not have helped.

**Still unknown, after twelve attempts.** It has not reproduced once:

| attempt | result |
|---|---|
| the named test alone, 3 runs | passed |
| full suite, default threads, 6 runs | passed |
| full suite, `--test-threads=1` (the gate's mode) | passed |
| full suite, serial, with `XDG_CONFIG_HOME` and `HOME` pointed at an empty probe dir (the gate's exact environment) | passed |

**Ruled out, by reading rather than by guessing:**

* *Frames advancing inside `pump`* -- it dispatches events and reconciles
  revisions; it never calls `step_frame`. **Wrong, and the whole cause** --
  see "Found, 2026-09-26" below: `pump` dispatches the ticks the loop
  synthesises, and `dispatch` answers a tick with `step_frame`.
* *An animation that raced ahead* -- `is_visible()` is true throughout
  `SlideOut`, so only `Hidden` fails the assertion.
* *`reduced_motion`* -- this was the most promising lead, because
  `begin_notifications_slide` returns early when it is set and the pane then
  lands immediately, which is *exactly* what "snapped instead of sliding"
  describes. But `AnimationManager::new()` defaults it to false, nothing loads
  it from configuration, and the only two tests that set it do so on their own
  session. Disproved.
* *Serial ordering and a leaked `XDG_CONFIG_HOME`* -- both reproduced above
  without failing.

Two confident causes have been written into this entry and removed again. The
useful residue is the list above: whoever sees this next should not re-derive
it, and should distrust the next tidy explanation, including their own.

**What the proper fix looks like** -- *once the cause is known.* The shape that
survives either diagnosis is to assert over a bounded number of frames the test
steps itself: send the close, step one frame, assert visible; step until not
visible and assert that took more than one frame. But note this does **not**
fix the failure described above, because the observed state was `Hidden`
immediately, and stepping fewer frames cannot make an unopened pane open. Do
not fix it by loosening the assertion either -- "eventually invisible" is true
of a snap, which is the thing it exists to catch.

**Seen once for certain. A second, unnamed failure may or may not be this.**
The pre-push scratch-config gate refused a push with
`desktop: its own tests did not pass, so this says nothing` -- and **that log
named no test**, which is what prompted the gate fix below. Attributing it to
this entry was an assumption, made because this test had failed an hour
earlier; it is recorded here as unverified rather than as a second sighting.
What *is* established either way: a failing test anywhere in `gui/desktop`
stops lane C publishing, and the crate has 2,978 tests for one to hide in.

That refusal also exposed a second, separate problem, now fixed: **the gate did
not say which test failed.** The four-line log it keeps held nothing but "FAIL
desktop", so a push blocked by somebody else's flake gave its reader no way to
look, only to re-run -- which is exactly how a flake becomes permanent.
`check-scratch-config.py` now names the failing tests, with three self-test
cases covering the message itself.

**Priority, revised:** worth fixing properly the next time it is seen, rather
than deferring indefinitely. The fix is described above; the tempting wrong one
is still wrong.

**Found, 2026-09-26.** `pump` does not call `step_frame` itself, but it hands
`dispatch` every event `EventLoop::poll` returns, and `poll` *synthesises* an
`Event::Tick` for any wake-up that has come due -- which `dispatch` answers with
`step_frame(elapsed_ms)`, the elapsed time being real. The close key arms a
wake-up 16 ms out (`begin_notifications_slide` -> `arm_next_frame`) *inside the
same pump*, so a test thread descheduled for longer than 16 ms before the pump's
next `poll` is handed a tick in that pump, and one descheduled for longer than
200 ms -- the pane's whole slide, `anim_speed` 5.0 -- finds the slide finished:
`Hidden`, exactly the state observed. The "drain" pump added to the test before
did not help, because the wake-up that fires is the one the close itself arms.
Nothing was wrong with the desktop: the time really passed, and a late frame
*should* finish a slide (`a_late_pump_moves_an_animation_on_by_the_time_that_passed`
pins that, by sleeping past the slide).

Fixed in the tests: `pump` is now its two halves -- reading and dispatching, then
`finish_batch` (the repaint, the grabs) -- and a test that looks at an animation
part-way through hands its key over with `deliver` (dispatch, then
`finish_batch`; no poll, so no clock). `frame` delivers its tick the same way.
Eight more tests had the same exposure and use it too: the pane's opening slide,
the overview's fade after Super+Tab (six) and the volume overlay's first frame --
and every user of `frame`,
whose own re-armed wake-up could ride along behind the tick it sent. The
assertion was not loosened: the close is now checked to land on exactly
`SlideOut(0.0)`, one frame to be part of the way out, and the rest to reach
`Hidden`.
