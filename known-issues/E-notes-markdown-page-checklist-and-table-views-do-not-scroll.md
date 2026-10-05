### [E] Notes' Markdown page, checklist and table views do not scroll -- 2026-09-28

**Status:** open.

**In short:** a long Markdown note, checklist or table is cut off at the
status bar and the rest cannot be read in that view -- the wheel does nothing
over it. A plain note scrolls, and so does any note while it is being written
(its Markdown source included), because both are drawn through the toolkit's
multi-line field, which keeps a scroll offset. Before 2026-09-28 the other
views drew past the status bar and off the bottom of the window; they are
clipped to the panel now (`render_editor_area`), which is the half that could
be done without state.

**The proper fix.** A scroll offset for the page views, moved by the wheel
through `guitk::wheel`, clamped to the page's height (which
`render_markdown_preview` already walks line by line), and reset when the
selection moves to another note.
