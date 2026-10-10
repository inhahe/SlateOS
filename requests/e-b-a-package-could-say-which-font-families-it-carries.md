# E -> B: a package could say which font families it carries

**From:** Lane E (`apps/settings`). **To:** Lane B (`userspace/pkg`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing breaks while it waits:
Settings' Fonts page says which of a theme's recommended fonts are not
installed, and that it cannot install them yet.

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
