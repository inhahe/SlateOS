## 1486. A picture in rich text is one character, the picture kept beside it -- and the program's clipboard carries pictures

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit's rich text field (the one a mail program writes a
formatted message in) took bold, colours and sizes but no picture, though
the roadmap asks for "image paste". It takes pictures now: a program can put
one in (its own "Insert picture" command, from a file), a user can paste one
copied elsewhere in the same program, or drop one on the field. In the text
a picture is a single character, so it is selected, deleted, copied and
undone exactly as a letter is; it stands on the line like a large letter,
shrunk to the field's width if wider. The program's clipboard, which held
text only, now holds a picture too. Copying a picture from one *program* to
another still waits on the system clipboard, which is the operator's open
question C-Q29.

**Where:** `gui/toolkit/src/picture.rs` (`Picture`, `Uploads`),
`gui/toolkit/src/clipboard.rs` (`set`, `set_picture`, `picture`,
`generation`), `gui/toolkit/src/richinput/doc.rs` (`OBJECT`,
`RichDoc::picture`, `pictures`, `plain_text`, `to_html_with`),
`gui/toolkit/src/richinput/layout.rs` (`Piece::picture`, `Shown`, `parts`),
`gui/toolkit/src/richinput.rs` (`insert_picture`, `pictures`, `drop_data`,
`copy`, `paste`), `gui/toolkit/src/editmenu.rs` (`EditState::takes_pictures`,
`why_no_paste`). Closes
`known-issues-resolved/TD-C-A-RICH-INPUT-CANNOT-TAKE-A-PICTURE.md`.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A picture is one character of the text, U+FFFC, the picture kept beside it at that offset** | an object list keyed by count of U+FFFC; or a picture carried on the run's format | Every edit is already "replace one stretch by another" -- slicing and appending the side list with the text keeps the picture with its stretch through cut, copy, paste, delete and undo, with nothing new in the history. The format stays a small `Copy` value, and typing after a picture does not inherit it. | A U+FFFC typed as text must be told apart from a picture's: it is a character with nothing beside it, drawn as the font draws it. |
| **A picture's identity is a number given when it is made** (`Picture::id`, from `0x5049_4354_0000_0000` up) | comparing pixels | A window draws a picture by number, uploaded once; a copy, a paste and an undo are the same picture under the same number, sent once. Equality is O(1). | Two pictures made from identical pixels are different pictures, sent twice. |
| **Pixels as `imagecodec` decodes them, shared and never changed** | the toolkit's `Canvas` (`Vec<Color>`) | Decoded once, uploaded with no conversion (`WireBytes::from_le_argb`), written as a PNG with no conversion (`encode_png`); a copy costs a pointer. | A program drawing its own picture converts once (`Picture::from_canvas`). |
| **The program hands the window the pictures** (`Uploads::changes`, each frame, drops first) | the field uploading them itself | The toolkit draws nothing and holds no window, as every widget here; the window library is lane F's and depends on the toolkit, not the reverse. Drops before uploads is §557's rule. | One more call in a program's frame -- the call every picture viewer already makes. |
| **Shown at its own size, shrunk to the box's width, standing on the baseline** | always its own size; or centred on the line | A word processor's default: a picture is an inline letter, a tall one lowering the baseline with the text's descent kept below. Never wider than the box, so it never needs cutting. | No resizing handles: a picture is its own size or the box's. |
| **A line may break before and after a picture** | a picture glued to the word beside it | UAX #14 gives U+FFFC a break opportunity either side (class CB); a picture beside a word wraps alone. | -- |
| **The clipboard holds text and a picture, and a generation number every copy changes** | text only; or a full format list (`dnd::DataObject`) now | The rich field keeps its whole copy beside the clipboard and brings it back only while the generation is the one it copied with -- the old test, "is the clipboard's text the same", brought formatting back onto text another field copied, and could not tell a picture-only copy from nothing. A format list waits on the system clipboard's transport (C-Q29), which is what decides the formats. | HTML and other formats are not on the clipboard yet; a rich copy carries plain text and, alone, the picture. |
| **Paste takes the clipboard's text where it has any, else its picture** | the picture first | The common case is text; a picture copied alone pastes as a picture. When the system clipboard brings both -- a browser's "copy image" with its address -- this is the rule to revisit. | A copy carrying both pastes the text. |
| **Paste in a text-only field explains a picture-only clipboard** ("What was copied is a picture, not text") | lit, doing nothing; or "Nothing has been copied" | The honest-controls rule (§1485): a dimmed row says why. | -- |
| **HTML holds the picture itself** (`data:image/png;base64,...`); `to_html_with` names it elsewhere | an `<img>` with no source | The default is self-contained -- any reader shows it; a mail program sending the picture as a part of the message names it `cid:` instead. | A large picture makes a large string. |
| **A drop is put where it was dropped, by the paste rule; a drop it cannot use moves nothing** | the field ignoring drops | The roadmap's "dragged in"; the data object is the toolkit's own. | A file dropped by name is the program's to read: the field reads no files. |

### What it does not do

- **Pictures between programs**: the system clipboard and drag between
  programs wait on C-Q29 (`TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`).
  When it is answered, `clipboard::set`/`picture` are the one place a picture
  leaves or enters the program -- as a PNG (`Picture::to_png`) out, any
  format `imagecodec` reads in.
- **Resizing a picture in the text**, dragging a picture or a selection out
  of the field, and wrapping text around a picture: not asked for by the
  roadmap item.
- **An "Insert picture" toolbar button**: the toolbar's buttons act on the
  field alone, and choosing a file is the program's (its file chooser); the
  program calls `insert_picture`.
