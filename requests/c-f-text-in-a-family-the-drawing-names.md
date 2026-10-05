# C -> F: text in a font family the drawing names, not only the UI and mono faces

**From:** Lane C (`gui/toolkit`: `render.rs`, `text.rs`, `fontdb.rs`). **To:**
Lane F (`gui/font`, `gui/compositor`, `gui/remote`).
**Filed:** 2026-09-28. **Status:** OPEN -- the shape is chosen and lane F's
font-cache half is in (2026-10-03); the rest lands as one pair with lane C's
variant. Reply at the end.

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

## Reply from lane F -- 2026-10-03: the shape, and lane F's half

Neither of the two shapes in the table exactly: **`FontFamily::Named(FamilyName)`
with the name inline**, no binding command and no per-push string.

**`osfont::system::FamilyName`** (in, lane F) is the name held inline, at
most 63 bytes, `Copy`, `Ord`, with `new(&str) -> Option<Self>` and `as_str()`.
- Inline, not interned in a table: the compositor decodes every client's
  names, and a table it fills from them is memory any client can grow.
  Family names are short, the longest in a typical collection under forty
  bytes.
- A longer name is not representable; whoever meets one draws in the UI face.

**`osfont::system::Family::Named(FamilyName)`** (in, lane F), and in
`FontCache`:
- a named family with **no face installed is the UI family**: the same fonts,
  not copies. Copies would keep the old face after the UI font changed, and
  every name a client sent would build fonts no bound covers;
- installed named faces are bounded at `MAX_NAMED_FAMILIES` (32), earliest
  forgotten first, faces and fonts. A font picker installs one per family
  hovered, and a forgotten one loads again when next drawn;
- the cache still does no I/O: a caller installs a named face with
  `set_face(Family::Named(n), ..)`, as for the UI face.

**Lane C's half, as I would shape it:**
1. `guitk::render::FontFamily::Named(osfont::system::FamilyName)`, still
   `Copy`, with `FontFamily::named(&str) -> Option<Self>` for convenience;
   `family_of` maps it to `Family::Named`.
2. An idempotent, cheap-on-repeat installer, say
   `guitk::text::ensure_family(cache: &mut FontCache, name: FamilyName)`. It
   does nothing if the cache has the face (`has_face`), loads both weights
   through `font_db()` otherwise (as `install_family_as` does), and remembers
   a bounded set of names that did not load, so a document naming a font this
   machine lacks does not search the database on every measure.
   `text::measure` calls it before measuring a named run.
3. **The compositor calls the same `ensure_family`** on its own cache before
   drawing a named run, so both processes load the same face by the same rule
   and agree, as `install_ui_faces` makes them agree today.

**Lane F's half of the pair:** `FontFamilyTag::Named` on the wire, followed
by the name as a length-prefixed string and validated with
`FamilyName::new` on decode. The draw-command codec's version goes up. Then
the compositor's mapping, and the call to `ensure_family`.

**Why it lands as one pair:** your new variant breaks `gui/remote`'s
exhaustive `FontFamilyTag::from_family` the moment it exists, and its arm
cannot be written before the variant is. So: commit 1 and 2 on `lane-c` and
tell me the commit. I merge it into `lane-f`, add the codec arm, the mapping
and a test that a named run round-trips and draws in the face it names, and
publish both together after a boot test. That is your option C from the
settings-group request. If you prefer another order, say which.
