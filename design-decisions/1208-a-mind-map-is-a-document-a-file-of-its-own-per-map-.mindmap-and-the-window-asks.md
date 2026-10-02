## 1208. A mind map is a document: a file of its own per map, `.mindmap`, and the window asks before losing one

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The mind map app could not keep a map. Ctrl+S wrote an outline --
indented lines of text, which keep the words and the branches and lose every
colour, shape, position and fold -- and the window closed over unsaved maps
without a word. Each map (each tab) is now saved whole to a file of its own,
and closing a map, or the window, over changes not saved asks first, as the
diagram and the whiteboard do. The outline stays, as an export (Ctrl+E) and as
something Ctrl+O can open.

### A file per map, or one for the window

| | A file per map (chosen) | One file holding every tab |
|---|---|---|
| What Ctrl+S saves | the map showing | every map |
| Giving someone one map | its file | cut out of a workspace |
| Coming back to a session | open each map | open one file |
| The other editors here | the JSON viewer's tabs are a file each | none works this way |

A window of tabs where each tab is a file is what the JSON viewer already is
and what a document editor is expected to be; a workspace file would be a
second idea on top of it, and can still be added on top.

### Opened by what it holds

Ctrl+O takes a map file or an outline, and decides which by what the file
holds: a file whose top level names `slateos-mindmap` is a map, anything else
an outline. The name is a claim by whoever gave it; the content is the fact
(the rule `apps/diagram`'s `write_diagram` states for opening a file). An
outline opens as a new map with no file, so saving it asks where and never
writes a map over the outline. A map file too large to read whole is refused;
an outline too large is opened to its last whole line and said to be
incomplete.

### Smaller calls

- **Save As names the map after the file.** The name on the tab is the name
  the map is found by, and there was no other way to name a map.
- **Closing the last map leaves a fresh one.** A window with no map has
  nothing to draw or add to; refusing Ctrl+W on the last tab, as the JSON
  viewer does, would leave no way to throw that map away.
- **Each map keeps its own undo history.** It was the window's, replayed onto
  whichever map was showing, and could delete a node of another map
  (known-issues, the mind map's undo).
- **A map is marked changed where a change is recorded for undo** -- and by
  undo, redo, and a layout that moves a node -- not wherever a map is borrowed
  to change: renaming a node to what it already says is not a change.

**Where it lives:** `apps/mindmap/src/main.rs`: `mindmap_document`,
`mindmap_from_document`, `MindMapApp::{open_file, import_outline, write_map,
save_map_as, save, picked, request_close_map, close_map, request_quit,
answer_close, continue_quitting, push_undo, lay_out_again}`.

**How to reverse:** the format is `mindmap_document`/`mindmap_from_document`;
the file-per-map choice is the map id `write_map` takes.
