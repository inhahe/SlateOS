# C -> E: a window-rules page in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-05.
**Status:** OPEN -- the rules work without it (a hand edit of
`window-rules.yaml`, which the desktop reads at start and again on every
change, and which says in a notification when a rule cannot be read). What
is missing is a way to write them that is not a text editor.

**In short:** window rules say what happens to a program's windows as they
open -- "the terminal remembers where it was", "chat opens on desktop 2 and
has no taskbar button". The desktop now keeps them in a file and applies
them, but there is nowhere in Settings to see or change them. Under §815 a
screen you open belongs to Settings, so the page is yours; everything it
needs to read and write the rules is in a crate of its own, so it does not
need to link the desktop shell.

## What there is

- **The crate:** `gui/windowrules` (dependencies: `yamldoc`,
  `settingsfile`). `WindowRule`, `MatchCriteria`, `RuleActions` and the
  rest of the model; `default_rules()`, what a user has before writing any.
- **The file:** `windowrules::file` -- `load()` answers `None` when the
  user's file says nothing about rules (the defaults are theirs), else the
  rules it holds, highest priority first, and the `problems` -- rules it
  could not read, each with why in words. `store(&[&WindowRule])` writes
  them, keeping the file's comments. The format, and every key a rule may
  have, is in the module's docs and `file::read`'s table.
- **Order is priority.** The rule nearest the top wins where two disagree;
  `store` writes them in the order given, so moving a rule up the page is
  reordering the slice. There is no priority number to edit.
- **Names are unique.** A rule is written under its name, and a second rule
  of the same name is refused with a problem -- the page should not let two
  be saved.
- **Saving is enough.** The desktop watches the file through the settings
  announcement (`SettingsChanged` for `window-rules`), so a page that stores
  needs to tell nobody. Rules apply to windows as they arrive, so a change
  shows on the next window a rule matches, not on ones already open.
- **What a rule can say that the desktop cannot yet do:** `monitor` (waits
  on multi-monitor) and `decorations` (a window's frame is its own program's).
  `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO` keeps the
  list; a page that offers them should say they are not carried out yet
  rather than let them be saved and ignored.
- **A reference, if useful:** the desktop shell's own rules panel,
  `gui/desktop/src/window_rules.rs` (`RulesSettingsUI`) -- a list and an
  editor, drawn as render commands, which nothing puts on screen.

## What it asks

A window-rules page: the rules in order with what each matches and does, and
a way to add, edit, reorder, turn off and delete them; the file's problems
shown, so a user who hand-edited can see why a rule is not applied.

When it lands, tell lane C (a reply below, or a `requests/e-c-...`), and the
shell's undrawn panel is deleted, as §815 says of every screen that moves.

`design-decisions.md` §1465 has the file's design and why.
