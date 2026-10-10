## 1469. A theme is checked by the desktop's own readers before it is installed or shared

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a theme someone shares, or installs, should first be checked
for what the desktop would refuse, ignore or change in it: a colour that is
not a colour, an icon that draws nothing, text too faint to read, a script
hidden in a picture. `appearance::themecheck` does that, and the
`themecheck` program runs it on a theme's folder from a terminal or a
theme repository's CI (`roadmap-detailed.md` §4.6). It answers with
errors (do not install or share it as it is), warnings (something is
ignored or adjusted) and notes. It judges with the same code the desktop
reads themes with, so a theme that passes is used as written.

**Where:** `gui/appearance/src/themecheck.rs` (the checks),
`gui/appearance/src/bin/themecheck.rs` (the program); tests in
`gui/appearance/src/themecheck_tests.rs` -- the shipped `aero` theme among
them.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The desktop's own readers decide**: `themes::parse` for the file, the icon renderer, the cursor reader, the palette for contrast, the readers' own size limits | a schema of its own -- JSON Schema, or a validator written for the repository's CI | One definition of a theme. A checker with its own idea of the format would pass themes the desktop refuses and refuse ones it reads, and nobody would notice until a user did. | The repository's CI needs this program built to run it. |
| **Errors refuse; warnings and notes inform** | pass or fail | The roadmap asks that contrast warn rather than refuse. An installer takes a theme with warnings; a repository can refuse them (`themecheck --strict`). | -- |
| **Errors:** a program or script, by its name or its first bytes; an SVG holding a script, an event attribute, a `<foreignObject>` or a reference outside itself; a link leading out of the folder or written from the root; a file past the limit the desktop reads; an icon or cursor the desktop cannot read; a screenshot outside the folder, or that is not a picture; a theme that sets nothing | warnings | "Themes are pure data -- never executable code" (`roadmap-detailed.md`). A theme gallery is a web page, and an SVG with a script in it is a script run on the gallery's visitors. A file the desktop refuses is a file its author meant to be used. | An SVG saved by a tool that leaves an `onload` in it is refused until it is cleaned. |
| **A link is allowed when it ends inside the folder**, resolved as the system resolves it (`fs::canonicalize`) | refusing every link; or judging a link by its text | Cursor themes from other desktops are made of links -- each older name of a cursor a link to its file -- so refusing links refuses every one of them. Judging by the text alone passes `icons/a.svg -> sub/a.svg` when `icons/sub` is itself a link out of the folder; the first version of this checker did, and its review found it. | Only where a link ends is checked: one may pass through a folder outside and come back in. Nothing outside is read either way. |
| **Contrast measured against every ground text is drawn on: a warning on the grounds every desktop has (the page, the toolbars), a note -- one per mode -- on those only the card style adds** | every ground a warning; or the default style's grounds alone | A theme is worn under whatever style its user picks, so the card style's surfaces are checked; but a colour the palette darkens only there, on purpose, is not something every user of the theme sees. The first version warned for each: 35 warnings on the shipped theme, mostly about the card style, which buried the ones that matter. | The shipped theme still has 19 warnings: Latte's colours of code sit just under 4.5:1 on the toolbars' ground, and the palette nudges them when drawn. |
| **Screenshots up to 4 MiB** | no limit | A theme browser fetches a whole page of them. | A limit the desktop itself does not have: nothing in it draws a screenshot yet. |
| **What a shared theme should say about itself -- author, version, licence -- is a warning**; a name, screenshots and tags are notes | errors | A theme made for oneself needs none of them; one offered to others does. | The built-in theme names no licence, the project not having chosen one, and is warned for it; its test records that. |

`meta.supports` is held to what the theme covers: an axis it lists and
does not set, or sets and does not list, is a warning; `sounds`,
`wallpapers` and `fonts` -- axes the format names that this desktop has not
built -- are a note. A file nothing reads is a warning, except a README,
LICENSE or the like at the top of the folder.

**What is not done.** Pictures of a theme drawn by a repository's CI, which
the roadmap leaves to the repository. Nothing on the desktop installs a
theme yet -- installing one is a folder appearing in the themes directory,
which the Settings application then lists -- so nothing calls the checker
before copying a theme in; whatever first does should. And the program is
not on the disk image: it is built for the machine a theme is made on or a
repository's CI runs on, and the image carries the programs under
`userspace/` (§1164); a theme author working on SlateOS itself would need it
staged.
