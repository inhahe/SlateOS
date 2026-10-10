## 1066. The terminfo library carries `xterm-256color` built in, as an ncurses fallback

**Date:** 2026-10-08
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** programs ask the terminfo database (a directory of files, one
per terminal type, saying what each terminal can do) about the terminal they
write to. SlateOS's own terminal says it is `xterm-256color`, but the disk
image has no terminfo database yet, so the answer was "no such terminal".
Most programs then just print plainly, but `pstree` on a terminal in a
non-UTF-8 locale asks the question ncurses' `setupterm` way and *exits* when
it is refused -- as it does on Linux without a database. This decides that
our port of the terminfo library (`userspace/terminfo`) carries the one
entry for `xterm-256color` inside itself, used only when the database has no
entry for the name, so SlateOS always knows its own terminal.

### What it is

ncurses has this mechanism already: built with `--with-fallbacks=NAMES`, it
compiles those entries into the library, and `setupterm` consults them
(`_nc_fallback2`) after the database has failed -- not found, or no database
at all -- and never before. Ubuntu's build has none. Ours has one:
`xterm-256color`, the reference's own compiled file (ncurses-base
6.4+20240113, 4071 bytes), turned into a byte table by
`scripts/gen-terminfo-fallback.py`.

| | on Ubuntu | here, before | here, now |
|---|---|---|---|
| `TERM=xterm-256color`, database present | the file | the file | the file |
| `TERM=xterm-256color`, no database | `terminals database is inaccessible` | the same | the built-in entry |
| `TERM=linux`, no database | inaccessible | inaccessible | inaccessible |

So on SlateOS's terminal: `pstree` in the C locale works instead of exiting;
`cal`, `hexdump -L` and the other `ulcolors` users colour, as on Linux.

### Why

* **It is upstream's mechanism, not an invention.** A system that ships no
  database is exactly what `--with-fallbacks` is for; a SlateOS build of
  ncurses would reasonably be configured with its own terminal's name.
  Nothing is decided differently from upstream once an entry is in hand.
* **The terminal type is the OS's own.** `apps/termchild` sets
  `TERM=xterm-256color` unconditionally; that the library should be able to
  describe the one terminal the OS itself provides is a property of the OS,
  not of whichever files the image happens to stage.
* **It cannot shadow the database.** A file found on disk always wins, so
  once the image has a database -- lane D's
  `requests/b-d-the-image-has-no-terminfo-so-no-program-can-tell-a-colour-terminal.md`
  -- the built-in entry is never reached for this name, and the harnesses,
  which run where the database exists, see no difference at all.

### Against

* It is a difference from the reference's build: on a Linux without a
  database, Ubuntu's `pstree` exits where ours carries on. That case cannot
  arise on the reference -- ncurses-base is `Priority: required` -- and is
  the one SlateOS is in.
* The entry is a snapshot. If the database on the image is later updated
  and this table is not, the two can disagree; but the database wins, so the
  snapshot only ever speaks where there is nothing else.
* Only one terminal. A session from elsewhere (`ssh` from a Linux console,
  `TERM=linux`; `screen`; `tmux`) still needs the database. More fallbacks
  would be easy to add to the script's list, but each is a guess about what
  SlateOS will meet; the database is the real answer for those, and is
  lane D's to stage.

### Revisit when

The image stages a terminfo database: the fallback then never answers for
`xterm-256color` and could be dropped -- or kept, as insurance, at the cost
of 4 KiB.
