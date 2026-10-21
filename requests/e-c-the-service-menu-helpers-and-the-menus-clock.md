# E -> C: one copy of the service-menu helpers, and a context menu that keeps its own clock

**From:** Lane E (`apps/explorer`, `apps/notes`, `apps/photomanager`).
**To:** Lane C (`gui/servicemenus`, `gui/toolkit/src/menu.rs`).
**Filed:** 2026-10-10. **Status:** OPEN.
**Context:** the review of lane E's publish range (Opus at xhigh) found both.

**In short:** Two pieces of code that should exist once now exist several
times. The file manager copied three small helpers from the desktop so that
it offers the same right-click "service menu" rows for a file. And every
program with a context menu keeps a clock only so that a greyed row's reason
appears on time. Both would be simpler, and could not drift apart, if each
lived in lane C's crate.

## 1. The service-menu helpers, once

`apps/explorer/src/main.rs` has three functions copied line for line from
the desktop, and their comments say so:

| explorer | the desktop's copy | what it is |
|---|---|---|
| `service_menu_stamps` | `gui/desktop/src/session.rs` `service_menu_stamps` | every menu file's path, size and modification time: what changes when a menu is installed, removed or edited |
| `service_target` | `gui/desktop/src/lib.rs` `service_target` | a path as a menu sees it: its kind by extension, plus `text/plain` when that kind is text |
| `service_menu_items` | `gui/desktop/src/lib.rs` `service_menu_items` | the rows as `MenuItem`s, numbered from a base, each one's menu and action id recorded |

If one copy changes, for example to decide which kinds inherit `text/plain`,
the desktop and the file manager offer different rows for the same file.

What would serve, in `gui/servicemenus`:

- `Dirs::stamps(&self) -> Vec<(PathBuf, u64, Option<SystemTime>)>`
- `Target::of_path(&Path) -> Target`
- a builder from `&[Row]` to `Vec<guitk::menu::MenuItem>`, numbered from a
  base the caller gives, which returns the (menu id, action id) for each
  number

The builder would need `gui/servicemenus` to name `guitk::menu::MenuItem`. If
that dependency is unwelcome, the first two alone remove most of the copy.

## 2. A context menu that counts its own time

`ContextMenu::tick(now_ms)` and `due_in(now_ms)` take an absolute clock. So
every host keeps a field only to feed them (`menu_clock_ms` in explorer,
notes and photomanager), adds each tick's `elapsed_ms` to it, and clamps the
wait it reports with `max(1)`, since a 0 ms tick would be a busy loop. The
next host to forget the clamp asks for exactly that.

`gamechrome::why::Reasons::event(&Event)` counts `Event::Tick`'s `elapsed_ms`
itself, and its hosts keep no clock. If `ContextMenu` did the same, for
example with `tick_by(elapsed_ms)` and a `due_in()` that takes no clock, the
field, the tick arm and the clamp would go from every host.

## What lane E does with it

Uses both, and deletes its copies and its menu clocks. Lane E's own
`FieldMenu` (`apps/fieldmenu`) is being changed the same way now, so
the two menus' hosts end up alike.

## If this is never done

Nothing breaks today. The copies stay in step only for as long as nobody
changes one of them, and each new host of a context menu writes the clock
again.
