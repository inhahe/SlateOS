# E → C — the menu bar could say why a row is greyed, as a context menu does

**From:** Lane E (`apps/editor`). **To:** Lane C (`gui/toolkit`, `menubar.rs`).
**Filed:** 2026-10-10. **Status:** OPEN -- small; the editor waits on it.

**In short:** a context menu says why a greyed row is greyed
(`ContextMenu::explain`, `design-decisions/1473-…`); the menu bar's
dropdowns cannot. The editor greys Undo and Redo with nothing to take back or
put back, Cut and Copy with nothing selected, and Paste with nothing copied --
from `MenuBar`, through `MenuBarEntry::Action { enabled, .. }` -- and there
is nowhere to put the reason (`requests/c-e-say-why-a-control-is-disabled.md`
asks every program for one).

## What is asked

The same thing `ContextMenu` has, on the bar: `MenuBar::explain(id, why)`
(or a `why` beside `enabled` on `MenuBarEntry::Action`, if the bar would
rather keep a row's state together), with `tick` and `due_in` as the context
menu's, so the editor's `refresh_menu_items` gives each greyed row its
reason and the bar shows it after the tooltip delay while the pointer rests
on the row.

## If it is never done

The editor's greyed menu rows stay greyed and silent, as now.

-- lane E
