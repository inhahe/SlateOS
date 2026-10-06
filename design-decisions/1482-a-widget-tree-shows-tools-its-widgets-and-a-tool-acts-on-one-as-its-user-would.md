## 1482. A widget tree shows tools its widgets, and a tool acts on one as its user would

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** every window built from the toolkit's widget tree can now be
read and worked by a tool -- a script, a recorded macro, a screen reader --
without its program writing anything for it. A tool sees each widget as a
node: what it is (a button, a check box, a text field), what it is called,
what it holds, whether it can be used, and where it is on the screen. It can
find "the Save button" by name, and press it, tick a box, set a field's text
or a slider's value, give a widget the keyboard or scroll a pane. Each
action goes through the widget exactly as a click or a key would, so the
program cannot tell a tool from its user. A program can keep a widget out of
the tree; it never has to put one in.

**Where:** `gui/toolkit/src/widget/automation.rs` (`WidgetTree::automation`,
`find`, `invoke`; `Widget::labelled`, `hidden_from_automation`).
`roadmap-detailed.md`, the automation framework's *Automatic Widget-Level
Exposure*.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Every widget is in the tree unless its program keeps it out** (`hidden_from_automation`) | a program adding each widget it wants seen | The roadmap's rule -- "the app opts *out*, not in" -- and the only way a screen reader works with programs whose authors never thought of one. | A decoration is seen until its program says otherwise. |
| **A node is a snapshot built on request** (`automation()`), not a live object kept beside the tree | a second tree kept in step with the first | One tree, so the two cannot disagree; a tool asks again after it acts. | A tool polling a large window rebuilds its nodes each time. |
| **A widget's name is what its program called it, else its own text, a field's placeholder, else its tooltip** | the tooltip first, or the CSS name | What a sighted user reads is what a screen reader says: the button's text, the box's label. `labelled` covers what shows no text -- a slider, a picture. The CSS `#name` is the node's `key`, the identifier a script finds from one run to the next. | A field whose label is a separate `Label` widget is called by its placeholder, not by that label, until labels can name the field they label. |
| **An action goes through the widget as its user's would** -- the same signals, the same group rules, the same restyle after | setting fields directly | The program hears a tool's press as it hears a click; a disabled or hidden widget refuses as it refuses its user; a style on `:checked` follows. | A tool cannot do what the user cannot -- which is the point. |
| **A text area's text set is one undoable edit** | replacing it as loading a document does | It is the user's text being changed for them, and Ctrl+Z takes it back as it would a paste. | -- |
| **A node's box is the widget's border box in window coordinates, whole even where a scroll pane cuts it** | the visible part only | The box is where the widget is; whether it shows is the pane's to say. | A tool pressing the middle of a widget half-scrolled out of view may press beside it. |
| **A component drawn outside the tree shows its parts through a trait, `Accessible`, naming each by its own hit-box target and answering its own event** | a second, tree-shaped model each component keeps | The target a component's frame already records for a part is the part's name; an action answers the event the component's host already handles from a click -- a pick -- so the host has nothing new to learn. The character picker is the first (`gui/charpicker/src/accessible.rs`), its boxes tested against its frame's. | Each component writes its own: the font picker, the file dialog, the colour picker and the shell's surfaces have yet to. |

**Not done here, and why:**

- *Other processes.* The tree and its actions are in the program's own
  process; carrying them over the automation channel, behind
  `automation.ui_inspect` and `automation.ui_control`, is `libautomation`'s,
  which does not exist yet (lane B's `userspace/`). It needs only these
  three calls.
- *Most components drawn outside the tree.* The hook is there
  (`Accessible`) and the character picker uses it; the font picker, the file
  dialog, the colour picker and the shell's own surfaces -- the taskbar, the
  start menu, the tray -- have still to name their parts.
- *A field's label naming it.* Nothing yet says which label is a field's;
  until something does, a field is called by its placeholder or by
  `labelled`.
