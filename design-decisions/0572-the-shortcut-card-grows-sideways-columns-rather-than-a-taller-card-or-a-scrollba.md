## 572. The shortcut card grows sideways: columns rather than a taller card or a scrollbar

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop can show a card listing every keyboard shortcut. Until
today nothing could open it; giving it a chord (`Super+/`) immediately showed
that it does not fit on the screen — twenty-five shortcuts at 38 pixels a line is
taller than a 1080-line display once the title is added, so the last few
shortcuts were drawn below the bottom edge, invisible, with nothing to hint they
were there. The choice was how to make it fit. It now splits into two columns
side by side, like a printed cheat sheet, instead of getting taller or gaining a
scrollbar.

### The problem, precisely

`hotkeys::render_settings_panel` laid the bindings out as one column, one row per
binding, and reported its height as `HEADER_HEIGHT + rows * ROW_HEIGHT +
PADDING`. With the default set at 25 bindings that is 44 + 950 + 16 = 1,010, and
`ToggleShortcutCard` made it 26 — 1,048, then 1,086 with the row that was added
alongside it. A 1920×1080 display has 1,080 lines. The card was taller than the
screen before a user added a single shortcut of their own.

The failure mode is the bad kind: the card renders, looks complete, and is
missing rows. A user reads to the bottom of what they can see, does not find the
shortcut they wanted, and concludes the desktop does not have one.

### Options

| | *What changes* | |
|---|---|---|
| **A. Columns** (chosen) | The card is 560 wide and about 1,000 tall today; past that it becomes 1,120 wide and half as tall, with the list continuing in a second column. | No new state, no new input handling, no hidden region. |
| **B. Scroll** | The card stays 560 wide and stops at the screen height; the rest is reached by scrolling. | Needs a scrollbar, a scroll offset, wheel handling, and keyboard scrolling — and *the card has no input handling at all*, so all of it would be new. |
| **C. Shrink the rows** | Everything gets smaller until it fits. | Buys one screenful and no more; the next user who adds five shortcuts is back where we started, and the card is now harder to read. |
| **D. Leave it, document it** | Nothing; the bottom rows stay off-screen. | The defect is invisible to the person it harms. |

### Why columns

**A hidden region needs a way to reach it, and this surface has none.** The card
is drawn from a `bool` on `DesktopShell` and nothing else — no focus, no
scroll offset, no wheel routing, no keyboard handling beyond the chord that
toggles it. Option B does not "add a scrollbar"; it adds the first input path
this surface has ever had, and it re-creates the same defect in a milder form
until every piece of that path is right: a region below the fold that the user
cannot see and might not know to look for.

**The card is read, not operated.** It is a reference sheet. Every printed
keyboard reference — vim, Blender, Photoshop, the card in the box a keyboard
came in — is laid out in columns for exactly this reason: the whole point is to
see it all at once and scan it. Scrolling defeats the purpose of the artefact.

**Width is the axis with room.** A 560-wide card on a 1920-wide display uses 29%
of the width and 100% of the height. Two columns use 58% and 50%. Even four
columns fit, which covers a user with a hundred bindings.

### Against columns

Reading order becomes down-then-across rather than straight down, which is one
more thing for the eye to work out. And a very narrow display (a 4:3 800×600, or
a phone-shaped one) can run out of *width* the way the old layout ran out of
height — the fold has no answer for that case, and the card would overflow
sideways instead. That is a strictly better failure than the old one (a card
running off the right edge is visibly cut, whereas one running off the bottom
looks finished) but it is not no failure. If small displays ever matter here,
that is when option B earns its cost.

### Shape of it

`panel_layout(registry, max_height)` is the single place the arithmetic lives.
It floors the rows that fit at one, divides the binding count by that to get a
column count, then *re-divides* by the column count so that 26 bindings over two
columns come out 13 and 13 rather than 25 and 1. `settings_panel_size` and
`render_settings_panel` both call it, and both take `max_height`, because they
are only guaranteed to agree about the column count if they are given the same
budget — `DesktopShell::render_shortcut_card` binds it once and passes the same
value to both. The renderer steps a `row_in_column` counter and shadows
`panel_x` with the current column's left edge, rather than dividing an index, so
there is no division by a count a later edit could let reach zero.

**Where it lives:** `gui/desktop/src/hotkeys.rs` — `PanelLayout`, `panel_layout`,
`settings_panel_size`, `render_settings_panel`; `gui/desktop/src/lib.rs` —
`DesktopShell::render_shortcut_card`.
