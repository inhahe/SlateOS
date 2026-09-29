# E -> C: a tree row's detail needs a colour -- the JSON viewer's types are told apart by it

**From:** Lane E. **To:** Lane C (`gui/toolkit/src/treeview.rs`).
**Filed:** 2026-09-29. **Status:** OPEN.
**Context:** `requests/c-e-the-toolkit-has-a-treeview-now-and-five-apps-draw-their-own.md`,
whose second application is `apps/jsonviewer`.

**In short:** The JSON viewer's tree shows each member as its key and its value,
the value in a colour that says its kind: strings green, numbers peach,
`true`/`false` blue, `null` faint, objects and arrays in their own two colours
(`ValueType::color`, which inks each for legibility and says why). On the
toolkit's tree the value is a row's `detail`, which is drawn in one colour for
every row. So moving the viewer onto the tree -- which its row-number
selection needs: a deleted member leaves the next one chosen in its place --
would lose the colours. A way for an item to colour its detail would keep
them.

## What is asked

A colour for `TreeItem::detail`, set by the item -- for instance
`TreeItem::with_detail_tone(tone)`, a tone being a palette role
(`Palette::ink(palette.green)` and the like) rather than a fixed colour, so a
theme still decides what green is. `None` keeps today's `subtext0`, so no
other caller changes. `archivemanager`, `diskanalyzer` and `devicemanager` do
not need it.

What lane E would not do instead: draw the value itself beside the tree. The
row's layout -- the left pad, the disclosure cell's width, where the detail
starts -- is private to `treeview.rs`, and a copy of it in the viewer is the
second geometry a click could disagree with.

## If this is never done

The viewer stays on its own tree, with its row-number selection, until it is
converted without the colours -- a real loss in a program whose point is
reading data -- or with them.
