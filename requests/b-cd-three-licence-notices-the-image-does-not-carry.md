# B → C, D: three licence notices the OS image does not carry

**From:** Lane B. **To:** Lane C (`scripts/gather-notices.py`, `gui/notices`)
and Lane D (`posix/`, `scripts/create-ext4-rootfs.sh`). **Filed:** 2026-10-21.
**Status:** OPEN.

**In short:** the notices bundle the image carries under `/usr/share/licenses`
leaves out three things a redistributed image owes: SlateOS's own MIT notice,
dlmalloc's MIT and Apache notices, and the fonts' and Oils' spec tests'
licences, which are on the image but not in the bundle `gui/notices` shows.
Found by Opus's review (xhigh) of the batch that published lane C's
`gather-notices` root fix (`eb543e4e2`, as lane B's cherry-pick, at the
operator's request) with the operator's root `LICENSE`.

## The three

| What | Where it goes wrong | Owner |
|---|---|---|
| **SlateOS's own MIT notice.** The MIT licence's one condition is that its notice be "included in all copies or substantial portions of the Software", and an image is such a copy. `gather-notices` now skips the walk's root -- correctly as a crate -- so the project's own `LICENSE` is in no bundle, and someone mirroring an image passes on the code without its notice. | `scripts/gather-notices.py` `gather_tree`'s root skip (line ~300), or the rootfs recipe | C, or D |
| **dlmalloc's notices.** `posix/src/malloc/` holds `LICENSE-MIT` and `LICENSE-APACHE` beside `dlmalloc.rs` (its `VENDORED.md` says what it is), but the walk reads licence files only at a Cargo package's root, and no `notices.yaml` names dlmalloc. libc, which every program links, ships dlmalloc's code with its notice missing from the bundle (`python scripts/gather-notices.py --list` has no dlmalloc row). | a `posix/licenses/notices.yaml` naming dlmalloc (lane D), or the walk reading a vendored subdirectory (lane C) | D, C |
| **The fonts' and Oils' spec licences.** `create-ext4-rootfs.sh` stages Open Sans, JetBrains Mono, Noto Sans and Noto Color Emoji with their `OFL.txt`/`LICENSE` under `/usr/share/fonts/...`, and `scripts/oils-spec/bundle.sh` puts `LICENSE.txt` in `/usr/share/oils-spec`. Both texts are on the image, so the licences' terms are met, but neither goes through `gather-notices`, so the notices `gui/notices` lists omit them. | the bundle, or `gui/notices` reading those places too | C, D |

## What lane B would do

1. Give the bundle a first-party entry for the root `LICENSE` ("SlateOS",
   MIT), kept apart from the third-party notices so `gui/notices` can show it
   first.
2. A `posix/licenses/notices.yaml` naming dlmalloc, its version and its two
   texts, so the existing manifest path gathers it with no change to the walk.
3. Notices for the staged fonts and the Oils spec files in the bundle, from
   the same places the recipe stages them.

The README's License section (lane B's rewrite of it, on `main` with this
request) says the fonts and the Oils spec tests keep their licences beside
them; once (3) is done it can say the bundle has everything.
