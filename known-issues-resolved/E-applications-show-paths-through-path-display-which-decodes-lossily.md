### [E] Applications show paths through `Path::display`, which decodes lossily -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26). Every `Path::display` / `OsStr::display`
under `apps/` -- 368 calls in 68 files, found by clippy itself rather than by
pattern (the fourteen application types with a `display()` of their own are
not paths) -- now calls `pathtext::ShowPath::shown` (new crate `apps/pathtext`),
which renders through `quoting::escape_unprintable`. `apps/clippy.toml` names
both methods in `disallowed-methods`, and the workspace denies `clippy::all`,
so a new one fails the boot test's clippy gate. design-decisions.md §1213.
Every site was then read, because a dozen were not displays at all but uses
-- an M3U line, a persisted key, a feed address read back as a path, a path
box, a linker argument, suggested file names -- and those now keep the name
exactly (see §1213's table). `pathtext` gained `text_or_shown` for them, and
ten per-application name helpers now go through `pathtext`.

The original entry, for the record:

**Was:** 449 uses in 73 files under `apps/` (measured with
`git grep -c "\.display()" -- 'apps/*/src/*.rs'`), most in status lines and
error messages. Fixed where it is a window's own name: the file manager's
title (2026-09-26), the image viewer's error messages (`shown_path`).

**In short:** a file or folder whose name holds bytes that are not text is
shown with a replacement character in their place, so two such names can
look the same on screen, and a message about one can seem to be about the
other. Nothing is lost -- the path itself is untouched -- but the user cannot
tell the files apart. The lossy-decode gate (`scripts/lossy-decode.py`)
catches decodes whose result is *kept*; `display()` builds only a string to
show, so the gate does not see it.

**The proper fix:** one shared way to show a path -- the escaping
`quoting::escape_unprintable` already gives names (`\351` for a byte that is
not text) -- applied at every site, and a ratchet like the lossy-decode one
so no new `display()` reaches a user. Mechanical, but 449 sites; worth doing
as one sweep rather than piecemeal. The per-app `shown_name`/`shown_path`
helpers written this month are the seed of the shared function.
