## TD-C-OVERLAY0-IS-A-DISABLED-INK-AND-SOME-LIVE-TEXT-IS-DRAWN-IN-IT -- FIXED 2026-09-13

**Date:** 2026-09-09. **Lane:** C.
**Where:** `gui/appearance/src/lib.rs` (`LIGHT_OVERLAY0 = #9CA0B0`); about ten
draw sites, `apps/editor/src/main.rs:2329` and `:2345` among them.

**In short:** the palette has a deliberately faint grey for things that are
switched off, so that "disabled" looks disabled. A handful of places use it for
text that is *not* disabled — the editor's status bar draws the live cursor
position and line count in it — and at that colour the text is close to
unreadable: 2.30 : 1 against the page, where 4.5 is the floor.

**The ink is not the bug.** `overlay0` fails 4.5 on all six panes, by design:

| ink | base | mantle | crust | surface0 | surface1 | surface2 |
|---|---|---|---|---|---|---|
| text | 18.57 | 17.27 | 15.87 | 13.60 | 11.55 | 9.71 |
| subtext1 | 10.50 | 9.77 | 8.98 | 7.69 | 6.53 | 5.49 |
| subtext0 | 9.58 | 8.91 | 8.19 | 7.02 | 5.96 | 5.01 |
| accent | 9.03 | 8.40 | 7.72 | 6.61 | 5.62 | 4.72 |
| **overlay0** | **2.30** | **2.14** | **1.97** | **1.69** | **1.43** | **1.20** |

WCAG exempts disabled controls precisely so that off can look off, and most of
the 829 uses are that: `if self.enabled { pal.text } else { pal.overlay0 }` in
`apps/alarmclock`, the same shape in `apps/dictionary`. Those are correct and
should stay.

**RE-MEASURED 2026-09-13: it is about two hundred, not about ten.** The first
pass was a grep for `overlay0` on the same line as a `tree.text(` call, and the
dominant shape in this tree is the *struct* form --

```rust
cmds.push(RenderCommand::Text { x, y, text, font_size, color: pal.overlay0, .. });
```

-- where the ink is five lines below the call. Counting both forms, by
*position* (the colour argument of a text draw, not the word `overlay0`
anywhere nearby), production code has **419** text draws inked `overlay0`:

| | count | what it is |
|---|---|---|
| **live** | **208** | no disabled/enabled word anywhere in the enclosing function. These are the bug. |
| ambiguous | 117 | such a word is in the function but not beside the draw; each needs reading. |
| exempt | 94 | the draw is right beside its own `if enabled { .. } else { .. }`. Correct as they are. |

Spread over about forty crates, the worst being `gui/desktop` (14),
`apps/screenrecorder` (13) and `apps/netscan` (12) -- so there is no single
place to fix it. Samples from the live set, to show they are not disabled
states: `apps/benchmark`'s "Press F5 or click Run to start benchmarking",
`apps/finance`'s "Total Balance" label, `apps/weather`'s chart axis
temperatures.

**FIXED 2026-09-13. 473 draws moved to `subtext0`; 14 were genuinely
disabled and stayed.** `scripts/check-overlay0-ink.py` refuses the live shape
from now on, and is wired into `check_lane_c_gui_gates`.

**Getting the rule right took four goes, and every wrong version reported a
clean tree.** Recorded because each failure is the same shape -- a check that
passes over a population it cannot see:

| the rule said | what it missed |
|---|---|
| `overlay0` on the same line as a `tree.text(` call | the struct form, where the ink is five lines below the call. This is the one that made the original entry say "about ten". |
| a disabled-word within nine lines | `fn render_disabled_button`, whose signature is *ten* lines above its draw |
| `\benabled\b` | an underscore is a word character, so it never matched `render_disabled_button` or `wifi_enabled` -- which is most of how these words appear in code |
| `is_empty` counts as disabled | 69 empty-state messages. `if list.is_empty() { draw("No devices found") }` is the one sentence in an empty pane, and WCAG exempts *disabled controls*, not empty lists. |
| the word anywhere nearby | `text: "No updates available."`, `text: "Disabled".to_string()`, and a comment reading "the placeholder is not editable text" -- none of them a condition |
| `color:` with the role on that line | a conditional ink spanning five lines. 64 draws, mostly search-box placeholders. Found by a failing test, not by the gate. |

