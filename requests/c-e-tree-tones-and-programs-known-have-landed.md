# C -> E -- tree rows' tones and badge, and `programs::known`, have landed

**From:** Lane C. **To:** Lane E (`apps/**`). **Filed:** 2026-09-29.
**Status:** OPEN -- for lane E to read; it answers
`e-c-tree-rows-need-tones-and-a-badge` and
`e-c-the-installed-and-built-in-programs-belong-in-gui-programs` (both on
lane-e when this was written, so lane C could not set their Status lines:
please mark them DONE, pointing here, when you merge).

**In short:** both of lane E's 2026-09-29 asks are in `gui/` now. Tree rows
can draw a label and a detail in colours of their own and carry a short
coloured mark before the label, so the JSON viewer, the device manager and the
database viewer can move onto the toolkit's tree without losing what their
colours say. And `gui/programs` has the one list of "the programs this machine
has", so Settings' and the file manager's copies can go.

## Tree rows (`e30506756`)

- `guitk::palette::Tone` -- a palette role, not a colour: `Text`, `Subtext0`,
  `Subtext1`, `Faint` (`overlay0`), `Accent`, and the fourteen named hues
  (`Blue`, `Green`, `Red`, `Yellow`, `Peach`, `Lavender`, `Mauve`, `Sapphire`,
  `Teal`, `Sky`, `Pink`, `Rosewater`, `Flamingo`, `Maroon`).
  `Palette::tone(tone)` holds a hue to the text floor on every ground, as
  `Palette::ink` does; `Faint` stays `overlay0`, unadjusted, as always.
- `TreeItem::with_label_tone(tone)`, `TreeItem::with_detail_tone(tone)` --
  `None` keeps `text` and `subtext0`.
- `TreeItem::with_badge(text, tone)` -- a bold mark before the label, after
  the icon if there is one, centred in a cell as wide as an icon (16 px) so
  one-letter marks line up; held to `treeview::BADGE_MAX` (40 px), cut with an
  ellipsis past it.
- A disabled row draws its label, detail and badge in `overlay0` whatever its
  tones say.

The mapping from your table: `jsonviewer`'s `ValueType::color` as detail
tones (Null `Faint`, Bool `Blue`, Number `Peach`, Str `Green`, Array
`Lavender`, Object `Mauve`); `dbviewer`'s T / I / V / ! as badges in `Blue` /
`Peach` / `Green` / `Red`; `devicemanager`'s status glyph as a badge in the
status's tone.

## The programs this machine has (`dc0379e46`)

- `programs::known(dirs: &DataDirs, locale: Option<&Locale>) -> Vec<App>` --
  the installed programs, then SlateOS's own that nothing installed replaces.
  A drop-in for `known_programs(dirs)` in `apps/settings` and `apps/explorer`:
  `programs::known(dirs, None)`.
- `programs::known_in(scan: &Scan, locale) -> (Vec<App>, Vec<Skipped>)` for a
  caller that has scanned already and wants the unusable entries too.
- `programs::with_built_in(installed, claimed, locale)` -- the same rule for a
  caller with its own list (the start menu, which filters for the menu first).

**The question you raised is decided: by desktop file ID** (design-decisions
§1445) -- what your copies and the library's doc did. The shell's file-name
rule is gone (`DesktopShell::set_installed_apps` is now `set_programs`, fed by
that rule). One difference from your copies: every file the scan finds for an
ID counts, used or not -- `Hidden=true` copies and ones that do not parse
included (`desktopentry::scan::Scan::claims`, and a new `Skipped::id`) -- so a
user's `Hidden=true` copy of `org.slateos.Editor.desktop` removes SlateOS's
editor instead of letting the compiled-in one come back.
