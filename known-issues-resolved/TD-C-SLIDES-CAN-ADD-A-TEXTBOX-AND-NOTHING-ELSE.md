## `TD-C-SLIDES-CAN-ADD-A-TEXTBOX-AND-NOTHING-ELSE` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/slides` edits a deck well enough -- new slide, duplicate,
delete, copy, paste, add a textbox -- and then stops. You cannot add a shape or
an image, you cannot delete an element you just added (only the whole slide
around it), and the theme and the slide transition are both **printed on screen
and impossible to change**.

**Verified.**

| Operation | State |
|---|---|
| `add_shape` | no production caller; `T` adds a textbox and nothing adds anything else |
| `add_image_placeholder` | the same |
| `delete_selected_element` | no production caller. `Delete` is bound to `delete_slide`, so the only way to remove an element is to remove the slide holding it |
| `set_theme` | no production caller, while the window draws `format!("Theme: {}", self.theme.name)` |
| `set_current_transition` | no production caller, while the window draws `"Transition: {}"` **twice** -- once on the slide and once in the property panel |

`selected_element` is genuinely written (`Some(eid)` in three places, when an
element is added), so `delete_selected_element` has a subject waiting for it.
That distinguishes this from `rssreader`, where the equivalent field had no
writer at all and the operations had nothing to act on.

**The theme and transition are the same defect as
`TD-C-SETTINGS-THE-PROGRAM-OBEYS-AND-NOTHING-CAN-CHANGE`**, and among the worst
instances of it, because they are displayed *three* times between them. A label
reading "Transition: Fade" in a property panel is not a status line; it is an
offer.

**The false positive worth recording.** `next_slide` and `prev_slide` also have
no callers, and they are **not** a defect: `Left`/`Right`/`PageUp`/`PageDown`
all move through the deck via `advance`, and `Home`/`End` via `jump_to`. They
are redundant duplicates of working code, so the feature is reachable and the
functions are merely dead. **"No caller" means the *function* is unreachable;
it does not mean the *feature* is** -- the implication runs one way only, and a
sweep that forgets this reports working programs as broken ones.

**What the repair wants.** Keys, on the pattern this lane has now used four
times: a key for shape and image, a key for deleting the selected element that
is not the one that deletes the slide, and a key each to cycle theme and
transition -- both of which already have `next()`-style cycling elsewhere in
this tree and are already displayed, so the display becomes true the moment a
key exists.



**Fixed the same day.** `S`, `O`, `L` and `A` add the four shapes, `I` adds an
image placeholder, `Ctrl+T` moves through the three themes and `Ctrl+R` through
the six transitions -- both of which the window was already printing. `Delete`
now removes the *selected element* when there is one and the slide otherwise,
with `Shift+Delete` always meaning the slide: erring towards the element is the
safe half of the ambiguity, since re-adding an element is cheap and re-making a
slide is not.

**One test earns its place more than the others.** `Key::T` is unguarded and
lives in the same match, so putting the new `Key::T if ctrl` arm after it would
have let Ctrl+T add a textbox and leave the theme alone -- which looks exactly
like a theme key that does nothing, the very defect being fixed. I wrote it
that way first and caught it before running it;
`ctrl_t_does_not_add_a_textbox` is what keeps it caught. 90 tests, up from 83.
