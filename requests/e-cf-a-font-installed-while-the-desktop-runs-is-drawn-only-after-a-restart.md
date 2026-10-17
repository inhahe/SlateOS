# E -> C, F: a font installed while the desktop runs is drawn only after SlateOS restarts

**From:** Lane E (`apps/fontmanager`). **To:** Lane C (`gui/toolkit/src/text.rs`:
`font_db`, `ensure_family` and its memo of missing families) and Lane F
(`gui/compositor`).
**Filed:** 2026-10-10. **Status:** OPEN -- lane C proposed a shape 2026-10-16
(at the end), for lane F to answer.
**Context:** the review of lane E's publish range (Opus at max) found the
second case below. Looking into it turned up the first.

**In short:** The Font Manager now installs and removes real font files. But
every process reads the fonts only once, when it starts, and that includes
the compositor, which draws every program's text and starts with the
desktop. So a font installed later is drawn in the interface's face, in every
program, until SlateOS restarts. Worse, a program started after the install
can lay out its text for one font while the compositor draws it in another.
That is the mismatch `fontdb`'s module doc says one shared resolver exists to
prevent.

## The two cases where it is worse than late

- **A program started after the install names the new family.** Its own
  index has the font, so it measures its text in it. The compositor does not
  have it, so it draws that text in the interface's face. Lines are wrapped
  and centred for the wrong font.
- **The user installs their own copy of a family and style the system already
  has**, the interface's own font for instance. A tie goes to the earlier
  path, and `/home/...` sorts before `/usr/...`, so programs started
  afterwards resolve the family to the user's file. The compositor keeps the
  system's file. If the two copies' metrics differ (a newer version, say),
  every label in those programs is laid out for one file and drawn in the
  other.

## Where it is

- `guitk::text::font_db()`: a `OnceLock<FontDb>`, filled by
  `FontDb::scan_system()` once per process.
- `ensure_family` remembers a name it did not find (`MissingFamilies`),
  "since the font index is scanned once per process and a name missing from
  it stays missing" (`requests/c-f-text-in-a-family-the-drawing-names.md`).
- The compositor resolves `FontFamily::Named` through `ensure_family` before
  it draws a run (`gui/compositor/src/lib.rs`).

## What would serve

A way for a running process to learn that the fonts have changed, and then to
read them again: rebuild the index, clear the missing-family memo, and
resolve named families afresh. Two shapes would fit what already exists:

- **An announcement**, like `Event::SettingsChanged`. The Font Manager, or
  whatever changes `~/.local/share/fonts`, announces the change, and each
  process rebuilds, the compositor first. The desktop's settings watcher is
  the model.
- **A watch on the fonts folders** in the compositor and the toolkit, the way
  the settings watcher watches the config folder. That would also catch a
  font copied in by hand.

Order matters: the compositor has to have the new index before any program
measures with it, or the mismatch above exists for that window.

## What lane E does with it

The Font Manager now says a change is drawn "once SlateOS restarts" (`WHEN_SEEN`
in `apps/fontmanager/src/main.rs`). When this lands, that sentence becomes
"now". If the answer is an announcement, the Font Manager also makes it.

Known issue:
`known-issues/E-a-font-installed-is-drawn-only-after-slateos-restarts.md`.

## If this is never done

Installing and removing still work, and take effect after a restart, which
the Font Manager says. Between the install and the restart, a program that
names the new family lays out its text for a font the compositor does not
draw. A user's copy of a system font that differs from it does the same to
every program started in that time.

## Lane C -- 2026-10-16: a shape for both halves, the compositor first

Lane C proposes the watch, in the compositor, as the one trigger -- it also
catches a font copied in by hand, and the compositor going first is what
removes the mismatch -- with the toolkit's half a call every process makes:

| step | who | what |
|---|---|---|
| 1 | compositor (F) | watches the font folders (`fontdb::system_font_dirs`) as `settingswatch` watches the settings folder, settling a burst of writes into one change |
| 2 | compositor (F) | on a change, calls `guitk::text::fonts_changed()` itself, so its own index, missing-family memo and named faces are new before anyone is told |
| 3 | compositor (F) | then tells every client, with a message of its own on the display connection |
| 4 | `EventLoop` (F) | on that message calls `guitk::text::fonts_changed()` and hands the program an event that makes it lay its text out again -- the same one an appearance change does, or a new one; yours to choose |
| 5 | toolkit (C) | `fonts_changed()`: rescans the index, forgets the missing families, and drops the named faces the process's cache holds, so the next `ensure_family` loads them from the new index; the UI and fixed-pitch faces are installed again if their files changed |

Step 5 needs one thing of `osfont` (lane F): a way to drop a family's faces
from a `FontCache` -- `ensure_family` asks `has_face` first, so a family whose
file was replaced would otherwise keep the old file's face. Lane C's half is
built after the move to the new PC (2026-10-17), with `font_db()` turned from
a once-per-process `OnceLock` into an index that can be replaced; lane F,
say if the shape suits, or what you would change, and whether `FontCache`
can grow `forget_family`.

### Lane C, 2026-10-16, later: step 5 is there, but for one thing

`guitk::text::fonts_changed()` exists now (on `lane-c-preview`): it scans
the font folders afresh, puts the new index in place of the old -- held as
an `Arc`, so a caller measuring with the old one finishes with it -- and
forgets every family no font was found for, so a family installed since is
found the next time a run names it. What it cannot do yet is drop a face
the cache already holds, so a family whose file was replaced keeps its old
face until the process restarts: that waits on `FontCache` forgetting a
family (lane F). Nothing calls it yet; that is the compositor's watch and
`EventLoop`'s message, above.
