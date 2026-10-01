# C -> E -- the one list of programs exists; four of your programs keep lists of their own

**From:** Lane C. **To:** Lane E (`apps/**`).
**Filed:** 2026-09-27. **Status:** OPEN -- step 3 of the operator's order for
C-Q20 (`design-decisions.md` §1425); lane C's steps are done.

**In short:** the operator chose one list of the installed programs, in a
userspace library, after an inventory of every list there was. Both are done:
`gui/programs` is the library, and `gui/programs/INVENTORY.md` records what each
of the fourteen old lists held and where it went. The desktop shell reads it
now. Four of your programs still keep a list of their own -- which is the
disagreement the decision exists to end -- and step 3 is those programs reading
the library instead.

## The library

- `programs::built_in(locale) -> Vec<desktopentry::App>` -- SlateOS's own
  fifteen programs, as desktop entries (`gui/programs/applications/`), each with
  the types it opens (`app.mime_types`), its categories, keywords, icon, and
  how it is started (`app.exec`, whose `%f`/`%F` match what each program
  accepts on its command line today).
- `programs::default_for(mime) -> Option<&'static str>` -- the built-in default
  for a type, as a desktop file id (`org.slateos.Editor.desktop`). Fifty types.
  A person's own choice (`gui/associations`) stands in front of it.
- `programs::Role` -- fourteen jobs a program can be the default for (web
  browser, email, file manager, text editor, terminal, image viewer, video
  player, music player, document reader, archive manager, calculator, calendar,
  maps, system monitor): `Role::ALL`, `label()`, `definition()`, and
  `filled_by(&programs)`, which answers `None` for a role nothing installed can
  do ("No web browser is installed").
- Installed programs are the same type: `desktopentry::scan` finds them, and an
  installed entry for the same program replaces the built-in one.

## What is asked

| Program | Its own list today | Read instead |
|---|---|---|
| `apps/fileassoc` | `add_default_apps` (eight programs with the extensions each opens) and `assign_default_associations` (seven group defaults, three overrides) | the programs and their `mime_types` for what can open a type; `default_for(mime)` (through the toolkit's extension-to-type table) for the default |
| `apps/settings` | the Default apps page (`--page default-apps`) | `Role::ALL` and `Role::filled_by` |
| `apps/explorer` | its Open With list and its fallback when no association is chosen | the programs whose `mime_types` hold the file's type, and `default_for` behind `gui/associations` |
| `apps/launcher` | its own copy of the built-in programs | `programs::built_in` beside the installed entries -- which also answers the earlier note that it cannot see what is installed |

**Two of `apps/fileassoc`'s overrides were not carried as defaults**, on
purpose -- `INVENTORY.md` section 4 says why: `.bin` (its type,
`application/octet-stream`, is every unrecognised file's, so a default would open
all of them in the hex editor; the hex editor's entry lists the type, so Open
With still offers it for any file), and `.iso` (the file manager, handed a file,
opens the folder holding it rather than the image). Whether File Associations
still offers those as choices is yours.

**Three programs' entries promise only what they do today**, and would promise
more when you change them: the calendar does not open a `.ics` named on its
command line, so its entry lists no type (the Calendar role is found by its
category); neither the PDF viewer nor the e-book reader reads EPUB, so EPUB has
no default. When either changes, add the type to the program's entry in
`gui/programs/applications/` (lane C's directory -- a request, or a message,
and lane C adds it) and the default follows.

## If this is never done

The shell and your programs keep two lists that can disagree -- today they
agree, because lane C built the library from yours, but only until one of them
changes. Lane A's removal of the kernel's lists does not wait on this: nothing
outside the kernel reads them.
