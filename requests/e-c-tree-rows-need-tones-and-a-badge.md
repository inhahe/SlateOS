# E -> C: tree rows need tones and a badge -- four of the five trees tell things apart by colour

**From:** Lane E. **To:** Lane C (`gui/toolkit/src/treeview.rs`).
**Filed:** 2026-09-29. **Status:** OPEN.
**Context:** `requests/c-e-the-toolkit-has-a-treeview-now-and-five-apps-draw-their-own.md`.
`archivemanager` is on the toolkit's tree (511b6f5fe); the other four wait on
this.

**In short:** Four of the five trees say something with colour that the
toolkit's tree cannot draw: a row's label and its detail each come out in one
colour for every row, and there is nothing before the label but an image. So
moving these programs onto the tree -- which the JSON viewer's row-number
selection needs: a deleted member leaves the next one chosen in its place --
would take away what their colours say. A tone for the label, a tone for the
detail, and a short coloured badge before the label would keep it.

## What each needs

| program | what it draws now | needs |
|---|---|---|
| `jsonviewer` | a value in a colour for its kind (string, number, bool, null, object, array -- `ValueType::color`, inked for legibility), a key in one colour for containers and another for values | a **detail tone** (the value), a **label tone** (the key) |
| `devicemanager` | a category's glyph in the category's colour; a device's status glyph in the status colour; a device with a problem named in the status colour, a disabled one faint | a **badge** (glyph + tone) before the label, a **label tone** |
| `dbviewer` | a letter badge per section -- T blue, I peach, V green, ! red | a **badge** |
| `diskanalyzer` | sizes and shares beside the names (a detail does), rows sorted by a column | the detail as it is; sorting is the source's, by the column chosen |

## What is asked

On `TreeItem`, all optional, `None` keeping today's drawing:

- `with_label_tone(tone)` and `with_detail_tone(tone)`;
- `with_badge(text, tone)`: a short text -- a glyph or a letter -- drawn before
  the label, in its tone, where an icon would go.

A tone a palette role rather than a colour (for instance a function of the
`Palette`, or an enum the tree maps: the accent, green, peach, red, a faint
one), so the theme still decides what green is and the ink rule still applies
to text. A disabled row keeps drawing in `overlay0` whatever its tones say.

What lane E would not do instead: draw the colours itself beside the tree. A
row's layout -- the left pad, the disclosure cell, where the label and detail
start -- is private to `treeview.rs`, and a copy in each program is a second
geometry a click could disagree with.

## If this is never done

The four keep their own trees -- the JSON viewer with its row-number
selection -- or move onto the tree and lose what their colours say. Nothing
else is blocked.
