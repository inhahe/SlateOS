## TD-C-TEN-FILES-SWITCH-OFF-THE-DEAD-CODE-WARNING-FOR-THEMSELVES

**Date:** 2026-09-16. **Lane:** C.
**Where:** a blanket `#![allow(dead_code)]` at the top of, among others:
`apps/explorer/src/{columns,fileops,thumbs}.rs`, `apps/imageviewer/src/video.rs`,
`apps/procexplorer/src/features.rs`, `apps/settings/src/remote.rs`,
`apps/match3`, `apps/pinball`, `apps/screenrecorder`, `apps/soundrecorder`.

**Status (lane E, 2026-09-28):** one remains, `apps/settings/src/remote.rs`,
and it waits on something other than a reading: it is the unreachable
remote page C-Q17 says to wire up (§1423), whose dynamic-DNS half needs the
userspace service in `requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`
and whose remote-desktop half has no server to configure (`todo.txt`,
"C-Q17: what is left"). The rest carry none: the explorer's `columns.rs` and
`fileops.rs` (the last on this date:
`TD-C-THE-FILE-OPERATIONS-MODULE-ADVERTISES-POLICIES-NOTHING-SELECTS`),
`procexplorer`'s `features.rs`, `match3`, `pinball`, and on this date the
screen recorder (an unused layout constant) and the sound recorder (the
noise gate's attack, documented and never read -- it opened and shut in one
sample, a click at each edge; it ramps now). `thumbs.rs` is `gui/thumbs`
now, and `imageviewer/src/video.rs` is gone.

**In short:** ten files tell the compiler not to mention code nothing uses.
Each suppression covers the whole file — thousands of lines in several cases —
so anything inside that loses its last caller goes quiet permanently. Three of
these files were measured on this date by deleting the line and building, and
all three were hiding something real.

**Measured:**

| file | what the allow was hiding |
|---|---|
| `fileops.rs` | 10 findings, including three `ConflictPolicy` and two `ErrorPolicy` variants **nothing can select**, while the module doc advertises "conflict resolution policies" and "per-file error handling (skip, retry, stop)" |
| `columns.rs` | a whole unreachable column-chooser renderer with its own palette entries and constants, plus a duplicate row renderer whose test existed to keep the two copies agreeing |
| `thumbs.rs` | two methods, both **legitimate** — documented deliberate keeps, now carrying a targeted allow instead |

**The method, which is cheap:** delete the line, `cargo check -p <crate>`, read
the warnings, restore or narrow. Fifteen minutes per file, and two of the three
turned up things worth acting on.

**Two cautions, learned from doing it.**

Not every finding is a defect. `thumbs.rs`'s `is_valid` exists because
`pixels`, `width` and `height` are public and a `Thumbnail` can be built
outside the module; its doc said so before I looked. **A targeted
`#[allow(dead_code, reason = "...")]` is the right home for those** — same
runtime cost as the blanket line, and it says which items are deliberate.

Three of these files are the unreachable features in `open-questions.md`
**C-Q17**, which is the operator's to answer. Measuring them is useful; acting
on them is not, until it is answered.

**Why this matters beyond tidiness.** Six unused capabilities were found by
hand today, and the compiler found two instantly the moment they appeared in a
binary crate with no suppression. The lint works. These ten files are where it
has been switched off, and `fileops.rs` shows what accumulates behind one: an
advertised feature list with two entries nothing can choose.
