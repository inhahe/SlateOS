## 546. The toolkit gets a focus, because without one every keystroke went to the last text field in the window

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The widget toolkit had no idea which control the user was typing
into. When a key arrived, the widget tree handed it to whichever control agreed
to take it — and since the tree is walked back to front, that was always the
*last* one. On a dialog with a "Name" box above an "Email" box, everything the
user typed went into "Email", no matter which box they had clicked. This entry
records introducing a focus — the single control that receives typing — and the
four choices that came with it: where the focus is stored, which controls can
have it, how a text field remembers a selection, and how a field longer than its
box decides which part of the text to show.

The task that led here was a much smaller one: `known-issues.md`'s
`TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL` asked for a caret to be
drawn in two fields that track one but never show it. A caret must only be drawn
in the field the typing goes to, so drawing one required knowing which field
that was — and the answer was that nothing knew, and that the guess being made
instead was wrong. Fixing the caret without fixing the routing would have drawn
a caret in the field the user clicked while the letters appeared in a different
one, which is worse than drawing nothing.

**The focus is a `bool` on the widget, not an id held by the tree.**
The alternative is the usual one: `WidgetTree` holds `focused: Option<WidgetId>`
and the widgets know nothing. That is tidier — it makes "at most one thing is
focused" true by construction instead of by an invariant the setters maintain —
and it was rejected for one concrete reason: `Widget::render` takes `&self` and
receives nothing but the render tree. A widget that cannot see the tree's
focused id cannot decide whether to draw a caret, so the id would have to be
threaded through `render` and through every one of its recursive calls, changing
a public signature that a dozen call sites outside the toolkit use. The `bool`
costs an invariant instead: `focus_by_id` clears the whole subtree before it
sets one, so the two-focus state is unreachable through the API, and no code
outside `widget.rs` can write the field because it is private.

**Buttons and checkboxes take focus, not just text fields.** The narrow choice
would be to give focus only to controls that consume text. It was rejected
because it makes Tab a lie: a user tabbing through a dialog would skip over the
OK button and have no way to press it without a mouse, which is precisely the
situation a keyboard user is in. So `accepts_focus` answers yes for text fields,
buttons and checkboxes — and, in the same change, a focused button responds to
Space and Enter and a focused checkbox to Space, since a control you can tab to
and then not operate is worse than one you cannot reach. Labels, containers,
progress bars and separators decline: there is nothing to do to them.

**A selection is an anchor plus the caret, not a pair of offsets.** A selection
needs two ends; the caret is already one of them. Storing a `(start, end)` pair
alongside the caret makes a third number that can disagree with the other two —
a pair that says 3..7 while the caret sits at 9 is representable and meaningless,
and every edit has to remember to fix all three. An anchor cannot disagree,
because the other end *is* the caret. This is also the shape
`gui/desktop/src/run_dialog.rs` already uses, so the toolkit now matches the one
field in the tree that had already solved this. The anchor is a plain byte
offset with no affinity, unlike the caret: a selection is a range of *text*, and
a range has no side of a direction boundary to be on.

**How far a long field is scrolled is recomputed every frame, not stored.**
A field narrower than its text has to decide which part to show. The obvious
implementation keeps a scroll offset in the widget and nudges it whenever the
caret would leave the box. That gives the nicest behaviour — the view only moves
when it has to, so text does not jump under the reader — and it was rejected
because a stored offset is a second source of truth about where the text is, and
it goes stale in every direction: `set_input_text` replaces the string under it,
a window resize changes the width it was computed against, a font change changes
the widths of every glyph. Each of those is a separate bug that shows as text
scrolled somewhere it should not be, and each needs its own line of code to
fix. Recomputing from the caret position makes "the caret is visible" true by
construction, with no state to invalidate. The price is real and one-sided:
moving the caret leftwards through a long string scrolls the view to put the
caret at the *left* edge, further than strictly necessary, so the text shifts
more than a user of another toolkit would expect. That is a cosmetic difference
against a class of correctness bugs, and it can be revisited by adding the
stored offset later without changing any caller.

**What Tab does at the ends.** It wraps, rather than stopping at the last
control or moving out of the window. A dialog is a closed set of controls with
nowhere else for the focus to go, so stopping would leave Tab silently doing
nothing — indistinguishable, to the user, from the bug this entry is about.
