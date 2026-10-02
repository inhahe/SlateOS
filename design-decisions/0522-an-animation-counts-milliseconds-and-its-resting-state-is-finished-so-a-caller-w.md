## 522. An animation counts milliseconds, and its resting state is *finished* — so a caller with no clock still gets a working screen

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop shell had an animation module (fades, slides, window
open/close) that nothing called, because until §521 there was no clock to step
it. Wiring it up turned out to need two changes to the animation code itself
before a single line of wiring was written. First, animations were counting
*frames* rather than *time*, which under a clock that is allowed to be late
means a busy moment silently makes every animation slower rather than dropping
frames. Second, an animation's "not started" state was its *invisible* state, so
any part of the program that drew an animated thing without running a clock drew
nothing — which is exactly the bug §520 had to delete the overview's fade over.
Both are now inverted: animations count milliseconds, and "no animation running"
means *fully drawn*.

### What was wrong with counting frames

`animations.rs` was written with `duration_ticks` and `current_tick`, and a
`tick()` that took no argument. Its module doc said "one tick = one frame". That
is a coherent model only under a fixed-rate clock that never misses — and §521
deliberately does not provide one. The shell's clock is a one-shot deadline on
the loop's own park, so a frame arrives *at least* 16 ms after the last one and
possibly much later, whenever the shell was busy answering input.

Under a frame count, a late frame does not drop anything. It makes the animation
*take longer*. A 200 ms fade on an idle machine is a 200 ms fade; the same fade
while the shell is answering a burst of window-list updates is a 300 ms fade,
and nothing anywhere reports that. It is a defect with no symptom other than
"the desktop feels inconsistent", which is the hardest kind to ever find.

It was also inexpressible. `roadmap-detailed.md`'s Tier 3 appearance settings
include an `animation-duration-ms` knob. There is no honest conversion from
milliseconds to ticks without knowing the frame rate, and the frame rate is by
construction not a constant.

So `Animation` now holds `duration_ms` and `elapsed_ms`, and `tick(dt_ms)` takes
the wall time the frame clock measured. A step that lands past the end lands
*on* the end rather than overshooting, and a step long enough to span several
passes of a looping animation carries the remainder through rather than losing
it. The property the tests assert is the one that matters: an animation driven
at 125 Hz and the same animation driven at 20 Hz finish at the same *time*, with
different numbers of frames.

### The rule that keeps §520 from happening again

§520 deleted the overview's open fade. The reason is worth restating because it
is a general trap, not an overview one: the fade had an `animation_progress`
that `show()` set to `0.0`, a `tick_animation(dt)` that the caller was supposed
to run every frame, and draw paths that began `if progress <= 0.0 { return }`.
Nothing called `tick_animation`, and at the time nothing could. The result was a
fullscreen overlay that was blank, took every click, and stayed that way.

The tempting reading is "the bug was the missing clock". It was not. The bug was
that **the state an animation starts in was the state in which the feature does
not work**, so the clock was load-bearing for correctness rather than for
polish. Any caller that does not run a clock — a layout test, a headless render
pass, an embedder driving the shell by hand, a future code path nobody has
thought of — gets a broken screen rather than a still one.

The rule adopted here, and applied to the restored fade:

1. **The resting state of an animation is its *finished* state.** The overview's
   fade is `Option<Animation>`, and `None` means fully open. `show()` sets it to
   `None`. A caller that never ticks sees the completed overlay.
2. **Starting an animation is the deliberate act, and only a caller that owns a
   clock does it.** `begin_fade` is separate from `show`, and its only caller is
   `ShellSession`, which is the thing that has the frame clock. Everything else
   that opens the overview gets the working version for free by doing nothing.
3. **Nothing gates drawing or hit-testing on progress.** The fade scales the
   backdrop's alpha and touches nothing else, so even a fade begun and then
   frozen at zero leaves every card drawn at full strength and every click
   landing where it should. This is asserted directly:
   `an_overview_whose_fade_never_runs_is_still_drawn_and_still_clickable`
   begins a fade, never advances it, and requires both a drawn overlay and an
   answered click.
4. **A finished animation is indistinguishable from one that never ran.**
   `tick_fade` sets the field back to `None` on completion, so no downstream
   reader can come to depend on "has faded" versus "was never faded".

The alternative — keep a single `progress: f32` and simply start it at `1.0`
unless someone asks otherwise — was rejected because it makes the safe state a
*value* rather than a *shape*. A future edit that resets progress in the wrong
place reintroduces §520 silently. With `Option`, "no animation" and "an
animation at zero" are different kinds of thing, and the kind that means
"nothing is running" is the one that draws correctly.

### What the shell can and cannot animate

`AnimationManager::tick` returns a rectangle per animated window, and the shell
throws them away. That is not an oversight: **a window's geometry belongs to the
compositor**, which owns the scene, the z-stack and the surfaces. The shell
cannot move a window by drawing it somewhere else, because it does not draw
other clients' windows at all (§519 — the window list *reports* rectangles and
nothing lets a shell set one). `animate_window` is therefore for a caller that
renders the result itself, and is documented as such; the same is true of
`animate_desktop_switch`'s slide offset.

