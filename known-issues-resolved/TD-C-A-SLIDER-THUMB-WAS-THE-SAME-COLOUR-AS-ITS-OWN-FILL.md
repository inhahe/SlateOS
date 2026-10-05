### TD-C-A-SLIDER-THUMB-WAS-THE-SAME-COLOUR-AS-ITS-OWN-FILL — 2026-08-24 — FIXED same day (`a03536528`)

**In short.** The touchpad settings page draws sliders (a horizontal bar you
drag a round handle along to set a number). The bar's left-hand portion — the
part showing how far along you are — was painted in the user's accent colour,
and so was the handle. Identical colours: 1.00:1 contrast, which means *no
visible difference at all*. For the whole left half of the slider's travel the
handle sat entirely on top of that fill and simply was not there; what the user
saw was one accent-coloured blob whose ragged right edge happened to be where
the value was. Fixed the day it was found.

**Where:** `gui/desktop/src/touchpad.rs`, the slider row's knob. Four other
modules drew the same control by hand — `display_settings.rs`, `notif_pane.rs`,
`osd.rs`, `mouse_settings.rs` — and disagreed about the handle's colour three
ways: three used `p.text`, `mouse_settings` used `appearance::emphasized` of
the fill, `touchpad` used the fill itself.

**How it was fixed.** `gui/desktop/src/slider.rs` now draws the control, and
all five call it. Same remedy as
`TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`, for the same reason: five
hand-drawn copies with a correct answer in one of them is the defect, and
correcting the one broken copy would have left five copies and three opinions.

**The interesting part: the fix is the *opposite* of the switch fix, and that
is not an inconsistency.** A switch knob is `readable_on` its own track,
because it is inset two pixels inside the track and the track is therefore the
only thing behind it. A slider thumb is **larger than its track** — a 10-to-14
pixel disc on a track 4 or 6 pixels tall — so most of it hangs over the card
behind the control, and its round outline (the thing that says *this is the
handle*) is read against that card, not against the fill. Applying the switch
rule here would give `readable_on(accent)` = `#11111B`, which on the stock dark
theme is 1.1:1 against a `base` card: an invisible handle with a crisp interior
nobody can see. So the thumb is `text`: 11.34:1 against the card, 1.46:1
against the fill, and the weak number is the one that costs nothing because the
fill only ever touches the thumb's *interior*.

The rule to carry forward: **ink is chosen for the background the shape's
outline is read against, and containment is what decides which background that
is.** `slider.rs` asserts the containment premise directly
(`the_thumb_overhangs_the_track_on_every_shape_the_shell_draws`) so that a
shape whose thumb fitted *inside* its track would fail rather than silently
inherit reasoning that no longer applies to it.

**This superseded a documented judgement**, `mouse_settings.rs` judgement 3
("the slider thumb is derived from its fill"), which was recorded when that
module was converted. Its instinct was right — the thumb had been a `LAVENDER`
constant named beside a `BLUE` fill, free to drift — and its background was
wrong. Both the module docs and its test now record the revision rather than
overwrite it, because the claim that outlived the change (the thumb is not a
constant chosen beside the fill) is the one worth keeping.
