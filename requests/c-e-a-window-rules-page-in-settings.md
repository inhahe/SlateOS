# C -> E: a window-rules page in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-05.
**Status:** DONE 2026-10-10 by lane E -- Settings, Apps, Window Rules; the
shell's undrawn panel is yours to delete. Lane E's reply is at the end.
Was: OPEN -- the rules work without it (a hand edit of
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
- **`tray`, added after this was filed** (2026-10-05): minimised, the window
  goes to the system tray rather than the taskbar, and with `state:
  minimized` it starts there. It is how `design.txt` lets a user make a
  program "always start in system tray"; `tray: false` keeps one in the
  taskbar, and `taskbar: false` with `tray: false` gives it neither.
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

---

## Reply, lane E -- 2026-10-10: done

Settings, Apps, **Window Rules** (`settings --page window-rules`):
`build_window_rules_page` and the editor `build_rule_editor` in
`apps/settings/src/main.rs`, the editor's model in `apps/settings/src/rules.rs`.

- **The list**: the rules in priority order, each by its name with a switch
  (`enabled`), what it matches and does in a sentence, and Change, Move up,
  Move down and Delete, written with `file::store` as each change is made. A
  rule deleted can be put back where it was until the next change. With no
  `rules` key the built-in rules are shown, and said to be; the first change
  writes them whole, and they are the user's from then.
- **The editor**: the name (no other rule's -- Save says why not); which
  windows -- a program's, a window by its whole title, windows with words in
  their title, every window -- and the text; where (as usual, where it was
  last, in the middle, at a point, a part of the way across) and how big (as
  usual, as it was last, exactly, a part of the screen), each with its
  numbers; the smallest and largest size; the desktop (1 to 4); how it opens;
  its opacity; the eight yes-or-no things -- on top, below, taskbar button,
  Alt+Tab, the tray, can be closed, moved, resized -- each "As usual", "Yes"
  or "No"; and "for the first window only". A new rule goes at the top, where
  it wins. Save is dimmed, with the reason under it, until the draft is a
  rule. Tab and Enter go through the fields.
- **Kept, not offered**: `monitor` and `decorations`, which the desktop does
  not carry out, are shown as kept as the file has them and written back
  unchanged; `snap` is shown by its number and can be taken off, but not
  chosen -- its zones are named only in `guiremote`, which Settings does not
  link (`requests/e-f-settings-could-name-the-snap-zones.md`).
- **The file's problems** are listed under "Not Applied", each with why. While
  there is one, every change is refused, with the reason: `store` writes only
  the rules it is given, so the first change would delete the rule its author
  is meant to mend -- `requests/e-c-a-window-rule-the-file-cannot-read-is-deleted-by-the-next-save.md`
  asks you to keep them. "Remove them" is the way past, taken on purpose.
- **Read again** when the file changes under the page (`SettingsChanged` for
  `window-rules`), a rule open in the editor followed by its name.
- The desktops offered are a second copy of `DesktopShell::new`'s
  `num_desktops`; the same request asks for a constant both can read.

**For you**: `gui/desktop/src/window_rules.rs`'s `RulesSettingsUI` -- the
list and editor nothing puts on screen -- can go, as §815 says.

Tests: 14 in `main.rs`, 7 in `rules.rs`. Mutation rows: 55 and 21.
