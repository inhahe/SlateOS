## 839. A caret's width is an appearance setting, not an input one

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous).

**In short:** the setting for how wide the text cursor is drawn existed and
did nothing -- it lived in a module nothing constructs. Giving it a live home
meant choosing between the crate that holds the *keyboard* accessibility
settings and the crate that holds everything the toolkit paints with. It goes
with the painting.

**The precedent that did not fit.**
`TD-C-STICKY-FILTER-AND-MOUSE-KEYS-ARE-BUILT-TESTED-AND-CONNECTED-TO-NOTHING`
fixed the same class of defect by moving sticky keys, filter keys and mouse
keys into `gui/inputsettings`, and the obvious move was to follow it. But that
crate was chosen because the *compositor* needs those settings: they change
what a keystroke means, and the compositor is where a keystroke is handled. A
caret is not handled anywhere; it is drawn, by `guitk`, whose settings arrive
through `appearance::AppearanceSettings`. Following the precedent would have
put the value in a crate the drawing code has no reason to read.

**The decision.** `AppearanceSettings::caret_width_scale`, a multiplier on the
toolkit's own `CARET_WIDTH`, clamped to 0.5-4.0 and written under
`accessibility:` in `appearance.yaml`. `guitk::style::Style` carries the
resolved width the same way it carries `selection_bg` and `foreground`: the
caller has the settings, the toolkit does not, and a `Style` is already the
thing that says what a widget looks like. `InputDialog` takes it through
`with_caret_width`, because a dialog is constructed rather than styled.

**Clamped at 4x, where the dead module said 5.** Past about four times, a
caret stops being a caret and starts covering the character after it. The old
limit was never exercised by anything, so there was no behaviour to preserve.

**The test is the point, and it is not a round-trip.** `a11y.rs` had a passing
round-trip test for its own copy of this setting -- store it, read it back,
check the clamp -- and that is exactly why nobody noticed it was wired to
nothing. A test that proves a setting is *stored* proves nothing about whether
it is *read*. The new test asserts that a caller can get from the settings to
a width in pixels, which is the step that did not exist.

**What this does not fix.** `focus_indicator` and `screen_reader` are still
dead, and the screen reader is not merely unwired -- the feature does not
exist. Those stay on the `TD-C-THE-ACCESSIBILITY-CONFIG-IS-A-DEAD-PARALLEL-COPY`
entry.
