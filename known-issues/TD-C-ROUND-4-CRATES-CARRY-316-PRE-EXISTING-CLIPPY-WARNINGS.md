### TD-C-ROUND-4-CRATES-CARRY-316-PRE-EXISTING-CLIPPY-WARNINGS (lane C, 2026-08-21)

**In short:** `CLAUDE.md` says a crate must be clippy-clean. Six of the
applications touched in round 4 are not, and were not before round 4 either —
between them they emit 316 warnings, none of which the migration introduced.
Recording the count so that the next person to open one of these files knows
the mess predates them and roughly how big it is.

**Where and how many** (`cargo clippy --target x86_64-pc-windows-gnu`, all
targets, 2026-08-21):

| Crate | Warnings |
|---|---|
| `apps/systemrestore` | 129 |
| `apps/archivemanager` | 77 |
| `apps/taskscheduler` | 56 |
| `apps/explorer` | 34 |
| `apps/rssreader` | 11 |
| `apps/screenrecorder` | 9 |

Overwhelmingly `clippy::arithmetic_side_effects` and
`clippy::indexing_slicing` — the two defensive lints `CLAUDE.md` asks for by
name. Both are the same shape of finding: a subtraction or an index that is
fine for the values the program actually produces, and that nothing stops from
receiving a value it is not fine for.

**Verified not introduced by round 4:** the warning sites in the two files the
migration edited most (`apps/explorer/src/columns.rs` at 462, 463, 1201, 1423,
1455, 1475; `apps/screenrecorder/src/main.rs` at 161, 167, 172, 3780, 3814,
4070, 4121, 4122, 4139) are all far from the edit sites (columns.rs ~1506,
screenrecorder ~709–790), and the migrated crates build and test with zero
warnings.

**The proper fix:** one crate at a time, replacing bare arithmetic with
`checked_*`/`saturating_*` and bare indexing with `.get()`, and suppressing
individually only where the invariant is real and can be written down. Not
folded into round 4 because it is a different kind of work — round 4 removes
duplicate answers, this removes unproven assumptions — and mixing them would
make both diffs unreviewable.

**If never fixed:** the lints stay noisy enough that a genuinely new warning
in one of these crates is invisible, which is the failure mode that matters
more than any individual warning.
