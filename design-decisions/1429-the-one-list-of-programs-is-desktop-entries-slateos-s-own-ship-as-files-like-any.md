## 1429. The one list of programs is desktop entries; SlateOS's own ship as files like any other program's

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous), inside the operator's §1425 &middot; **Lane:** C

**In short:** the operator chose one list of programs, in a userspace library
(§1425). That library, `gui/programs`, keeps SlateOS's own programs in the
same format the start menu already reads for programs installed on the machine:
one small text file per program, the freedesktop "desktop entry". So there is
one kind of program record, not two, and the same files can later be installed
on the image unchanged. What opens what by default is kept the same way, in the
freedesktop defaults file.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Desktop entries (chosen)** | one format for built-in and installed programs, read by the reader the start menu already uses (`gui/desktopentry`); an installed entry with the same id replaces the built-in one with no merge code; the files install as `/usr/share/applications/*.desktop` as they are; every program ported to SlateOS brings its own | text parsed at start-up (fifteen small files, microseconds); a typo in a file is found by a test rather than the compiler -- `tests/inventory.rs` parses and validates each |
| A Rust table of structs | typos are compile errors | a second model of "a program" beside the desktop entry, with a conversion between them to keep in step -- the exact shape of the four disagreeing lists this replaces; and nothing to install on the image |
| Keep the kernel's registry, read it over a system call | already exists | the operator chose userspace (C-Q20, B); the registry named nine paths no crate builds |

**What is not in it:** a person's own choices of what opens what. Those stay
in `gui/associations` (`fileassoc.yaml`, written by the File Associations
program); the library's defaults are what stands behind them.

**As built:** `gui/programs` (2026-09-27): fifteen entries, fifty type defaults,
fourteen roles, and `INVENTORY.md` with the fourteen old sources item by item.
