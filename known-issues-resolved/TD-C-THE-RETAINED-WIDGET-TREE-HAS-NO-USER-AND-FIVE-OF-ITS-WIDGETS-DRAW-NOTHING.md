## `TD-C-THE-RETAINED-WIDGET-TREE-HAS-NO-USER-AND-FIVE-OF-ITS-WIDGETS-DRAW-NOTHING` (lane C, 2026-09-27) -- **FIXED 2026-10-01**

**Status:** FIXED 2026-10-01, all three points -- the tree still has no
program using it, which is now a choice for programs rather than a defect.
The layout half was §1455's (flexbox and grid, and clicks routed through
it). Then: the tree draws in the palette its program gives it
(`WidgetTree::set_palette`), every kind through its component module
(`gui/toolkit/src/widget/draw.rs`: `button`, `checkbox`, `radio`, `slider`,
`field`/`textedit`, `textarea`, `scrollbar`), so one control cannot be drawn
two ways -- the tests compare the tree's commands with the module's for the
same box. `Style`'s text and selection colours are optional, unset meaning
the palette's (they defaulted to black and Windows' blue). The five silent
kinds draw and answer: `TextArea` holds the toolkit's `TextArea` (press,
drag, keys, wheel); `RadioButton` is chosen by a click or Space and clears
its sibling radio buttons; `Slider` holds the toolkit's `Slider` (press,
drag, keys, the wheel while focused); `ScrollView` lays its content out at
its width and unbounded height, scrolls under the wheel within it, cuts it
to its box and draws its bars -- across its foot too, by the scrollbar
module's `draw_across`, a column's bar laid on its side; `Image` draws its
picture. Buttons and boxes light under the pointer.

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
