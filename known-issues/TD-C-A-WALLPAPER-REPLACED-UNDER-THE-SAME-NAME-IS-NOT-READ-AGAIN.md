## TD-C-A-WALLPAPER-REPLACED-UNDER-THE-SAME-NAME-IS-NOT-READ-AGAIN (lane C, 2026-10-05) — OPEN

**Status:** OPEN — repaired on lane C's branch 2026-10-05 (`lane-c-wip`);
closes when that change reaches main after a boot test. The session keeps
each picture's file stamp -- its length and when it was written -- beside
the request (`wallpaper_stamp`, `login_image_stamp`), and asks again when
it differs: `sync_wallpaper` for the desktop's fixed, scheduled or theme
picture, `sync_login_background` for the greeter's own or the one it
shares. Both run at every appearance announcement, also when no setting
changed (`adopt_appearance_change`), so the watcher needed no stamp of
its own. Tests: `a_wallpaper_saved_over_under_the_same_name_is_read_again`,
`a_login_picture_saved_over_under_the_same_name_is_read_again`. A file
replaced with nothing announcing it is still noticed only at the next
announcement -- the trigger below stands for that part.

**In short:** if you save a new picture over the one the desktop is showing
-- the same file name, new contents -- the desktop goes on showing the old
picture until you choose a different one or sign in again. Choosing a
picture by another name is shown at once. The same holds for a theme's
recommended picture (design-decisions 1471) and a time-of-day schedule's.

**Where:** `gui/desktop/src/session.rs`. `sync_wallpaper` asks for a new
image only when the path differs from the one up
(`self.wallpaper.current_image_path() != Some(path)`), and
`refresh_wallpaper_image` remembers what it asked for as `(id, path)`. Nothing
records what the file held, so the same path is the same picture.

**To reproduce:** set `wallpaper: /home/<you>/Pictures/a.png` in
`appearance.yaml`, sign in, copy another picture over `a.png`, then save any
appearance setting in Settings (which sends `ReloadAppearance`). The old
picture stays.

**The fix:** remember the file's identity with the request -- its length and
modification time, read when it is asked for -- and ask again in
`sync_wallpaper` when either differs, as well as when the path does. The
appearance watcher must then count the same stamp
(`appearance::dependencies`), or a Settings save that leaves
`appearance.yaml`'s bytes unchanged is reported as no change and
`sync_wallpaper` never runs. Both only help once something looks: like an
edited theme file (`TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE`),
a replaced picture is noticed at the next settings announcement, not when it
is written, until there is a way to be told a file changed. **Trigger:** a
file-change notification service, or the first program that writes a
picture the desktop may be showing (a wallpaper editor, a "set as
wallpaper" that overwrites).
