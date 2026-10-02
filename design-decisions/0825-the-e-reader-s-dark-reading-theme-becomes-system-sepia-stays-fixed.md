## 825. The e-reader's "Dark" reading theme becomes "System"; Sepia stays fixed

**Date:** 2026-09-08
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the e-book reader has a button that switches its page between two
looks, labelled "Dark" and "Sepia". The "Dark" one was a private copy of the
desktop's dark colours, so switching the desktop to a light theme left the
reader dark, for no reason anybody had chosen. It now simply *is* the desktop's
theme, and the button says "System" instead — because a button labelled "Dark"
while the screen is light is a label that contradicts what you can see. "Sepia"
is unchanged and deliberately still fixed: warm paper is a reading surface you
pick for your eyes, and one that turned dark along with the desktop would be
the single thing it exists not to do.

**What was actually there.** `ThemeColors::dark()` set fourteen fields, and
every one of them was a Catppuccin Mocha value that the desktop `Palette`
already carries under a name: `background` was `base`, `surface` was
`surface0`, `surface_alt` and `separator` and `selected_bg` were `surface1`,
`text` was `text`, `text_dim` was `subtext0`, `accent` was `blue`, `accent_dim`
was `surface2`, `bookmark_color` and `error` were `red`, `progress_bar` was
`green`. Not "similar to" the palette — the same twenty-hex table, written out
a second time.

**The decision.** `dark()` becomes `from_palette(&Palette)`, `ThemeKind::Dark`
becomes `ThemeKind::System`, and the label follows. `sepia()` is untouched.

**Why this is not simply the palette conversion applied.** Everywhere else in
the sweep the rule has been mechanical: a colour resolved at draw time follows
the theme, a colour stored on the user's data stays fixed. Here the reading
theme is *neither* — it is a display preference for the document, chosen in the
app, which is exactly the kind of thing that has a legitimate claim to ignore
the desktop. Sepia keeps that claim. What "Dark" could not defend is being a
**duplicate**: it offered no choice the desktop did not already offer, and the
duplication meant a user who set a light desktop theme had no way to get a
light reader at all — the two options were dark-Mocha and sepia.

**The renamed variant is the reason this is recorded rather than just done.**
It changes a word on screen. The alternatives were to leave the label reading
"Dark" while the reader draws light (a label that lies), or to keep a third
variant so "Dark" still means Mocha specifically (which reintroduces the
duplicate palette this removes, and leaves the reader unable to follow a
light desktop unless the user knows to pick "System"). Neither is better than
being accurate about what the button does. The operator may prefer a different
word — "Desktop", "Automatic", "Match system" — and that is a one-line change.

**A test was replaced, not just updated.** `test_dark_theme_colors` asserted
`tc.background.r < 100` — "the dark theme is dark". That passed just as well
when the reader held its own hardcoded copy and ignored the desktop entirely,
so it could not distinguish the bug from the fix. It now asserts the fields
*equal the palette's*, and that a light desktop produces a different background
— which is the property that was actually wrong.
