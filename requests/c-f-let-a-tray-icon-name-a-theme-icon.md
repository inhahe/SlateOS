# C → F — Let a tray icon name an icon from the icon theme

**From:** Lane C (`gui/desktop`, `gui/appearance`). **To:** Lane F (`gui/remote`).
**Filed:** 2026-09-26. **Status:** OPEN -- a proposal; the wire is yours.

**In short:** a program's tray icon is a *character* it sends
(`guiremote::tray::TrayIcon::glyph`, at most 32 bytes), and the taskbar draws
it as text. Most of the pictures a program would want there -- a battery, a
network, a speaker -- are emoji, and no font SlateOS has draws emoji: the
built-in face covers Basic Latin, box drawing and block elements, and Inter
and DejaVu Sans have none. So a program's tray icon is a box, where every
picture the shell draws itself is now an icon from the icon theme
(`design-decisions.md` §880, §881; `known-issues.md`
`TD-C-THE-SHELL-DREW-ITS-PICTURES-AS-EMOJI-NO-FONT-IT-HAS-CAN-DRAW`).

**The proposal:** an optional icon *name* beside the glyph.

- `TrayIcon { owner, id, glyph, tooltip, icon_name: Option<String> }`, the
  name a freedesktop-style icon name (`battery-caution`, `network-offline`,
  `audio-volume-muted`), at most 64 bytes of `[a-z0-9_-]` -- what
  `appearance::icons::is_valid_name` accepts, so the shell can refuse anything
  else before it reaches a path.
- On the wire, a new frame version (or a flag bit in the existing `flags`
  byte) with the name as one more length-prefixed string; a version-1 frame
  decodes with `icon_name: None`, so no program has to change.
- The shell draws the named icon from the chosen theme, in the bar's own ink
  as it draws the glyph (§842: the tray is shell chrome), and falls back to
  the glyph when the name is absent or nothing draws it. The built-in set
  already draws the names a tray most needs: the battery's four states,
  `network-idle`/`network-offline`, the four volume levels, the microphone,
  `notifications`.

A name rather than pixels, because a name follows the theme and the light or
dark mode, costs a few bytes, and lets the shell upload each picture once;
pixels would be one more image format on the wire and a copy per program.

## What lane C does once it exists

`DesktopShell::render_taskbar` draws `icon_name` through the same path as the
bell and the start button (`icon_in`, `send_frame` uploads it), with the
glyph as the fallback; `apply_tray_icons`' change detection already compares
whole `TrayIcon`s, so a program switching icons repaints. A test in the
style of `the_taskbars_own_pictures_are_icons_the_built_in_set_draws`.

## What happens until it is done

Nothing breaks: programs keep sending glyphs, and the ones a font can draw
(letters, box drawing) still draw. The ones it cannot are boxes, as today.
