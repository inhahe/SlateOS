### [E] The mind map's undo acts on whichever map is showing, and can delete a node of another map -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26) -- with the mind map's own file (design-decisions §1208).

**In short:** the mind map app holds several maps, as tabs, and keeps one undo
history for the window. Undo replays the last change onto whichever map is
showing. Each map is given a copy of the id counter when it is made, so node
numbers repeat from map to map -- and undoing "add a node" made on one map,
while another is showing, deletes that other map's node with the same number,
with everything under it. Deleting a map leaves its changes in the history too.

**Where.** `apps/mindmap/src/main.rs`: `MindMapApp::{undo, redo, apply_reverse,
apply_forward, switch_map, delete_active_map}`, `MindMap::new` (the cloned
`IdGenerator`).

**And the tabs cannot be reached.** The tab strip is drawn, but no click
selects a tab and no key does: `switch_map`, `add_map` and `delete_active_map`
have no caller. Ctrl+O reads an outline into a new tab and makes it the one
showing, so every map before it is then out of reach for the rest of the
session.

**The proper fix** is the whiteboard's (known-issues, the document
applications entry): each map keeps its own history, and a change is undone on
the map it was made on. The tabs want to work -- a click, Ctrl+Tab, a new map
and closing one (asking first when it has unsaved changes). With it, the mind map wants what the diagram and the
whiteboard got -- a file of its own that keeps every colour, shape and fold (its
outline save keeps only the words and the tree, as its notice says), the
unsaved mark, and the question before a close or an Open.

**Fixed** the same day. Each map keeps its own history
(`MindMap::{undo_stack, redo_stack}`), so an undo acts on the map it was made
on, and a map closed takes its history with it. The tabs answer a click,
Ctrl+Tab and Ctrl+Shift+Tab; "+" and Ctrl+N add a map; a tab's close mark and
Ctrl+W close one, asking first when it has changes not saved. Each map saves
whole to a file of its own and the window asks before losing one (§1208).

Found on the way, and fixed with it: the toolbar's eight buttons were drawn and
answered no click, and three overlapped the next by five pixels; what the last
open or save did was kept "for the status line" and never drawn, so every open
and save said nothing; the status line ran under the selected node's line; and
a press on the sidebar or the status line was taken as one on empty canvas,
dropping the selection and starting a pan. The notice strip is gone with what
it warned of: the map is kept whole now, and the outline export says what it
leaves out.
