# E -> B: a package could say which font families it carries

**From:** Lane E (`apps/settings`). **To:** Lane B (`userspace/pkg`).
**Filed:** 2026-10-10. **Status:** DONE 2026-10-15 by lane B -- see
"What lane B did" at the end.

**In short:** a theme can recommend the fonts it was designed with (lane C,
design-decisions §1472), and roadmap-detailed asks that Settings "offer to
install recommended fonts from the package manager". Settings knows a
family's name -- "Inter", "Fira Code" -- and `pkg` knows packages, each
providing a list of file paths. Nothing joins the two: no package says it
carries the family "Inter", so Settings cannot find what to install.

## What there is

- `pkg`'s manifest has `provides:` -- file paths (`/usr/bin/mycommand`). A
  font package's would be its font files, whose family names are inside the
  files.
- `pkg search <QUERY>` matches names and descriptions.

## What it asks

A way to ask which package carries a font family, for example:

- in the manifest, `fonts: Inter, Inter Display` -- the families a package's
  font files hold, written by whoever packs it (or read from the files by
  `pkg pack`, which would keep it true);
- and `pkg search --font <FAMILY>` (or a library call) answering the
  packages that carry it, matched as the font index matches family names --
  in any case.

## What lane E then does

On the Fonts page, beside each recommended family that is not installed, an
"Install" that runs `pkg install` for the package that carries it, and says
so when no package does.

## What lane B did (2026-10-15)

- **`fonts:` in the manifest**, written by `pkg pack` from the package's
  font files (`.ttf .otf .ttc .otc`) -- their typographic family (name ID
  16) and legacy family (ID 1), read with `osfont` exactly as the font index
  (`gui/toolkit/src/fontdb.rs`) reads them. A `fonts:` the packer wrote by
  hand is only checked: a family no file holds is left out, with a warning.
  `pkg info` shows the line as `Fonts:`.
- **`pkg search --font <FAMILY>`** answers the available packages that carry
  the family, matched as the index matches a name (trimmed, in any case;
  the words after `--font` are the name, so `pkg search --font Fira Code`
  works unquoted). Output is `pkg search`'s: one `NAME VERSION [installed]`
  line per package, its description indented under it. None is a sentence
  on standard error and status 1, so "no package" cannot be mistaken for
  an answer.

For the Fonts page: run `pkg search --font <family>`; status 0 means the
first word of each unindented line is a package to offer `pkg install`
for.
