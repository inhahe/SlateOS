# C -> F: text in a font family the drawing names, not only the UI and mono faces

**From:** Lane C (`gui/toolkit`: `render.rs`, `text.rs`, `fontdb.rs`). **To:**
Lane F (`gui/font`, `gui/compositor`, `gui/remote`).
**Filed:** 2026-09-28. **Status:** OPEN -- nothing waits on it but the font
picker; lane C builds the toolkit half once the drawing half has a shape.

**In short:** a program can only ask for text in two faces today, "the UI
face" and "the fixed-pitch face" (`FontFamily::{Ui, Mono}`); which fonts those
are is the user's setting, and the compositor installs them. That is enough
for a desktop, and not for two things `roadmap-detailed.md` §3.5 asks the
toolkit for: a **font picker** whose preview shows each family as the user
moves over it (the "live tentative selection" item), and **documents** -- a
word processor's runs, a presentation's headings -- whose text is in fonts the
*document* names. Both need to draw a line of text in a family by name.

## What is asked

A way for a render tree to say "this text, in the family *Noto Serif*" --
and for measuring and drawing to agree about it, as they must for the two
faces today (`osfont::system::FontCache`'s documentation says why: a label
measured in one face and drawn in another overflows its button).

The shape is yours to choose. Two that seem to fit what is there:

| | For | Against |
|---|---|---|
| `FontFamily::Named(id)` with a small interned id, and a `RenderCommand` that binds an id to a family name once per tree | `FontFamily` stays `Copy`; a name crosses the wire once, not per run | a second command to keep in step with the first |
| `RenderCommand::PushFontNamed { name }` beside `PushFont` | one command, obvious on the wire | a string per push; `PushFont`'s `FontFamily` stays two-valued |

Whichever: a family this machine does not have draws in the UI face, as the
compositor's `install_family` already falls back, and says so to nobody --
a picker lists only installed families (`guitk::fontdb::FontDb::families`),
so the fallback is for a document from another machine.

## What lane C does after

- `guitk::text::measure` (and the paragraph wrapping on it) measure in a named
  family through the same cache the drawing uses.
- `guitk::fontpicker`: the dialog -- family list with search, styles the
  family has, size, a preview line drawn in the tentative choice -- firing
  `Tentative`, `Committed` and `Cancelled` so a host can preview and revert.
- The Settings app's font page can then use it for the UI and mono fonts
  (lane E), which the compositor already honours.
