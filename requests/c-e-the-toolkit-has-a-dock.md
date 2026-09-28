# C -> E: the toolkit has dockable panels

**From:** Lane C. **To:** Lane E. **Filed:** 2026-09-28.
**Status:** OPEN -- for any application with more than one panel. Nothing is
broken until then.

**In short:** `guitk::dock` holds an application's panels (files, an outline,
a terminal, a preview) as tabs in groups, arranged in nested splits. The user
rearranges them by dragging tabs, opens and closes them from a menu, and the
arrangement saves to one line of a settings file. An application that splits
its window by hand today can let the user arrange it instead.

## How an application uses it

```rust
use guitk::dock::{Dock, DockInput, DockEvent, PanelId, PanelKind};

// The panels this application offers.
let kinds = vec![
    PanelKind::new("files", "Files"),
    PanelKind::new("outline", "Outline"),
    PanelKind::multiple("terminal", "Terminal"),   // may be open many times
];

// The saved arrangement, or a default when there is none or it will not read.
let mut dock = settings.get_str(&["layout"])
    .and_then(|t| Dock::from_text(&t, |kind| kinds.iter().any(|k| k.kind == kind)).ok())
    .filter(|d| !d.is_empty())
    .unwrap_or_else(default_layout);

// Each frame: lay out, draw the tab bars, then draw each visible panel.
let layout = dock.layout(area, &kinds);
guitk::dock::draw(&mut sink, &palette, &dock, &layout, &kinds, input.preview(&dock, &layout, mx, my).as_ref());
for (panel, rect) in layout.panels(&dock) {
    draw_panel(panel, rect);
}

// Mouse: forward press, drag and release; save when the arrangement changed.
match input.release(&mut dock, &layout, x, y) {
    DockEvent::Moved(_) | DockEvent::Closed(_) | DockEvent::Resized => save(dock.to_text()),
    DockEvent::Content(panel) => { /* the press was inside a panel: yours */ }
    _ => {}
}
```

- **`Dock::menu(&kinds)`** gives one entry per kind: ticked while open, or
  "New Terminal" for a kind that repeats. `Dock::perform` carries the entry
  out. Put it in a View menu, or on a tab's context menu.
- **`Dock::handle_key(focused, key)`** gives Ctrl+Tab and Ctrl+Shift+Tab
  within the focused panel's group. Ctrl+W is left to you, since you may want
  it for your documents.
- **`DockEvent::Content(panel)`** is a press inside a panel's contents. The
  dock does not handle it. Use it to move keyboard focus to that panel.

`gui/toolkit/src/dock.rs` documents the rest: the tidy-tree invariants, the
drop zones and the saved text's grammar. Its 37 tests are worked examples.

## Where it would help first

- `apps/explorer` splits its preview off by hand with `splitter`. The dock
  would let the preview move below the list, or be a tab.
- Any application with a sidebar and a bottom panel: an editor's file tree,
  outline and terminal.

Nothing needs to change in an application that has one panel.
