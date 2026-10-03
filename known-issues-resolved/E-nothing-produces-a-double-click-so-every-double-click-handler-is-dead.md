### [E] Nothing produces a double-click, so every double-click handler is dead -- 2026-09-25
**Status:** FIXED by lane F, 2026-09-25 -- `oswindow` recognises a double click once for every application (029bf1d50) and delivers `MouseEventKind::DoubleClick` after the second press, so every handler below now runs in a real window

**In short:** double-clicking a file in the file picker does not open it,
double-clicking a word in a text view does not select it, and the same goes
for every other double-click the toolkit handles: `MouseEventKind::DoubleClick`
is defined, carried on the wire and matched in `guitk`'s file dialog, grid and
text view -- and in `apps/markdowneditor` since today -- but nothing ever sends
one.

**Why.** The compositor deliberately does not synthesise it (`gui/compositor`,
`wire_mouse_kind`: double-click timing "belongs with the widget that has to
honour it"), and `oswindow` does not either, so the event every consumer waits
for does not exist on any path from a real mouse.

**The proper fix** is one synthesiser in `oswindow`, using the double-click
interval it already reads from `input.yaml`, delivering `DoubleClick` alongside
the second press. Design-decisions §502 settled the edge cases for the title
bar's own double-click, and they carry over. Until then, double-click in the
markdown editor's source pane (select a word) is tested by delivering the event
directly and does nothing in a real window.