The gate decides by **position** now: the ink expression (however many lines
it spans), the enclosing function's *name*, or a block that structurally
encloses the draw -- with string literals and comments blanked before any
match. Twelve self-test fixtures, four of which expect no finding, so a
detector that has stopped looking fails rather than passes.

**Three decisions that were not mechanical.**

* **Placeholders became readable.** This entry asked for them to be decided
  rather than assumed. WCAG exempts disabled controls and says nothing about
  placeholders, so "Search..." is now legible rather than ghostly.
* **A hierarchy lost its third level.** `gui/desktop/src/hotkeys.rs` drew
  heading / key badge / app name as `text` / `subtext0` / `overlay0`. The
  light theme cannot give the third level back: `LIGHT_SUBTEXT0`'s own doc
  comment says it and `LIGHT_SUBTEXT1` are 1.10 apart, "the same luminance to
  any eye", because every ink clearing 4.5:1 on `surface2` is crowded into one
  band. The badge is now told apart by *having a badge*. Legibility is the
  floor; a hierarchy is a preference.
* **`PermissionState::NotDecided` stayed `overlay0`**, and this is a recorded
  disagreement rather than a decision. Its doc comment argues that raising it
  would make "a not-decided row start looking decided". WCAG 1.4.3 exempts
  *inactive* components, and a permission awaiting a choice is fully operable
  -- "Not decided" is the status the user is there to read. Left alone because
  it was argued explicitly by whoever wrote it, and overruling that silently
  would be worse than leaving it. **It is also invisible to the gate**, which
  sees draw sites and not colour-returning methods, so it is one of the
  `TD-C-FORTY-NINE-COLOUR-METHODS` class.

**One bug the sweep introduced and the tests caught.** A desktop widget built
its colour channel by channel -- three separate `if` expressions for red,
green and blue -- so the sweep moved the red and left the other two, producing
a colour in no palette at all. Collapsed to one conditional.

**The bug is the sites where nothing is disabled.** `apps/editor`'s status bar
is the clearest: `tree.text(8.0, bar_y + 5.0, &pos_text, …overlay0, 11.0)` draws
"Ln 12, Col 4" — live, always-current information — at 2.30 : 1 and 11 px. A
first pass counts about ten draws that are not behind an enabled/live
conditional; each needs reading individually, because "not behind a conditional"
is not the same as "not disabled".

**Proper fix:** audit those ten. Anything conveying live state moves to
`subtext0`; anything genuinely marking disabled or placeholder stays. Note that
*placeholder* text is **not** exempt under WCAG even though disabled is, so the
three placeholder uses need deciding rather than assuming.

**Why it went unnoticed, and the wider point.** `overlay0` was absent from
`scripts/contrast-explorer.html` until today, and the guard added with §826 —
`light_inks_clear_the_contrast_floor_on_every_surface` — checks four inks and
does not include it. So both instruments that would have shown this were
looking at a palette with one fewer ink than the palette has. The tool now
includes it; the guard deliberately still does not, because asserting a known
and partly-legitimate failure means either a red build or a muted test. That
is the same reasoning as the thirteen accents in
`TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS`, and it should be revisited
once the ten sites are triaged: after that, a guard could assert overlay0 is
used *only* in exempt positions, which is the property that actually matters.

**Also found in the same survey:** `overlay1` and `overlay2` are declared and
used **zero** times anywhere in `gui` or `apps`. They are dead palette rungs.
