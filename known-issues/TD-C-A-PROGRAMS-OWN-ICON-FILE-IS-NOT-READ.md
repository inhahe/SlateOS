## TD-C-A-PROGRAMS-OWN-ICON-FILE-IS-NOT-READ

**Date:** 2026-09-26. **Lane:** C. **FIXED 2026-09-26**, the same day:
`IconTheme::render` draws an icon file named by its path (SVG, or PNG scaled
by area to the size asked), and a name no theme draws from `hicolor` --
scalable first, then the best-sized PNG, in each standard context -- and then
`pixmaps`; the exact name in the built-in set and `hicolor` before any shorter
one. Kept below as it was written.

**In short:** a program's desktop entry may name its picture as a file
(`Icon=/opt/app/icon.png`) or ship it into the standard `hicolor` icon
directory; the start menu draws neither, only names its own icon theme draws,
and falls back to the generic program picture. Nothing is broken -- every
program still has a picture -- but a program's own is not shown.

**Where.** `appearance::icons::IconTheme::source` looks in the chosen theme's
`icons` folders and the built-in set, by name. `desktop::launcher::AppEntry::icon`
carries whatever the entry says; `DesktopShell::program_icon` asks for it with
`GENERIC_PROGRAM_ICON` as the fallback.

**The proper fix.** Two lookups, in the order the Icon Theme Specification
gives: (1) an absolute `Icon` path -- read the file, SVG through the existing
renderer and PNG through `imagecodec`, scaled to the size asked; (2) a name not
in the chosen theme -- look in `$XDG_DATA_DIRS/icons/hicolor/<size>/apps/` and
`scalable/apps/`, the fallback theme every program installs into. Both belong
in `appearance::icons` so every program that draws icons gets them, and the
request type already carries owned names for exactly this.

**Why it has not bitten.** No program ships an entry yet
(`requests/c-e-ship-a-desktop-entry-with-each-program.md`).
