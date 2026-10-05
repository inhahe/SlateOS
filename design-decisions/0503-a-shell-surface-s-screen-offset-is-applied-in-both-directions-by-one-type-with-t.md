## 503. A shell surface's screen offset is applied in both directions by one type, with the outbound half expressed as a translation command rather than rewritten coordinates

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop shell works out where things are in whole-screen
coordinates — the taskbar is "at the bottom of a 1440-tall display". But the
compositor, when it delivers a mouse click to one of the shell's windows,
reports the position *inside that window*: a press on the start button arrives
at y ≈ 8, not y ≈ 1400. The same mismatch runs the other way for drawing. So
every click coming in needs the window's screen position added, and every
picture going out needs it subtracted. The decision is that one small type owns
both of those adjustments, and that the drawing half is done by telling the
compositor "shift everything by this much" rather than by editing each drawing
command's numbers.

**The problem.** `DesktopShell::hit_test` compares a point against
`taskbar_rect()`, `start_button_rect()`, `clock_rect()` and the popup rects,
all of which are derived from `screen_height`. A compositor client is only ever
told window-local coordinates (`guiremote::InputEvent`'s own doc says so, and
the compositor subtracts the client rect origin before sending). Feeding
delivered coordinates straight into `hit_test` falls through every chrome arm
and lands on `Hit::Desktop`; the error is exactly `screen_height -
taskbar_height`, so it is *invisible* on a fixture screen the same height as
the taskbar and grows with the display.

**Decision, part one: one type, two methods, both directions.** `Surface {
window, origin }` in `gui/desktop/src/session.rs` has `to_screen` (input,
`+origin`) and `localize` (output, `−origin`) and nothing else. Those are the
only two places in the crate where either translation is written.

*The alternative was to fix only the direction you happen to be working on*,
which is what happens naturally when input and rendering are written at
different times. It fails in a specific and nasty way: getting one right and
the other wrong yields a desktop that *draws correctly and responds in the
wrong place* — or the reverse. Both halves look plausible in isolation, and a
test of either half alone passes. Binding them to one type makes the pairing
testable: `a_point_that_hits_an_element_is_a_point_that_element_was_drawn_at`
asserts the round trip, and was verified to fail for *both* one-sided bugs by
reintroducing each separately.

*The other alternative was to re-base the shell's rectangles per surface* —
make `taskbar_rect()` return `y = 0` when asked by the panel. Rejected on two
grounds. It restores two answers to "where is the taskbar", which is the defect
the whole window-list projection work exists to remove. And it breaks
`hit_test` outright: that function decides which of two *overlapping* things a
point belongs to by comparing the popup rects against the taskbar rect, a
comparison that is only meaningful while they all live in one space.

**Decision, part two: the outbound half is `PushTranslate`/`PopTranslate`, not
a coordinate rewrite.** `localize` wraps the tree in a translation the
compositor honours (`gui/compositor/src/lib.rs`, and `guiremote` encodes it as
tag `0x08`) rather than walking the commands and adjusting each one's numbers.

- *For the rewrite:* it is self-contained — no dependency on the compositor
  implementing translation correctly — and the submitted tree has no extra
  commands in it.
- *Against it, decisively:* `RenderCommand` has a dozen variants carrying
  positions in different shapes (points, rects, glyph runs, clip regions). A
  rewriter must be extended for every variant added, and the failure mode when
  somebody forgets is that the new command silently draws in the wrong place —
  not a compile error, not a test failure, just a misplaced widget nobody
  notices until they look. The translation command has no per-variant knowledge
  to get wrong.

**And it is applied unconditionally, including for origin `(0, 0)`.** Two of
the three surfaces are screen-aligned, so a "skip the translation when it is a
no-op" branch is tempting and measurably cheaper. Rejected: it would send those
two surfaces down a code path the translated one never takes, which is exactly
the arrangement in which the two directions get to disagree without any test
noticing. `a_surface_at_the_screens_origin_is_translated_the_same_way_as_any_
other` exists to keep the branch from coming back.
