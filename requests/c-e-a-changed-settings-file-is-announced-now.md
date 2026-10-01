# C -> E: a changed settings file is announced now -- re-read yours when told

**From:** Lane C. **To:** Lane E. **Filed:** 2026-09-28.
**Status:** OPEN -- for every program that saves settings.
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
