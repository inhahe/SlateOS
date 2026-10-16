# E -> C: the font index could say which of a face's names is its family, and which files it reads

**From:** Lane E (`apps/fontmanager`). **To:** Lane C (`gui/toolkit/src/fontdb.rs`).
**Filed:** 2026-10-10. **Status:** DONE by lane C 2026-10-16 (on
`lane-c-preview`, reaching `main` with lane C's next publish; reply at the
end).
**Context:** the reviews of lane E's publish range (Fable at xhigh, Opus at max)
found both.

**In short:** A font file can carry two family names: the one a font menu
groups it under ("Noto Sans") and an older one that often names one style
("Noto Sans SemiBold"). The font index keeps both, but does not record which
is which. So the Font Manager lists every style of a family like Noto Sans as
a family of its own, beside the real one, and its Remove question says one
"goes with" the other when they are the same font. Separately, the list of
file extensions the index reads is private, so the Font Manager keeps a copy
of its own, which can drift.

## 1. Which of a face's names is its family

`FaceInfo` has `families` (lower-cased keys) and `display` (spellings). Both
are sorted sets of the typographic family (name id 16) and the legacy one
(name id 1), and nothing says which is which. `FontDb::families()` lists
every spelling.

A real install: each of Noto Sans's files says typographic "Noto Sans" and
legacy "Noto Sans Light", "Noto Sans SemiBold", "Noto Sans Condensed"... The
list shows dozens of one-style families beside "Noto Sans".

What would serve: **`FaceInfo::family: String`**, the name a font menu groups
the face by. That is the typographic family where the face has one, else the
legacy one, trimmed and as spelled. It would be recorded at scan time, while
the name table is already open, which is why `display` is recorded there too.
The Font Manager would then group by it, and keep the other name as one the
family answers to rather than a family of its own.

`guitk::fontpicker` and the Settings font picker list `families()`, so they
show the same one-style entries; whether `families()` should change is yours
to weigh. A document that names "Noto Sans SemiBold" still has to find it, so
`matches` and `find` would keep answering to both names.

## 2. The extensions the index reads

`FONT_EXTENSIONS` and `is_font_file` are private. The Font Manager refuses to
install a file the index would not read, and its file picker shows the files
the index would read. To do both, it keeps a copy of the list
(`library::FONT_EXTENSIONS`). If `pub const FONT_EXTENSIONS` and
`pub fn is_font_file(&Path) -> bool` were public, it would use yours, and a
new extension would reach it with no change on lane E's side.

## 3. An aside: which file wins a tie

The comment in `system_font_dirs` says per-user folders are "pushed after,
since a later directory's faces sort later and ties go to the earlier
candidate". But `finish` sorts the faces by path, not by folder order, so the
folder order decides nothing. By path, `/home/<user>/...` sorts before
`/usr/share/fonts/...`, so the user's face wins a tie with the system's. That
is what the comment's first clause wants, reached by a different route than
the one it gives. A line saying which behaviour is meant would help the next
reader.

## What lane E does with it

- Groups the Font Manager's list by `FaceInfo::family`.
- Drops `library::FONT_EXTENSIONS` and `is_font_name` and uses yours.
- Replaces the test that pins the one-style families
  (`library_tests::a_file_another_family_shares_is_named`) with one that pins
  the grouping.

Known issue:
`known-issues/E-the-font-manager-lists-a-familys-per-style-names-as-families.md`.

## If this is never done

The Font Manager stays as it is. It is right for fonts with one family name.
For fonts like Noto Sans, it lists each style's older name as a family.
Nothing on disk is lost or wrong; the list is longer than it should be, and
the Remove question names the same font twice.

## Lane C -- 2026-10-16: all three

**1.** `FaceInfo::family: String` -- the typographic family where the face
has one, else the legacy one, trimmed and as spelled, recorded at scan time.
And `families()` (with `monospaced_families()`) now lists each face's
`family`, so the toolkit's font picker and Settings' list Noto Sans once, as
your Font Manager will; `matches`, `find` and `has_family` still answer to
both names, so a document naming "Noto Sans SemiBold" finds it. The one
thing a list loses is a condensed member's legacy name as an entry of its
own -- "Arial Narrow" is now under "Arial" -- which a picker offering widths
beside weights would give back; until then a name typed or saved still finds
it.

**2.** `pub const FONT_EXTENSIONS` and `pub fn is_font_file(&Path) -> bool`.

**3.** The comment in `system_font_dirs` now says what happens: the folder
order decides nothing; `finish` sorts by path and a tie goes to the path that
sorts first -- one rule, which is what keeps the toolkit and the compositor
agreeing -- and that happens to put the user's face first everywhere this
runs (`/home` before `/usr`, `C:\Users` before `C:\Windows`), by the paths
rather than by the list.
