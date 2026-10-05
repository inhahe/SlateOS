### [C] TD-C-MENU-ROWS-DRAW-NO-ICONS -- 2026-09-29

**Status:** FIXED 2026-09-29 (lane C). `icon` is documented as an
icon-theme name or an absolute path, as a desktop entry's `Icon` is;
`ContextMenu::render_with_icons` draws it in the column before the label
(a check mark wins that column), finding each picture through a resolver
its owner passes -- the toolkit cannot know the theme or the uploads --
and hands the same resolver to an open submenu; `render` stays
picture-free. The desktop's menus resolve through its icon registry in the
menu's text colour (`DesktopShell::render_menu`): a jump list row shows its
action's picture, an item a program added to a file's menu its own, a
submenu its first item's. A name the theme lacks draws nothing and leaves
the label in line.

**In short:** a menu row has a field for its picture, and nothing draws it.
Every menu in the desktop and the applications is text only -- which is
fine while nothing asks for a picture, and now something does: an item a
program adds to a file's right-click menu names its icon
(`Icon=object-rotate-right`), as KDE shows it.

**Where:** `guitk::menu::MenuItem::{Action, Submenu}` carry
`icon: Option<String>`; `ContextMenu::render` never reads it, and every
caller passes `None`. The desktop's service-menu rows
(`DesktopShell::service_menu_items`) pass `None` too, with a comment, though
`servicemenus::MenuAction::icon` and `Row::Submenu::icon` hold the names.
What the field means -- an icon-theme name, a path, a glyph -- was never
written down.

**The proper fix:** say that `icon` is an icon-theme name or an absolute
path, as a desktop entry's `Icon` is; have `ContextMenu` reserve a column
for it when any row has one and draw it through the icon theme the taskbar
already resolves program pictures with; then pass the service menus' names
through.
