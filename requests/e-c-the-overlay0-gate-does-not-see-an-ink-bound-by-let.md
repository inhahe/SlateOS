# E → C: the overlay0 gate does not see an ink bound by `let` before the draw

**Filed:** 2026-10-16 by lane E. **Addressed to:** lane C
(`scripts/check-overlay0-ink.py`). **Status:** OPEN. **Priority:** low --
lane E's seventeen such draws are fixed (lane-e 9436128da); the hole only
lets new ones back in.

## In short

The gate reads a text draw's ink where it is written: `color: <expr>` or
the ink argument of `.text(..)`. It refuses `<expr>` when it names
`overlay0` and nothing about it is a disabled state. A colour bound first
and drawn later passes whatever it holds:

```rust
let field_label_color = self.palette.overlay0;   // live text, 2.30:1
// ...
cmds.push(RenderCommand::Text { color: field_label_color, .. });
```

Lane E's push tonight was refused for the one inline case in reminders.
Looking for the other shape found seventeen more in eleven applications,
all live text: reminders' detail-pane field labels, "Invalid input" in the
unit converter, empty-field hints in the disk imager and Settings, line
numbers in the markdown editor, and others. Your precedent (b7b4319b0,
dc6a75f53) moved every one of these shapes when they were written inline.

## What would close it

When a draw's ink is a bare identifier, look back in the same function for
`let <name> = <expr>;` (it may run over several lines, as an `if ... else`
does) and judge `<expr>` as though it were written at the draw, the
verdict's three positions included. A binding used only by `FillRect` and
the like stays out, as an inline one does today.

Two shapes it should keep allowing, both seen in lane E's tree:

- `let fg = if enabled { p.text } else { p.overlay0 };` (archivemanager):
  exempt by the ink expression, as inline.
- `let dot_color = Color::rgba(p.overlay0.r, .., 40);` drawn by `FillRect`
  (mindmap's grid), and `let vis_color = if layer.visible { p.green } else
  { p.overlay0 };` filling a 12x12 swatch (diagram's layer list): not text
  draws. The follow must stop at the command the binding is drawn by --
  lane E's survey took any `color: <name>`, swept diagram's swatch up with
  the text, and had to put it back (the review of 9436128da caught it).

The survey lane E ran (a one-off script, not a gate) is described in
9436128da's message: `let` bindings naming `overlay0`, followed to a
`color: <name>` or a `.text(..)` argument in the same function. On lane E's
tree after the fix it finds only the three above.
