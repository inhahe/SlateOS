### [E] The two text editors draw a file name's control characters raw in the tab and title -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26) -- `Document::shown_name` in both editors
renders the name through `pathtext` at every drawing and message site; `name`
itself stays exact for Save As and the conflict markers. Tests
`a_documents_name_is_kept_exactly_and_drawn_escaped` in each. Was:
`apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`,
`shown_file_name` and every use of `Document::name` / the document's `name`.

**In short:** a document's name is one string doing two jobs. It is *used* --
Save As suggests it, and a merge conflict writes it into the text as a marker
-- so it is the file's name exactly whenever that is text
(`pathtext::ShowPath::text_or_shown`). It is also *drawn*, in the tab and the
window title, where a name holding a control character (a tab, a line break;
legal in a SlateOS name) is drawn raw rather than as an escape. Nothing is
lost or misnamed; the label only looks wrong for such a name.

**The proper fix:** keep the exact name as the document's name and render it
at each drawing site with `Path::new(&name).shown()` (or keep a second,
shown label beside it). Every other lane E program already draws names
through `shown`; these two were left because their name also feeds Save As
and the conflict markers, which must stay exact.
