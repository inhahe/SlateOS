## 834. Selection is a change of colour, not of weight

**Date:** 2026-09-12
**Lane:** C
**Decided by:** Operator (answering C-Q13; Claude recommended A and the operator took it, then corrected the thickness)

**In short:** when you pick a row in a list, its outline turns blue-green. It
does not also get thicker. The operator chose the "everything selected takes the
accent" option and then rejected the part of the mock-up that drew the selected
outline at double weight — colour alone carries it.

**Why the thinner one is right, beyond taste.** A 2px outline on a 1px layout
costs a pixel. A row that grows when selected shifts everything below it, so a
list twitches as the selection travels down it, and a click target moves under
the pointer between frames. Colour changes nothing about geometry.

It is also one signal for one fact. Colour *and* weight says "selected" twice,
which is the same objection as marking a link with colour alone was the opposite
of — there the second signal was necessary because colour is not perceivable to
everyone; here the outline is present either way and only its colour changes, so
the weight adds nothing a reader could use.

**The implementation already did this**, which is worth recording because it
means the mock-up was wrong rather than the code. `Palette::push_surface_radii`
has always stroked at `line_width: 1.0` for every `Surface`, selected or not.
Only `scripts/contrast-explorer.html` drew selection at 2px, so the operator was
rejecting a picture of a thing that was never built.

**What this settles for the shell.** The 158 `gui/desktop` sites blocked on
C-Q13 can now be converted: `Surface::Selected` is an accent outline, everywhere,
at one pixel. The two shell tests that encode the older accent policy —
`launcher::the_accent_marks_where_you_are_and_never_what_a_thing_is` and
`clipboard_viewer::only_the_active_filter_tab_follows_the_accent` — are now
superseded by an operator decision and should be rewritten to the new rule
rather than preserved.
