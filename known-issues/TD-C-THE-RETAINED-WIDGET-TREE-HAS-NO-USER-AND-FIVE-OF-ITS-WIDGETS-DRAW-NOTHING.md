## TD-C-THE-RETAINED-WIDGET-TREE-HAS-NO-USER-AND-FIVE-OF-ITS-WIDGETS-DRAW-NOTHING (lane C, 2026-09-27)

**Status:** OPEN

**What.** `gui/toolkit/src/widget.rs` -- `Widget`, `WidgetKind` and
`WidgetTree`, the toolkit's retained widget tree with its flexbox layout
(`layout.rs`) -- has no program using it. `apps/diskimager`, `apps/filediff`
and `apps/hexeditor` import `Widget`, `WidgetId` and `WidgetTree` and construct
none; `apps/kanban` uses it in one test. Every window in the tree is drawn by
the toolkit's component modules (`button`, `textinput`, `treeview`, `slider`,
...) into a `Frame`, or by hand.

Because nothing drives it, it has decayed without anyone seeing:

- **Five declared widgets draw nothing and answer nothing:** `TextArea`,
  `RadioButton`, `ScrollView`, `Slider` and `Image` fall through the render
  match's `_ => {}`. A program that built a form with a slider in it would get
  a blank space. (`Slider` was found while `guitk::slider` was written,
  2026-09-27, `design-decisions.md` §1431 -- the component exists now; the
  tree's variant does not use it.)
- **It draws with its own colours, not the user's theme.** `render` takes no
  `Palette`; widgets are drawn in `Style` colours with fixed defaults, and
  `ProgressBar` fills with `#0078D7`, Windows' selection blue -- the colour the
  palette conversion removed from everywhere else (838).
- It routes no events for any program (see
  `TD-C-EVERY-KEYSTROKE-WENT-TO-THE-LAST-TEXT-FIELD-IN-THE-WINDOW`, whose fix
  noted that no application routes events through `WidgetTree::handle_event`).

**Why it is not fixed in passing.** Making one variant real is not the fix:
the tree has no palette to draw a themed slider with, and threading one
through it is the same work for all of its widgets. The real question is what
the tree is *for*. `roadmap-detailed.md` §3.5 asks for a layout engine (flexbox,
grid, sizing to content) and the tree is the only one the toolkit has, so
retiring it outright would drop that; keeping it means rendering every
`WidgetKind` through the component modules and a `Palette`, so the tree and
the components cannot draw one control two ways.

**Proper fix.** Make the tree a layout-and-routing layer over the component
modules: `render(&self, palette, sink)` draws each kind through its module
(`guitk::button`, `guitk::slider`, `textinput`, ...), the five silent kinds
either render through a module or are removed until one exists, and the
hardcoded colours go. Do it when a program first wants the tree's layout --
or sooner, if the §3.5 layout items are picked up, since they are the same
work. Until then nothing is broken for a user, because no user reaches it.
