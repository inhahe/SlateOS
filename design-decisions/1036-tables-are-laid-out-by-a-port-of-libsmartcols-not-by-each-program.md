## 1036. Tables are laid out by a port of libsmartcols, not by each program

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** about twenty util-linux programs print tables -- `lsblk`,
`findmnt`, `lsmem`, `lscpu`, `lslocks`, `swapon --show` among them -- and none
of them decides its own column widths. They hand rows to a library,
libsmartcols, which works out how wide each column is, what gets cut or
wrapped on a narrow terminal, how a tree is drawn, and what `--raw`,
`--pairs` and `--json` look like. Our versions of these programs each laid
out their tables themselves, so each got the widths, the cutting and the
JSON a little differently from upstream and from one another. The library is
now ported once, as the crate `userspace/smartcols`, and `lsmem` is the first
program printed through it: on WSL's own memory, at nineteen terminal widths
from 1 to 250 columns, its output matches util-linux 2.39.3 byte for byte.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Port libsmartcols as a crate, function by function (chosen)** | every table program prints what util-linux prints, at every terminal width | one copy of the width arithmetic (averages, deviations, the seven reduction stages), which is what makes widths match and is easy to get subtly wrong; each program port shrinks to its own logic | ~2,800 lines before its first user; the parts no program needs yet (groups, sorting, colours, custom wrapping) are left out and must be added when one does |
| Lay out each table in the program, as before | nothing, until a table is wide or a terminal narrow | no new crate | twenty private layouts; a narrow terminal cut each differently, and `--json` was each program's own dialect |
| A Rust table crate (`comfy-table`, `tabled`) | tables look like that crate's | small and maintained | its layout rules are its own, so no output could match upstream at all |

How it is shaped: upstream links lines, columns and cells through
reference-counted pointers and intrusive lists; here a `Table` owns vectors
of them in upstream's list order. A `LineId` is a line's place, which stays
valid because nothing is removed; a `ColumnId` is a column's identity, which
`move_column` (added for `column --table-order`) does not change, and each
line's cells are indexed by the column's `seqnum`, as upstream's are. Output goes into a byte buffer
the program writes itself, so a failed write is the program's to report, as
util-linux's `close_stdout` reports it. Text is measured with glibc's rules
for the locale in force -- `mbrtowc`, `iswprint` and `wcwidth` through
`quoting` and `charwidth` -- so a C locale sees `\xNN` escapes where a
UTF-8 one sees characters, as upstream does.

**Where:** `userspace/smartcols` (the port); `userspace/lsmem` (its first
user); `scripts/lsmem-diff.sh` (346 cases against util-linux 2.39.3).

**Revisit** if a program needs what was left out; its module docs list it.
The rest of the table printers -- `lscpu`, `lsblk`, `findmnt` among them --
should move onto it as each is ported (known-issues
TD-B-TABLE-PROGRAMS-LAY-OUT-THEIR-OWN-TABLES).
