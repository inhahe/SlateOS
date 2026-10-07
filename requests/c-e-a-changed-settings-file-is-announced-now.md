# C -> E: a changed settings file is announced now -- re-read yours when told

**From:** Lane C. **To:** Lane E. **Filed:** 2026-09-28.
**Status:** DONE 2026-09-28 (lane E) -- every program named below reads its
file again when told: `lockscreen` 57e9b5abd, `markdowneditor` 4b4fda7b1,
`passwordgen` 2e0755f64, `habits` 43e11c674, `regextester` 7898339d0,
`weather` cb9bbf09b, `filesearch` 0c65c8116, `ebook` 1e1845ff7, Settings
e095a5ecb (all seven files it edits), `explorer` (the commit after this
line). Each has a test with two windows and mutation rows.

What it turned up, beyond the re-read:
- **A re-read must not be saved.** Settings wraps every event in a
  snapshot and saves what changed; an announcement taken through that path
  would have written the file straight back over whoever wrote it. It is
  answered ahead of the snapshot. `explorer` answers it ahead of its
  dialogs for the same reason: a dialog owns the input, and an
  announcement is not input.
- **Keeping one's place.** A re-read that reorders a list keeps the
  selection on its item (`habits`, `regextester`, explorer's hand
  arrangement), and a list that shrinks keeps its scroll in reach
  (`filesearch`).
- **A window's own view is not the file's.** Explorer re-applies a folder's
  columns only when the file's entry for that folder changed, so columns
  shown and not saved outlast another window's choice of, say, a thumbnail
  size.
- **Data files are not settings files.** Notes, contacts, the e-book
  library and about fourteen more keep data written whole from each
  window's copy, which nothing announces; two windows of one of them lose
  each other's changes. `known-issues.md` "[E] Two windows of one program",
  and the operator's choice of fix is E-Q5.

Original status: OPEN -- for every program that saves settings.
**Decision behind it:** `design-decisions.md` §1418 (the operator's C-Q26
answer), §1434.

**In short:** When a settings file changes, every open window is told now. That
covers a program saving its own file, another copy of it saving, or a person
editing the file by hand. The desktop shell watches the settings folder and
the compositor relays the news. A program only sees the change if it reads its
file again when told, and today none does. Each program that keeps a settings
file needs a few lines.

## What a program does

Every window receives `Event::SettingsChanged { group }`. The group names the
file, and your program holds its own name already (the `CONFIG_NAME` constants):

```rust
Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {
    self.settings = Settings::load();   // settingsfile::load(CONFIG_NAME), as at startup
    // redraw with them
}
```

`SettingsGroup::file_name()` answers `appearance`, `input`, `notifications`
and `session` for the four desktop files, and the name for anyone else's, so
the same comparison works for any program.

- **Your own save is announced back to you.** Re-reading it is harmless,
  since what you read is what you wrote. Nothing needs to tell your own save
  apart.
- **A deleted file is announced too.** `load` then gives the empty document,
  which your settings reader already turns into defaults.
- **Save through `settingsfile::store`**, as you do. It writes a copy and
  renames it into place, which the watch announces at once. A file written in
  place is announced once it has been quiet for a quarter of a second.
- `settingsfile` now refuses a name that could not be announced. The rule is
  1 to 32 bytes of `a`-`z`, `0`-`9`, `_` and `-`, the same as
  `SettingsName`'s. Every `CONFIG_NAME` in the tree passes, so nothing changes
  for you. A name with a dot or a path in it used to be mangled silently.

## Which programs

Every program with a `CONFIG_NAME` or its own `settingsfile::load`:
`ebook`, `explorer`, `filesearch`, `habits`, `lockscreen`, `markdowneditor`,
`passwordgen`, `regextester`, `weather`, and Settings for the files it edits
on the shell's behalf. The four §1418 named first are the lock screen's clock
and date, the markdown editor's autosave, the password generator's rules and
the explorer's copy-onto-an-existing-name choice.
