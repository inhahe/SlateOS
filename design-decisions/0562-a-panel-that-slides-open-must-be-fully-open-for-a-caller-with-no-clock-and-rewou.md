## 562. A panel that slides open must be fully open for a caller with no clock, and rewound into the slide by one that has

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop's notification panel slides in from the right edge of
the screen. It was written so that "open the panel" meant "start it at zero
width and let an animation widen it" — but nothing in the tree was running that
animation, and a panel of zero width is a panel drawn entirely off the edge of
the screen. Worse, the code that decides what a click landed on measured the
same zero, so the panel was not only invisible, it was un-clickable at exactly
the place it was invisible. The decision is where "open" should land when
nobody is animating.

**The defect, concretely.** `NotificationPane::show` set the state to
`SlideIn(0.0)`. Both the drawing code (`notif_pane.rs`, `render`) and the
hit-testing code (`handle_mouse_event`) place the panel's left edge at
`screen_width - PANE_WIDTH * visibility`. At a visibility of `0.0` that is the
right-hand edge of the screen: nothing is drawn, the dimming layer behind it is
fully transparent, and a click anywhere is judged to have landed *outside* the
panel — which the panel treats as "the user wants me closed". Every one of
those is silent. There is no panic and no compile error; the panel simply never
appears.

**This is the second time.** §520 hit it in the Exposé overlay (the "show me
every window" screen), where `if progress <= 0.0 { return }` meant the first
working build drew a blank, un-dismissable, full-screen sheet. That one was
resolved by *deleting* the fade, because the shell genuinely had no frame clock
at the time. It has one now — `gui/window/src/lib.rs`'s `wake_after` synthesises
a `Tick` event, and `session.rs` routes it — so this time the animation can
actually run, and the question is only who starts it.

**Decision.** The plain verbs land immediately; a separate verb rewinds.

- `show`, `hide` and `toggle` put the panel *on* its destination —
  `Visible` or `Hidden` — with no animation. This is what every caller gets by
  default, so a caller with no frame clock (a test, an embedder driving the
  shell by hand, the login screen, a user with reduced-motion turned on) gets a
  panel that is all the way open or all the way gone. There is no state in
  which "open" means "invisible".
- `begin_slide()` is called *afterwards*, and only by a caller that owns a
  clock. It looks at where the panel now is, puts it back at the far end, and
  lets the clock carry it. `ShellSession::begin_notifications_slide` is that
  caller, and it mirrors `begin_overview_fade` line for line, including the
  reduced-motion guard that skips the rewind entirely.
- `slide_from` remembers the visibility the panel was last *drawn* at, so a
  slide reversed halfway — the user closes the panel while it is still opening —
  resumes from where it is on screen instead of snapping to the far end and
  playing the whole way back. That snap would be a visible jump in the single
  case the animation exists to smooth.

**Alternative considered: keep `show` starting the slide, and require every
caller to have a clock.** Rejected for the reason §520 gives. The property that
matters is not "the animation is convenient to start" but "a feature does not
depend on an animation to be usable" — and a rule enforced by documentation is
a rule that the next orphan module will break in the same silent way. Making
the *default* correct means the failure mode of forgetting `begin_slide` is a
panel that appears instantly, which is a cosmetic loss rather than a feature
that does not exist.

**Alternative considered: delete the slide, as §520 deleted the fade.**
Rejected because the premise changed. §520 deleted an animation that nothing
could run; the shell now has a frame clock with a live caller and a tick route,
so the slide is a working animation rather than a decorative comment.

**A consequence worth naming: the "it closed" event moved.** It used to be
emitted when the slide-out *finished*. That makes it fire once for a session
that animates and — since a non-animating session never reaches the end of a
slide — never at all for one that does not. It is now emitted by the `hide`
gesture itself, which is both when the user actually closed the panel and the
one place that happens exactly once either way.

**And a trap the session side has to avoid.** The session decides "a gesture
opened or closed the panel" by comparing the open flag before and after each
event. But the slide *ends* by changing that same flag — a slide-out finishes at
`Hidden` — and that happens inside the frame-tick handler. Sampling around
ticks as well would read the end of every slide as a fresh gesture and restart
it, for ever. `dispatch` therefore skips the comparison for `Event::Tick`, and
`a_frame_tick_does_not_restart_the_slide_it_just_finished` is the test that
holds it there.

**Where it lives.** `gui/desktop/src/notif_pane.rs` (`show`, `hide`, `toggle`,
`begin_slide`, `is_sliding`, `slide_from`, `tick`, and the eleven tests naming
these terms), `gui/desktop/src/session.rs`
(`begin_notifications_slide`, the `is_tick` guard in `dispatch`,
`anything_moving`, `step_frame`), `gui/desktop/src/lib.rs`
(`toggle_notifications`).
