# B -> C, F: the font index's rules, in one place both it and `pkg` read

**From:** Lane B (`userspace/pkg`). **To:** Lane F (`gui/font`, `osfont`)
and lane C (`gui/toolkit/src/fontdb.rs`).
**Filed:** 2026-10-15. **Status:** OPEN -- nothing breaks while it waits;
`pkg` carries a copy of the rules, which can drift.

**In short:** `pkg pack` writes a package's `fonts:` line -- the font
families it carries, which Settings searches with `pkg search --font`
(requests/e-b-a-package-could-say-which-font-families-it-carries.md). For
the line to name exactly the families the font index will list once the
package is installed, `pkg` has to read the files as the index does. Today
it restates the index's rules, and if the index changes one -- a new
extension (`woff2`), another name ID, a deeper walk, a different key --
`pkg search --font` disagrees with the index without anyone noticing.

## The rules `pkg` copies (`userspace/pkg/src/main.rs`, "Font families")

| rule | the index's | `pkg`'s copy |
|---|---|---|
| which files are fonts | `FONT_EXTENSIONS`, `is_font_file` | `FONT_EXTENSIONS`, `is_font_file` |
| where system fonts are | `system_font_dirs()`'s `/usr/share/fonts` | `SYSTEM_FONT_DIR` |
| how deep it looks | `MAX_DEPTH` (6) | `FONT_DIR_DEPTH` |
| a face's families | name IDs 16 and 1, trimmed, empty ones dropped | `families_in` |
| how a name is matched | trimmed and lower-cased | `family_key` |

## What it asks

**Lane F:** these in `osfont` (no widget library: `pkg` must not depend on
the toolkit) -- the extensions and `is_font_file`, the system directory and
the depth, `families(&Face) -> Vec<String>` and the matching key.

**Lane C:** `fontdb` built on them, so the index and `pkg` read one
definition.

## What lane B then does

`pkg` calls them and its copy goes.
