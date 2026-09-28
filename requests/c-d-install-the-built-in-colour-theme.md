# C → D — Install `gui/appearance/themes/` as `/usr/share/slateos/themes/`

**From:** Lane C (`gui/appearance`). **To:** Lane D (`scripts/create-ext4-rootfs.sh`).
**Filed:** 2026-09-25. **Status:** OPEN.

**In short:** the desktop has colour themes now (`design-decisions.md` §874).
A theme is a folder holding `theme.yaml`, and the system's themes are read
from `/usr/share/slateos/themes/<name>/theme.yaml`. The tree ships one,
`gui/appearance/themes/aero/theme.yaml` -- the built-in colours written out as
a theme, which is the template a user copies to make their own and what the
theme list describes the built-in theme by. Please stage every folder under
`gui/appearance/themes/` into the rootfs at `/usr/share/slateos/themes/`.

## What to copy

```sh
mkdir -p "$STAGE/usr/share/slateos/themes"
cp -R gui/appearance/themes/. "$STAGE/usr/share/slateos/themes/"
```

The whole directory rather than the one file, so a theme added later needs no
second request. Files only, no build step: a theme is data.

Since 2026-09-26 the built-in theme's folder holds an `icons` directory of
SVG files as well (`design-decisions.md` §880) -- the copy above takes them
with it; nothing more to do. They are compiled into the desktop too, so the
installed copy is the template to edit, not a dependency.

## What happens until it is done

Nothing breaks. The built-in theme's colours are compiled into the desktop, so
it loads with or without the file; without it, the theme list shows the
built-in theme as "Aero" with no author or screenshots, and a user has no
template on the machine to copy (it is in the source tree).

## How to check

On a booted image, `/usr/share/slateos/themes/aero/theme.yaml` exists and
matches the tree's copy byte for byte. `appearance::themes::available()`
(what the Settings theme picker will call) then describes the built-in theme
from it -- `the_built_in_entry_is_described_by_its_installed_file` in
`gui/appearance/src/themes.rs` is that path on the host.
