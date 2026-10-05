## 557. A wallpaper that cannot be shown costs a wallpaper, not a desktop — and the shell, not the wallpaper model, is what opens the file

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop can now actually display a picture as the wallpaper.
Two things had to be settled to get there. First, *who opens the file*: the
object that holds the wallpaper settings deliberately never touches the disk, so
the code that reads and decodes the `.png` was put in the shell session — the
layer that already talks to the display server — rather than in the settings
object. Second, *what happens when the file is unreadable*: a missing, corrupt
or oversized wallpaper now records an error and lets the rest of the desktop
paint normally, instead of aborting the whole repaint. So a bad wallpaper leaves
you with a plain coloured background and a working taskbar, rather than a
desktop that draws nothing.

### The setting

`WallpaperManager` (`gui/desktop/src/wallpaper.rs`) allocates an *image id* — a
number the display server uses to look up stored pixels — and emits a draw
command naming it. The display server draws nothing, silently, for an id it has
no pixels for. Nothing in the tree had ever stored any, which is
known-issues.md `TD-C-NOTHING-DECODES-A-PICTURE-SO-EVERY-IMAGE-ID-NAMES-NOTHING`.
With `gui/imagecodec` written, the remaining question was where to put the read.

### Decision 1 — the read lives in `ShellSession`, not in `WallpaperManager`

`WallpaperManager` performs no I/O, and says so in its own doc comments in three
places; `populate_slideshow_paths` exists precisely because the *caller* is
expected to scan the directory. Its ~2000 lines of tests all run with no
filesystem at all, and its `get_render_commands` is a pure function of settings
plus a clock.

*Alternative considered:* give the manager a `&dyn Fs`-style hook and let it
load its own picture. That would put the decision in one obvious place and
remove the need for a caller to remember to refresh.

*Why not:* it would make every one of those tests either take a stub filesystem
or stop covering the loading path, and it would give an object whose whole
character is "settings plus arithmetic" a dependency on a decoder and a socket.
The session already owns the connection — uploading is a method on a window
handle — so putting the read there adds no new dependency edge at all.

*Cost accepted:* `paint_background` must call `refresh_wallpaper_image` first,
and a future second painter of the background would have to remember to do the
same. Mitigated by making it the first statement of the only method that paints
that surface, with a comment saying why, and by making it cheap to call — it
compares a remembered `(image id, path)` pair and returns immediately when
nothing changed.

### Decision 2 — an unshowable wallpaper is recorded, not propagated

`refresh_wallpaper_image` returns `Ok(())` for three failures — the file cannot
be read, it cannot be decoded, and the display server *refuses* it (over the
per-connection image budget of §556) — storing the reason in
`ShellSession::wallpaper_error()`. Every other error still propagates.

*Alternative considered:* propagate all of them, so `repaint()` fails loudly and
the caller decides.

*Why not:* the caller is a shell's main loop, and the only thing it can sensibly
do with "your wallpaper is corrupt" is carry on painting. Propagating means the
taskbar, the clock and the start menu do not draw because of a `.png` — a
strictly worse outcome than a plain background, and one the wallpaper's own
design already anticipates: `render_image` always pushes a colour underlay
first, commented *"visible through letterboxing or if image fails to load"*.

*Where the line is drawn:* a `Refused` reply means the display server considered
the request and said no — a fact about this picture. A transport error, a closed
connection or a protocol mismatch is a fact about the *connection*, and the
`submit` two statements later would fail on it anyway; swallowing those would
only move the report somewhere less useful.

*Cost accepted:* the failure is silent to the user until something renders
`wallpaper_error()`, and nothing does yet. Logged in known-issues.md under the
same entry. This is a strictly better position than before — the failure was
previously not merely unreported but *unrepresented*.

### Decision 3 — the failed attempt is remembered, so it is not retried per frame

The remembered pair is written *before* the read, so a wallpaper that fails is
attempted once rather than on every repaint. `paint_background` runs on every
repaint, and a repaint happens on every click that changes anything, so the
alternative is re-reading and re-inflating a 4K file per click — a stutter with
no visible cause.

*Cost accepted:* a user who repairs the file in place, without changing the
path, gets no new attempt until the wallpaper id changes (any `set_image`, any
slideshow step, or a settings reload). Judged the right trade: the failure mode
of retrying is a permanent frame-rate cost on a broken configuration, and the
failure mode of not retrying is one extra click on a repair that is itself rare.

### Decision 4 — a slideshow step drops before it uploads

`refresh_wallpaper_image` releases the previous id *before* reading the next
file, not after uploading it. Uploading first would charge §556's
per-connection image budget for two full-screen pictures simultaneously, so a
budget sized to fit one wallpaper would refuse every slide after the first. The
cost is a window of a few milliseconds in which the old id is gone and the new
one is not there yet — invisible, because the frame that would draw either of
them is submitted after both.

### Where it lives

`gui/desktop/src/session.rs` (`ShellSession::refresh_wallpaper_image`,
`release_wallpaper_image`, `wallpaper_error`, the `wallpaper_image` field),
`gui/desktop/Cargo.toml` (the `imagecodec` dependency),
`gui/window/src/lib.rs` (`WindowHandle::upload_image`/`drop_image`),
`gui/desktop/src/session/tests.rs` (the eight `wallpaper` tests).

**Numbering note.** Lane C's allocated band ended at 499; this continues the
documented overflow past it.
