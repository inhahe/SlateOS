## `TD-C-A-PRESENTATION-EDITOR-THAT-CANNOT-TYPE` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/slides` cannot put a single word on a slide. Every text box
it creates says "New Text", every title slide says "Presentation Title", the
deck is called "Untitled Presentation", and **there is no way to change any of
them**. You can add slides, shapes and images, reorder them, copy and paste
them, cycle the theme and the transition, and export the result -- a deck of
placeholders.

**Verified.** There is no text input anywhere in the program:

| | |
|---|---|
| typing | `key.text` / `event.text` appear **zero** times in production; there is no `typed()`, no `types_text`, no edit buffer |
| element text | **zero** assignments to `.text` anywhere |
| the deck title | `title: String::from("Untitled Presentation")` at construction, **zero** writers |
| a new text box | `add_textbox` seeds `text: String::from("New Text")` |
| a new title slide | `Slide::new` seeds `text: String::from("Presentation Title")` |
| the accessor an editor would need | `element_by_id_mut` exists, is `pub`, and **has no caller** |

**This is not the same defect as an unreachable operation.** The operations
this app is missing were never written: there is no `set_element_text` sitting
callerless, because nobody wrote one. What is written is everything *around*
authoring -- layouts, themes, transitions, undo, export, a sorter view -- and
the hole is in the middle.

**The uncomfortable part, recorded because it is the useful part.** Earlier the
same day I fixed this app's *other* reachability gaps: `S`/`O`/`L`/`A`/`I` to
add shapes and images, `Ctrl+T` and `Ctrl+R` for theme and transition,
`Delete` for the selected element, and labels naming their keys. All of that
was real and none of it was the thing that matters. **I checked which written
operations had no caller, and that question cannot see a capability nobody
wrote.** A sweep for unreachable functions finds the periphery of an app and is
blind to a hole at its centre -- and the more thoroughly the periphery is
built, the more finished the program looks. `apps/slides` has 90 tests and a
sorter view.

**The question that would have found it** is not "what is unreachable" but
"what is this program *for*, and can a user do that". For a presentation
editor that is one sentence: put words on a slide. It takes longer to answer
than a grep, which is why it keeps being skipped.

**What the repair wants.** A text-entry mode, on the pattern
`apps/markdowneditor` and `apps/rssreader` now use: a key to begin editing the
selected text box (`F2` and `Enter` are both conventional), characters routed
into it via `element_by_id_mut`, `Backspace`, and `Escape`/`Enter` to finish.
`Slide::new`'s placeholders then become what they read as -- prompts -- rather
than permanent contents. The deck title wants the same treatment, and it is
what `export_as` names the file with.


**Fixed the same day.** `Enter` or `F2` types into the selected text box,
`Shift+Enter` gives a second line, and leaving on either `Escape` or `Enter`
keeps the words. The canvas draws the buffer while it is being typed, because
the commit happens on the way out and drawing the element would leave the user
typing at a slide that never changes.

**The test found a defect the design had.** `begin_editing` seeded the buffer
with the box's current text -- right for editing, wrong for a box that still
holds its prompt, because the first thing anyone types produces "New TextHi"
and they have to delete the prompt first. `PLACEHOLDER_TEXT` now lists the
seven strings a box is born holding and a box still holding one starts empty.
**A placeholder is a prompt, not content**, and the friction of deleting it
first is exactly what stops someone writing at all. The cost is that a user
who genuinely wants a box reading "New Text" types it twice.

`typing_does_not_fire_the_shape_keys` is the one that would have bitten
otherwise: `S`, `O`, `L`, `A` and `I` add shapes outside this mode, so a title
containing any of them would have littered the slide while being written. 93
tests, up from 90.

**The deck title too, later the same day.** `Ctrl+Shift+T` names the deck --
it had no writer, so every deck was "Untitled Presentation" in the window bar
*and* in the filename `export_as` builds. It shares the text mode through an
`EditTarget` enum rather than a second `Option`, since the deck's name is not
an element and has no id. An empty name is refused rather than blanking the
bar and exporting a file called ".pptx".

**Getting it in took three attempts, all the same mistake in different
costumes:** the enum went inside a `struct` body (I anchored on a field's doc
comment), then between a `#[derive(Debug)]` and the struct it belonged to --
which silently gave *my* enum that derive and produced a conflicting-impl
error 300 lines from the cause. Earlier the same evening an `impl` block went
inside another `impl`. **An anchor chosen by its text lands wherever that text
is, and in Rust the space above an item is owned by the item.** 96 tests.
