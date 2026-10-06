## 1484. A picture of a theme is the desktop's own drawing, composed headless by the compositor's own code -- and a menu's glass is behind its panels alone

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a theme can now be shown as a picture of the desktop wearing
it -- its wallpaper and icons, the taskbar with the start menu open, a
window of controls in the theme's frame, a notification -- drawn by
`theme-preview nord nord.png`, or by `themepreview::render` from a program.
That is what a theme browser shows a theme by, what a theme repository's
reviewers look at, and the picture a theme made on this machine (derived or
put together, §1483) has no screenshot for. The first picture showed a real
fault, now fixed: while a menu was open the whole desktop was frosted
behind it, not just the part under the menu.

**Where:** `gui/themepreview` (the scene, the composing, the program),
`gui/appearance/src/lib.rs` (`AppearanceSettings::wear_theme`),
`gui/desktop/src/session.rs` (`glass_of`, the glass surface,
`place_glass`). Asked for by `roadmap-detailed.md` §4.6 (*Automated
Validation*: "Preview rendering: CI job renders standardized screenshots";
*Theme Browser*: "Large visual previews as the primary selection
mechanism").

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The picture is drawn by the desktop's own code and composed by the compositor's own, headless** (`DesktopShell`, `WidgetTree`, `Compositor`) | a renderer of its own that imitates the desktop | A picture cannot drift from what the desktop draws: a change to the taskbar, the frames or the glass shows in the next picture with no second drawing to update. | The preview depends on the shell and the compositor, and draws text in whatever faces the machine has. |
| **One standard scene for every theme** -- wallpaper and icons, taskbar, start menu open, a window of controls, a notification | a scene per theme, or a theme's own choice | Two themes' pictures compare part for part, which is what a browser's grid of them is for. | A theme whose character is elsewhere (a terminal's colours, the file manager) is not shown by it: those are lane E's programs, not the desktop's. |
| **A theme is worn over the default settings** (`wear_theme`, every axis it covers, then nothing else) | over the user's own settings | Two pictures differ by their themes alone; a CI renders the same picture on any machine. | The user's accent and text size are not in it. |
| **`wear_theme` is the appearance model's**, answering the axes taken | each caller choosing axes itself | The browser's one-click apply and the picture wear a theme the same way, by `ThemeInfo`'s own `provides_*`. | -- |
| **A menu's glass is behind the panels it fills** (`glass_of`): an empty, click-through surface between the taskbar and the menus asks for the blur, moved to the box round the open menus' fills; the menus' own whole-screen surface asks for none | the whole-screen surface asking for the blur, which the compositor applies to its whole frame | The frosting is where the menu is. Read from what the menus draw, not a list of them, so a menu added later is covered. No protocol change. | Two menus open at once share one box of glass round both; a fill a clip cuts short still has its whole glass. |
| **A whole-screen fill is a scrim, not glass** | counting every fill | A pane that dims the desktop behind it is not frosting it. | A menu drawn as one fill the size of the screen would get no glass. |

### What it does not do

- **The pointer, a terminal, the file manager, the Settings app** are not
  drawn: the last three are lane E's programs, which a desktop-only scene
  cannot draw.
- **The taskbar's clock** shows the time the picture is taken.
