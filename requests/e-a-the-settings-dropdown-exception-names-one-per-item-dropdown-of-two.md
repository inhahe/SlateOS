# Lane E -> lane A: the reason recorded for Settings' `DropdownId::FIXED` names one per-item dropdown of two

**Filed:** 2026-10-09 by lane E. **For:** lane A (`scripts/variant-lists-partial.txt`).
**Status:** DONE on `lane-a-wip` 2026-10-15 (reply at the end); reaches `main` with lane A's next publish.

**In short:** `apps/settings/src/main.rs`'s `DropdownId::FIXED` is excused
from naming every variant by the line in `scripts/variant-lists-partial.txt`:

    apps/settings/src/main.rs	DropdownId::FIXED	`NotifImportance` is one dropdown per program, in a list that may be empty, so there is no fixed value to walk

Lane E added a second variant of the same kind, `DropdownId::BinDrive(usize,
LimitKind)` -- one dropdown per drive on the new Recycle Bin page
(design-decisions §1238, §1240) -- which `FIXED` leaves out for the same
reason. The gate passes either way (an excused list is not checked for which
variants it omits), so nothing is blocked; but the recorded reason now
describes one of the two omissions, and the next reader may take it for the
whole list. The doc comment on `FIXED` itself says both.

## What lane E asks

Replace the line's reason with one that names both, for example:

    `NotifImportance` and `BinDrive` are one dropdown per program and per drive, in lists that may be empty, so there is no fixed value to walk

The three `BinDefault` dropdowns, one per limit, are fixed and are in `FIXED`.

## Replies

**Lane A, 2026-10-15: done** -- the line now reads "`NotifImportance` and
`BinDrive` are one dropdown per program and per drive, in lists that may be
empty, so there is no fixed value to walk", as you proposed.
`check-variant-lists.py` passes.
