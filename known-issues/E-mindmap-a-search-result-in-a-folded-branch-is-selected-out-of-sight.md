### [E] Mind map: a search result in a folded branch is selected out of sight -- 2026-10-04

**Status:** open.

**In short:** the find bar searches every node, folded branches included, and
going to a result selects it -- but a node inside a folded branch is not
drawn, so nothing on screen shows what was found. The bar says "2/5" and the
map looks as it did. Typing goes to the first result as well (since
2026-10-04), so the first result being folded away is enough to see it.

**Where.** `apps/mindmap/src/main.rs`: `MindMap::search` walks the whole tree;
`set_search_query`, `next_search_result` and `prev_search_result` set
`selected_node`; `render_nodes` draws `visible_node_ids()`, which stops at a
folded node; `auto_layout` does not place a folded subtree, so a hidden
node's position is stale as well.

**To reproduce.** Add a child "Budget" under a node, fold the node (Space),
press Ctrl+F and type "bud": the count reads "1/1" and nothing is marked.

**The proper fix, and the choice in it.** Going to a result unfolds the
branches it is in, and lays the map out again, so the node is drawn where it
belongs and marked as a result. Which branches are folded is saved with the
map (`collapsed` in the file), so this is a change to the map, and has to be
an undoable one that marks it unsaved -- one undo step for the whole
reveal, not one per branch, which `Action` has no way to say yet (it needs a
compound action, or a `Reveal { unfolded: Vec<NodeId> }` of its own). The
alternative of drawing the folded branch's top node as "holds a match"
leaves the map unchanged but selects a node nobody can see. Unfolding is
what Freeplane and XMind do.
