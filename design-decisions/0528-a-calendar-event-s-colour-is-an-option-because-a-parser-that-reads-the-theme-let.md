## 528. A calendar event's colour is an `Option`, because a parser that reads the theme lets a display setting edit the user's file

**Date:** 2026-08-23
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Each event in the calendar can have a colour you picked, saved in
your calendar file as a line like `color: FF7F00`. Most events have no such
line, because most people never pick one. The old code handled that by giving
those events a specific blue — and then writing that blue back into the file the
next time it saved. The colour it invented came from the dark theme. Converting
the calendar to read the shared palette would have made the invented colour
depend on which theme was active, so opening the calendar in light mode would
have rewritten every uncoloured event in your file. The field is now
`Option<Color>`: "you did not pick one" is stored as *nothing*, not as a colour,
and nothing is written out for it.

**The tempting non-decision.** The palette conversion's mechanical rule is "every
`theme::X` becomes `p.x`". Applied here it would have turned

```rust
color: parse_hex_color(val).unwrap_or(theme::BLUE),
```

into `.unwrap_or(p.blue)`, which reads as an improvement and is not one. The line
is in **`import_text`**, a parser. A parser that consults the palette makes the
same bytes parse to different data depending on a display setting. `export_text`
then writes that data straight back out. The two together mean that merely
*opening* the calendar under a different theme silently edits the user's file —
and, worse, does so in a way that survives a theme change back, because the
colour is now explicit. A display preference must not be able to author user
data. That is not a tradeoff; it is the constraint that ruled the option out.

**So the real question was: `Option<Color>`, or a sentinel?** Both keep the
parser palette-free.

| | |
|---|---|
| **Sentinel** — keep `Color`, reserve one value (say fully transparent) for "unset" | The type stays simple and every existing `event.color` site compiles unchanged. But "unset" is now spelled as a colour, so any site that forgets to check the sentinel draws it — and a fully transparent dot is an *invisible* dot, which is exactly the failure mode the whole entry is about. It also makes the sentinel unusable as a real value forever. |
| **`Option<Color>`** — chosen | Every reader is forced by the compiler to say what it does with "unset". The three call sites that resolve it (the month dot, the detail bar, the serializer) each had to state a rule, which is how the month dot's missing rule was found at all. Costs: a wider type, and `Some(...)` at every construction site including a hundred lines of test setup. |

This is the same reasoning as §526, where a window peek's *sampled* colour became
an `Option` because "nobody has looked at this window yet" is not a colour
either. Two `Option`s for the same reason in two modules is a pattern worth
naming: **in a renderer, "no value" must not be spelled as a value, because the
renderer's job is to draw values.**

**The default now lives at the draw site, not the parse site.** `None` resolves
to `p.lavender` in `CalendarEvent::dot_color(&self, p: &Palette)` — one method,
called by both the grid dot and the detail card's bar, so the two marks for one
event cannot come to disagree. Two sites resolving `None` independently would be
two answers to one question, and they drift.

**What this cost, and what it bought.** It cost the `Option` wrapper at every
construction site and a `color: None` in about a dozen test fixtures. It bought
the discovery that the month-grid dot never read the field at all: the dot was a
fixed lavender, so a colour the user set was parsed, stored, serialised back
faithfully, and visible only on the detail card that opens when a day is
selected. The round-trip test that "covered" the field passed throughout, and
would have passed forever, because **a value that round-trips correctly and a
value that is used are unrelated properties.** Making the compiler ask what
`None` means is what made someone look at the site that was not asking anything.

**Known and deliberately left open:** a `Some(c)` whose colour happens to match
the accent draws an invisible dot on today's disc. The user's colour is honoured
unconditionally, which is the right default and the wrong *only* rule; the three
candidate fixes are all user-visible, so it is logged as
`TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC` rather than
guessed at here. Note that no membership test can ever see it: the user's colour
is not a palette member at all, so `palette_check::assert_drawn_from` is *told*
to accept it, and a membership test cannot check a value it was told to accept.
