## 1431. The toolkit's slider: a press on the track jumps there, the wheel only while it has the keyboard, and a thumb is a 24-pixel target however small it is drawn

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit has a slider now (`guitk::slider`), and three things
about how it behaves could reasonably have gone the other way. Pressing on the
track away from the thumb moves the thumb to where you pressed, rather than
stepping it towards the press a page at a time. Turning the mouse wheel over a
slider changes it only while the slider has the keyboard, so scrolling a page
of settings does not change every slider it passes under the pointer. And the
part you can take hold of is at least 24 pixels across, even where the thumb is
drawn 12 and the track 4, so nobody has to aim at a line to move it.

**1. A press on the track jumps there.** The alternatives:

| Option | What a press on the track does |
|---|---|
| **Jump** (chosen) | the thumb goes to the press and is held there, so the press can become a drag |
| Page | the thumb steps a page towards the press, repeating while held (the Win32 trackbar) |

`roadmap-detailed.md` names "the click-to-jump region along its track", and it
is what GTK ("primary button warps slider"), the web's range input and WinUI
do. Paging is precise for a keyboardless user who wants exactly one page, but
the keyboard already gives that (Page Up and Page Down), and paging makes the
common gesture -- press where you want it and drag from there -- impossible.

**2. The wheel only while the slider has the keyboard.** The alternatives:

| Option | What a wheel turn over a slider does |
|---|---|
| **Only when focused** (chosen) | nothing unless the slider has the keyboard; then one notch is one step |
| Whenever hovered | changes the slider the pointer is over (GTK 3's scale) |
| Never | the wheel is not the slider's at all (Chrome's range input) |

Hovered is the classic trap: a settings page scrolled with the wheel passes
its sliders under a resting pointer, and each one it passes changes -- the
scroll stops and the volume goes to zero. Never throws away a fine control
for the user who has clicked into the slider. Focused keeps it for them and
takes it from nobody else. The slider does not know focus, so the rule is
where it can be kept: `handle_mouse` leaves the wheel alone, and `wheel` is a
separate call a host makes only while the slider has the keyboard.

**3. A handle is a 24-pixel target, an edge a 3-pixel margin.** `guitk::grab`
states both. 24 is WCAG 2.2's success criterion 2.5.8, *Target Size
(Minimum)*, which is the nearest thing to an agreed floor for a pointer
target; a slider's drawn size (a 12-pixel thumb on a 4-pixel track) is well
under it and does not need to be larger to be *seen*. An edge between two
areas cannot grow like that -- the panes on both sides are clickable -- so a
divider keeps the splitter's 3 pixels either side. A scrollbar's thumb is the
third case: it runs in a track the scrollbar owns, so it grows along the track
(a press just past its end takes hold of it rather than paging) but not across
it, where the list is (`grab::in_track`); the scrollbar stays the width every
desktop draws one. Where two handles' regions overlap, the press goes to the
one drawn nearer. The numbers are in logical pixels, so they scale with
everything else on a dense display.

**What it reports** uses the colour picker's three words, so a host handles
both alike: `Changed` while a drag moves the value (show it, do not save it),
`Confirmed` when a drag is let go or a key or the wheel moves it (save it),
`Cancelled` with the value it went back to when Escape abandons a drag. A drag
that ends where it began confirms nothing, so a host that saves on `Confirmed`
does not rewrite a file for a click that changed nothing.

**Where it bites:** `gui/toolkit/src/slider.rs`, `gui/toolkit/src/grab.rs`; the
desktop's notification pane is the first host of the slider, and the colour
picker (`colorpicker.rs`) and the scrollbars of the file dialog and the tree
view take hold by the same rule. Applications draw their own
sliders and are asked to move onto this one in
`requests/c-e-the-toolkit-has-a-slider-now.md`.
