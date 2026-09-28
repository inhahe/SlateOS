# C → E — the toolkit has a treeview now, and five applications draw their own

**From:** lane C. **To:** lane E. **Filed:** 2026-09-24.
**Status:** open — adoption, one application at a time; nothing is blocked on
it and nothing breaks if it waits.

## In short

`design.txt` asks the toolkit for a treeview and for a "tristate checkbox
treeview … with a function to populate it with a directory". Both exist now:
`guitk::treeview` (the widget) and `guitk::dirtree` (the directory function).
Five applications in your tree each hand-roll a tree — a node type with an
`expanded` flag, a flatten-to-rows pass, a row painter with indentation, and
keyboard handling none of them finished. This is a request to move them onto
the shared widget, which is what makes it more than a sixth copy.

## What there is

- **`TreeView<K>`** holds only the *view*: which nodes are open, which is
  selected, the scroll position, the ticks. The data stays in your
  application: implement `TreeSource` — one method, "the children of the node
  at this key path" — over the structure you already have. Every piece of state
  is keyed by the node's **key path**, never by row number, so opening a node
  cannot redirect the next click (`archivemanager`'s `toggle_path` comment
  records that exact bug).
- **Keyboard:** Up/Down, Home/End, PageUp/PageDown, Right opens then steps in,
  Left closes then steps out, Enter activates (opens a folder, reports a file),
  Space ticks, typing a letter jumps. **Mouse:** click the arrow to open, the
  row to select, double-click to activate, right-click for a
  `TreeEvent::ContextMenu`, wheel, and a draggable scrollbar.
- **Drawing is palette-driven** (`Surface::Selected` for the selection, the
  accent for ticks), and a tree can be drawn into your own `Frame` with
  `TreeView::draw(palette, &mut frame, AppTarget::Tree)`, so a modal over it
  takes the click. `handle_hit` then acts on what your frame's hit test found.
- **Disabled nodes** carry a reason (`TreeItem::disabled("why")`), shown via
  `hovered_reason()` / `selected_reason()`; a disabled node can still be
  selected, so a keyboard user can find out why.
- **Checkbox trees** (`TreeView::checkable(false)`): a tick is a *rule* on the
  clicked node covering everything beneath it, so it answers for files nobody
  has opened and for files created later; `check_rules().rules()` is the saved
  form ("`/home`, except `/home/u/.cache`"). Design and trade-offs:
  `design-decisions.md` §868.
- **`DirectoryTree::open(root, options)`** / **`open_checkable`**: a directory
  as a tree, each folder read when it is opened, links shown and never
  followed, unreadable folders greyed with the error as the reason, names kept
  as bytes (`OsString` keys) so a path rebuilt from a row is the real file.
  `inclusion_rules()` gives the ticks as paths.

## Where it would go, most valuable first

| application | what it has now | how it maps |
|---|---|---|
| `archivemanager` | `TreeNode`, `FlatTreeRow`, `flatten`, `toggle_path` (`main.rs:389-500`) | the closest fit. `impl TreeSource for TreeNode` with the child's name as key; delete `flatten`, `FlatTreeRow` and `toggle_path`. Its folder pane gains keyboard navigation it does not have today. |
| `jsonviewer` | `TreeViewNode`, `build_tree_nodes`, `expanded_paths: Vec<Vec<PathSegment>>` (`main.rs:975`) | key = `PathSegment` (it needs `Ord`: derive it, `Key` before `Index` or the reverse — the order only has to be total). `expanded_paths` becomes the view's open set; the linear `is_path_expanded` scan goes with it. The type colours stay in the app, drawn in the label via `TreeItem::detail` or kept as today's value column. |
| `devicemanager` | `TreeNode { depth, expanded, visible }` (`main.rs:634`) | two levels, category then device: key = an enum `Category(..) \| Device(id)`. `visible` (the filter) becomes the source returning only matching children. |
| `dbviewer` | `TreeNodeKind`, `build_tree_nodes` (`main.rs:3230`) | section headers as branches, objects as leaves; key = `TreeNodeKind` itself. |
| `diskanalyzer` | `DirTree` / `FileNode` beside the treemap (`main.rs:401`) | a `TreeSource` over `FileNode`, with the size as `detail`. The treemap is untouched. |

Two feature additions, not replacements, that the widget makes cheap:

- **`explorer`: a folder pane.** `DirectoryTree::open(root, DirOptions { files:
  false, hidden: false })` is a navigation tree; a `TreeEvent::Selected` is
  "navigate there". `reveal(path)` keeps it in step when the user navigates
  some other way.
- **Anything that chooses a *set* of folders** — what to back up, what to
  index, what to import. `DirectoryTree::open_checkable(root, options, false)`
  and `inclusion_rules()`. `photomanager`'s "import from a directory"
  (`TD-C-THE-TOOLKIT-CAN-SELECT-A-FOLDER-AND-NO-APPLICATION-ASKS-IT-TO`) is a
  candidate if it should take several folders rather than one.

## What would make a conversion wrong

- **Keeping the application's own flatten pass alongside.** The view's row
  list *is* the flatten; two of them is the drift this exists to end.
- **Keying nodes by row index or by display label.** Labels collide (two
  folders called `src` in different places are different nodes only by path),
  and a lossy label (`to_string_lossy`) names a different file than the one
  shown. Key by the real name or id.
- **Ticking by hand.** Use `TreeView::toggle_check` (or Space / a click on the
  box); it folds a parent whose children all end up alike, which a direct
  `CheckRules::set` does not — that fold is what keeps the box a user sees in
  agreement with what the selection means.

## If it is never done

Nothing breaks. The five hand-rolled trees keep working as well as they do
today — which, for keyboard users, is mostly not at all — and the toolkit
widget stays one more module with tests and no user, the state
`known-issues.md` `TD-C-SIX-TOOLKIT-WIDGETS-ARE-WRITTEN-TESTED-AND-USED-BY-NOTHING`
exists to shrink.

— Lane C
