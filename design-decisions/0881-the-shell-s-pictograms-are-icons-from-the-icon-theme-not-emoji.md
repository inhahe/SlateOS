## 881. The shell's pictograms are icons from the icon theme, not emoji

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The desktop drew its small pictures -- the notification bell,
the volume and brightness pop-ups, the login screen's power buttons, the
battery widget -- as emoji characters. No font the desktop looks for draws
them: the built-in one covers Basic Latin, box drawing and block elements, and
Inter and DejaVu Sans, the UI faces it prefers, have no emoji either. So every
one of them was a box. They are now icons from the icon theme (§880), with the
built-in set drawing all of them, so they appear whatever fonts are installed
and change with the theme.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What names? | the Icon Naming Specification's where it has one (`audio-volume-high`, `media-eject`, `battery-caution`, `network-offline`, `system-reboot`, `start-here`, ...), otherwise the name the large icon sets share (`display-brightness`, `notifications-disabled`, `view-reveal`, `system-suspend`, `avatar-default`, `pan-start`) | names of our own for the rest | a theme made for another desktop then draws these too; ours are the fallback, not the vocabulary. |
| A more specific state than a set draws | name the specific icon (`display-brightness-high`, `audio-input-microphone-muted`, `system-suspend-hibernate`) and let the lookup drop the last part | only the general names | a set that draws the state shows it; one that does not still shows the general picture, never a gap. |
| Where the request is kept | each part that draws keeps its own registry (the shell's menus, the desktop's icons, the overlays, the login screen), and the session's one upload path asks them all | one registry shared by reference | each part stays constructible alone, as every unit test builds it; a surface whose part the session does not ask is caught by the session test that checks every frame's icons went up first. |
| Every surface | the taskbar, the menus, the overlays, the background and the login screen all upload the icons a frame names before sending it, through one function | per-surface code | a surface that sends a frame naming an icon it never uploaded draws nothing there, silently. One door is one place to get it right. |

### What is not done here

- Icons a *program* puts in the tray are still characters it sends
  (`guiremote::tray::TrayIcon::glyph`): naming a theme icon there is a wire
  change, lane F's.
- Typographic marks -- the `✓` beside a chosen row, the `…` that marks a cut
  -- stay text. Every TrueType face the toolkit looks for draws them; only the
  built-in bitmap face does not, and it draws no accented letter either, which
  is a font question rather than an icon one.
