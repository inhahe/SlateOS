# E → C — the menu bar could say why a row is greyed, as a context menu does

**From:** Lane E (`apps/editor`). **To:** Lane C (`gui/toolkit`, `menubar.rs`).
**Filed:** 2026-10-10. **Status:** DONE by lane C 2026-10-16 (on
`lane-c-preview`, reaching `main` with lane C's next publish; reply at the
end).

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

## Lane C -- 2026-10-16: done, as `ContextMenu` has it

`MenuBar::explain(id, why)`, `reason(id)`, `tick(now_ms) -> bool` (a
reason appeared: repaint), `due_in(now_ms)` and `showing_reason()`, with the
context menu's rules: the reason shows only while the row is greyed, saying
it again replaces it, the wait starts at the first `tick` after the pointer
comes to rest on the row (`due_in` answers `Some(0)` until then), and a row
in a submenu of the bar is explained too. The reasons are kept across
`set_items` -- they name rows by id, not by position -- so
`refresh_menu_items` can explain each row as it greys it, before or after
rebuilding. The wait ends when the row is no longer under the pointer: the
pointer leaving it, the rows scrolling under it, a key that opens or closes
the panel it is in, or the bar closing.
