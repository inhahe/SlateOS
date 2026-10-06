# C → E — A click on a submenu's row closes the whole menu

**From:** Lane C (`gui/toolkit`). **To:** Lane E (`apps/**`).
**Filed:** 2026-10-06. **Status:** OPEN.

**In short:** in Explorer, Notes and Photo Manager, clicking a menu row that
opens a submenu -- "Sort by", "View", "Open with" -- closes the whole menu
instead of opening the submenu. Hovering the row still opens it, so the
pointer that rests before it clicks gets there; a click, a touch screen, or a
tool pressing the row does not. The toolkit could not tell you the
difference until now: `ContextMenu::handle_click` answered `None` both for a
miss and for a row that opened its submenu, and each of the three programs
closes its menu on `None`. `ContextMenu::click` (lane C, 2026-10-06) answers
which it was. The fix in each program is a few lines.

## What there is

`guitk::menu::ContextMenu::click(x, y) -> MenuClick`:

- `MenuClick::Chosen(id)` -- a row was chosen; the menu has closed itself.
- `MenuClick::Opened` -- a row's submenu opened (in the menu, or in a
  submenu of it); the menu stays open.
- `MenuClick::Missed` -- a line, a greyed row, the padding, or outside the
  menu (which closed it). Close the menu on this, as now.

`handle_click` is unchanged (`Chosen(id)` is `Some(id)`, the other two
`None`), so nothing breaks; it simply cannot tell an opening from a miss.

The desktop shell's own menu switched to `click` in the same change, and its
"View" and "Add widget" rows now open as they do on every desktop.

## What is asked

In each of these, use `click` and keep the menu on `MenuClick::Opened`:

| Program | Where | Today |
|---|---|---|
| Explorer | `apps/explorer/src/main.rs`, `click_menu` | `handle_click`, then `self.menu = None` whatever it answered |
| Notes | `apps/notes/src/main.rs`, the `note_menu` press arm | `handle_click`, then `self.note_menu = None` |
| Photo Manager | `apps/photomanager/src/main.rs`, the `photo_menu` press arm | `handle_click`, then `self.photo_menu = None` |

The shape, from the shell (`gui/desktop/src/lib.rs`, the desktop menu's
press arm):

```rust
match menu.click(x, y) {
    MenuClick::Chosen(id) => { /* close, then act on `id` */ }
    // A row that opens a submenu opened it: the menu stays.
    MenuClick::Opened => {}
    // A miss closes the menu, and the press is spent doing so.
    MenuClick::Missed => { /* close */ }
}
```

A test for each: a click at the middle of the submenu row
(`ContextMenu::item_rect(index)`) leaves the menu open with its submenu
shown, and a click on a row in the submenu then chooses it.

## Also new, for whoever builds menus

A context menu now shows tools its rows itself (`guitk::menu::MenuPart`, the
`Accessible` implementation): a program that puts its menus in its own
automation tree gets each row's label, its keys or why it is greyed, its
tick, and the submenu it opened. A program that acts on its menus through its
own click handling -- as all three of these do -- presses a row for a tool
with `ContextMenu::press_point(id)`, which scrolls the row into the menu and
says where to click.
