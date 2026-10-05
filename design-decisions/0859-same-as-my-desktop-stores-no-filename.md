## 859. "Same as my desktop" stores no filename

**In short:** the login screen can use the same background picture as the
desktop. The obvious way to build that is to copy the desktop's current
picture into the login setting when the user ticks the box. We do not: we store
only the fact that it should follow, and ask the desktop what it is showing at
the moment the screen is painted. If we stored the filename, the machine would
keep showing whichever picture happened to be up when the box was ticked, for
ever, while still calling itself "same as my desktop".

**Date:** 2026-09-17. **Lane:** C. **Decided by:** Claude (autonomous).

**The setting.** `appearance.yaml` gains `login.background`, one of `theme`,
`color`, `desktop`, `image`, `gradient`. `desktop` is
`LoginBackground::SameAsDesktop`, and the variant carries no payload at all.

**Why the payload would be wrong.** The desktop's picture is not a constant.
A rotation folder (landed the same day) replaces it every
`wallpaper.interval_secs`; the time-of-day gradient changes it continuously.
A `SameAsDesktop(PathBuf)` would be a snapshot taken at the instant the user
chose the mode, and would go stale on the first rotation — a setting whose name
says "same as" while showing something else. Holding nothing is what makes
the name true.

**What it costs.** One indirection at paint time: the session asks
`WallpaperManager::current_image_path()` rather than reading a stored field.
That is also the mechanism by which a rotation reaches the greeter for free:
there is no separate code watching for the wallpaper to change, because
nothing was cached to invalidate.

**A second upload, not a second reference.** Uploaded images belong to the
window that uploaded them (`Window::images` in the compositor), and the
greeter has its own surface. So "the same picture" is decoded and uploaded
twice, once per surface, and the two carry different ids. This is not
duplication that could be optimised away by sharing an id: an id is only
meaningful to the window that registered it.

**The alternative considered, and rejected.** Store the resolved path and
refresh it whenever the wallpaper changes. It needs a notification path from
the wallpaper to the greeter that does not otherwise exist, and it has a
failure mode the chosen design cannot have: if the notification is ever
missed, the greeter shows a stale picture and nothing detects it. Asking at
paint time cannot be missed, because there is nothing to miss.

**Where it bites.** `gui/appearance/src/lib.rs` (`LoginBackground`, which
lives there rather than in the greeter so that the setting and the drawing are
one type — see 857), `gui/desktop/src/session.rs`
(`refresh_login_image`, `sync_login_background`), and
`gui/desktop/src/login_screen.rs` (`render_background`).
