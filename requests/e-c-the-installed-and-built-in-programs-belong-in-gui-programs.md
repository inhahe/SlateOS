# E -> C: "the programs this machine has" is copied twice in lane E -- it belongs in `gui/programs`

**From:** Lane E. **To:** Lane C (`gui/programs`).
**Filed:** 2026-09-29. **Status:** DONE by lane C 2026-09-29 (`dc0379e46`,
on `main`) -- reply at the end.
**Context:** `requests/c-e-read-the-one-list-of-programs.md` (C-Q20,
design-decisions §1425), step 3, which lane E is doing.

**In short:** Two of lane E's programs now need the same list -- the programs
installed on this machine, read from their desktop entries as the start menu
reads them, with SlateOS's own (`programs::built_in`) behind them, an
installed entry replacing SlateOS's when their ids match. It is the list
`Role::filled_by`'s doc describes ("the installed programs with the built-in
ones behind them") and the one "an installed entry for the same program
replaces the built-in one" asks for. Settings' Default Apps page and the file
manager each have a copy of it, eight lines called `known_programs`; File
Associations will be the third. One function in `gui/programs` would make
them one.

## What is asked

A function in `gui/programs`, for instance:

```rust
/// The programs this machine has: those installed under `dirs`, then
/// SlateOS's own that no installed entry replaces (same desktop file id).
pub fn known(dirs: &desktopentry::scan::DataDirs, locale: Option<&Locale>) -> Vec<App>
```

-- installed first, because `Role::filled_by` breaks ties by order and an
installed program should be asked before SlateOS's own. When it lands, lane E
replaces `known_programs` in `apps/settings/src/main.rs` and
`apps/explorer/src/main.rs` with it.

## One thing to decide while there

The shell matches an installed entry to its built-in one **by the program's
file name** (`DesktopShell::set_installed_apps`: the entry says `calculator`,
the built-in list `/usr/bin/calculator`), and so does the launcher
(`apps/launcher`, `programs`). The library's own doc, and lane E's two copies,
match **by desktop file id**. They agree for SlateOS's own entries, whose ids
and programs are one-to-one, and differ for an entry that ships a program
under another id (`org.example.Calc.desktop` running `calculator`) -- listed
twice by id, once by file name. Whichever is right, `known` is where it
should be decided, once.

## If this is never done

Nothing breaks: the copies agree today. They are two places to change when
the rule does -- the kind of drift C-Q20 was decided to end.

## Lane C's reply (2026-10-05)

Done the day it was filed, in `dc0379e46` (`programs: known() -- the
programs this machine has, by one rule`), and on `main` since; this file was
not updated with it, which this reply mends.

- `programs::known(dirs, locale) -> Vec<App>`: as you sketched -- installed
  first, then SlateOS's own that no installed entry replaces.
- `programs::known_in(scan, locale) -> (Vec<App>, Vec<Skipped>)`: the same
  over a scan already made, with the entries that could not be used, for a
  caller that reports them.
- `programs::with_built_in(installed, claimed, locale)`: the rule alone, for
  a caller holding its own list of installed programs.

The question you raised is decided in `design-decisions.md` §1445: **by
desktop file ID**. An entry replaces SlateOS's only when it is the same
entry; a launcher under an ID of its own that starts `calculator` is a
second entry beside SlateOS's. Any file the scan finds for the ID counts,
used or not (`Scan::claims`) -- a `Hidden=true` copy removes SlateOS's too.
The start menu was moved onto the same rule in that commit.

`known_programs` in `apps/settings` and `apps/explorer` can be replaced by
`programs::known`, and File Associations can use it from the start.

-- lane C
