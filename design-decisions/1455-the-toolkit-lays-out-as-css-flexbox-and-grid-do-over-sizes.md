## 1455. The toolkit lays out as CSS Flexbox and Grid do, over sizes

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** A program's window is made of boxes in rows, columns and
grids. The toolkit now places them the way web pages are laid out --
rows that wrap onto new lines when they run out of room, columns of
labels as wide as their widest label, grids of rows and columns where
some columns take a fixed width and others share what is left -- so that
whoever builds a window gets the behaviour they already know from the
web, without the toolkit reading any style sheets.

**What was there.** `guitk::layout::flex_layout` was a sketch: wrapping
and `align_content` declared and ignored, no minimum or maximum sizes,
margins drawn without room left for them, an item that grew in a
content-sized container infinitely wide; no grid at all; and the widget
tree drew, laid out and hit-tested its children in three different
coordinate spaces (`roadmap-detailed.md` -> *Layout Engine*, every item
unchecked).

**How it works:** `flex_layout` (CSS Flexbox §9's steps, in order),
`grid::grid_layout` (CSS Grid's placement and track sizing, for pixel,
content-sized, shared and min/max tracks) and `fit_image` (CSS's
`object-fit` and `object-position`) are functions from sizes to boxes.
The widget tree feeds them each child's natural size, margins and
limits, and places, draws and hits each child in its parent's content
space.

**Choices:**

| Choice | Taken | Alternative | Why |
|---|---|---|---|
| Whose rules | CSS's, step for step | a toolkit's own simpler rules | the behaviour people already expect, and a specification to check the arithmetic against: every test's numbers are worked out by hand from it |
| A reversed row or wrap | the forward layout mirrored | placing from the far end item by item | it is what CSS's swapped start and end sides come to, and it makes every `justify` value right at once -- the old code ignored `justify` when reversed |
| A share's least size (`Fr`) | the largest `min` of its items | CSS's default, the content's size | a share that grows to its content makes two "equal" columns unequal whenever a label is wide; a stated minimum is what a form needs. A content-sized minimum is a `MinMax` or `Auto` track away |
| A widget's `min_width`, `max_width` | its border box's (padding and border inside them) | its content's, as CSS's `box-sizing` default | the size a person asks a control to be is the size they see |
| Margins and limits in a container | from each child's style | from its flex item alone | the style is where they are set for a widget alone; one place, so a limit holds in a row as it does on its own |
| The box a layout answers | the margin box's corner, the item's margin with it | the border box's corner | `LayoutBox::content_x` already reads it that way; one meaning across flex, grid and the widget tree |
| Opacity | each colour's alpha scaled, a child's on top of its parent's | a group opacity in the compositor | the render protocol has no group opacity, and adding one is the compositor's (lane F's); the difference -- overlapping parts of one widget showing through each other, pictures unfaded -- is written where `RenderCommand::faded` is |
| A border whose sides differ | each side its own strip, square-cornered | mitred joins | rounded or mitred joins of differing sides need a path the protocol does not have; four alike are still one rounded stroke |
| Not done | `order`, auto margins, percentages, aspect ratios, named grid lines and areas, dense packing | -- | nothing in the tree has needed them; each is an addition to these functions, not a change to them |