What the shell genuinely draws, and so can genuinely animate, is its own chrome:
the overview overlay, the start menu, the calendar, Alt-Tab, the notification
pane, the login screen. The overview's backdrop fade is the first of these to be
wired, and it is the one §520 removed, so the loop opened there is now closed.

`ShellRequest::SwitchDesktop` deliberately does **not** start the slide. The
offset it produces has no reader today, and starting it would repaint the whole
chrome sixty times a second for a fifth of a second to change nothing on screen
— a heartbeat nobody listens to, which is the exact failure this entry exists to
avoid repeating.

### Verification: every test was proved to be a regression test

Seventeen tests were added. Each was proved by reintroducing, one at a time, the
defect it names — as a guarded reversible source patch — and requiring a
deterministic failure that names the test back. Nineteen markers, all caught:

| Marker (the defect reintroduced) | Test that caught it |
|---|---|
| `showstartsthefade` — `show()` starts the fade, so an un-ticked overview is invisible | `an_overview_that_is_never_ticked_is_fully_open` |
| `fadegatesdrawing` — `render_overview` returns nothing at zero progress (§520 verbatim) | `a_fade_that_has_begun_still_leaves_every_card_drawn` |
| `fadegateshittesting` — `on_mouse_click` refuses input at zero progress | `an_overview_whose_fade_never_runs_is_still_drawn_and_still_clickable` |
| `thefadeisnotdrawn` — backdrop alpha ignores the fade | `the_backdrop_is_dimmer_part_way_through_the_fade` |
| `afinishedfadelingers` — a completed fade is not cleared | `a_finished_fade_is_indistinguishable_from_one_that_never_ran`, `a_frame_advances_the_fade_and_the_last_one_stops_asking_for_more` |
| `thefadenevergivesup` — `tick_fade` always asks for another frame | `a_fade_reports_when_it_wants_another_frame_and_when_it_does_not` |
| `zerofadeisaonemsfade` — `begin_fade(0)` makes a 1 ms fade | `a_zero_length_fade_is_no_fade_rather_than_a_one_millisecond_one` |
| `hidekeepsthefade` — `hide()` leaves the fade running | `hiding_drops_the_fade_so_the_next_opening_does_not_inherit_it`, `closing_the_overview_takes_its_fade_with_it` |
| `endingafadedoesnothing` — `end_fade` is a no-op | `turning_reduced_motion_on_mid_fade_lands_on_fully_open` |
| `openingstartsnofade` — the session arms a frame but starts no fade | `opening_the_overview_asks_for_a_frame` |
| `thefadeisnotmovement` — `anything_moving` forgets the overview | `opening_the_overview_asks_for_a_frame` |
| `animationsarenotmovement` — `anything_moving` forgets the manager | `a_window_animation_runs_off_the_same_clock` |
| `startingananimationdoesnotarm` — `animate_window` leaves the clock unarmed | `a_window_animation_runs_off_the_same_clock` |
| `ticksarenothandled` — `Event::Tick` falls through to the catch-all arm | `a_frame_advances_the_fade_and_the_last_one_stops_asking_for_more` |
| `thefadeisnotstepped` — the frame steps the manager but not the fade | `a_frame_advances_the_fade_and_the_last_one_stops_asking_for_more` |
| `armunconditionally` — every frame arms the next, moving or not | `a_frame_with_nothing_moving_asks_for_no_more_frames` |
| `afixedratetimerinstead` — the shell arms a wake-up at start-up | `an_idle_desktop_asks_for_no_frames` |
| `paintdecidedafterthestep` — repaint decided after the step, dropping the last frame | `the_frame_that_finishes_the_fade_is_still_painted` |
| `reducedmotionstillfades` — reduced motion reaches the paint but not the clock | `reduced_motion_opens_the_overview_without_a_fade_and_without_a_clock` |

Every one of the seventeen tests earned at least one marker; none had to be
recorded as merely additional coverage. Both patched files were verified
restored byte-for-byte by SHA-256 after each batch.

**A note on the tool, again.** §521 recorded that the prover's reversal lives in
a `finally`, which does not cover the process being killed, and added a `heal()`
pass that undoes a marker left applied. This run found the *next* hole in the
same tool: the reversal was a reverse search-and-replace, and one marker's
patched text (`return false;`) occurred twice in the file afterwards, so the
undo silently did nothing and left the defect in the tree. Worse, the guard
added to detect exactly that threw *inside* the block whose `finally` was the
thing that could not undo it. The reversal is now a **byte snapshot of the file
taken before patching**, written back unconditionally — which cannot fail on
non-uniqueness because it does not search for anything. The general lesson: an
undo that has to *find* what it is undoing is a guess; an undo that restores
what it saved is not.
