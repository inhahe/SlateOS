## 861. The photograph on screen has one image id; the grid will need a pool

**In short:** the compositor stores pictures under numbers the application
picks. Photo manager's single-photo view shows exactly one photograph at a
time, so it always uses the number 1 and overwrites it whenever the selection
moves. The alternative -- a number per photograph -- reads better until you
ask who hands the numbers back when a photograph scrolls out of sight. Nobody
in this application is watching for that yet. The thumbnail grid will have to
be, and that is where the machinery belongs.

**Date:** 2026-09-17. **Lane:** C. **Decided by:** Claude (autonomous).

**The two ways to do it.**

| | *What changes* |
|---|---|
| **A. One id per photograph** | Returning to a photograph shows it without decoding it again. Every id must be released when its photograph leaves the screen, or the compositor keeps every picture the user has clicked -- a 24-megapixel photograph is about 96 MB in this form, so an afternoon's browsing is measured in gigabytes. |
| **B. One fixed id** (chosen) | Selecting the same photograph twice decodes it twice. Nothing can accumulate: there is only ever one picture, and the next upload replaces it. |

**Why B.** The cost of A is not the memory, it is the lifecycle. A pool of ids
needs a rule for when a photograph is no longer visible, an owner for that
rule, and a test that the rule fires -- and the failure it prevents is
invisible until a long session runs out of memory, which is the worst possible
time to discover the rule was never wired. B cannot fail that way by
construction.

**B is not free, and the figure is known.** Decoding is not cheap: a
21-megapixel JPEG measured 669 ms in release (and 7.6 s in debug, which is
why that measurement is stated in release). Flipping between two photographs
therefore re-decodes each time, where A would not. That is a real cost and it
is the reason this is a decision rather than an obvious choice -- it is
accepted because the single-photo view is driven by a human moving a
selection, so the decode happens at the speed of clicks, not of frames.

**What makes B safe rather than merely simple** is that the upload's owner is
recorded beside it. `ShownPicture` carries the `PhotoId` it was decoded from,
and the view draws the picture only when that id matches the current
selection. Without it, the upload -- which outlives a change of selection by
the one frame it takes to replace it -- would be drawn for a single frame
under the *new* photograph's name: the wrong photograph on screen, briefly,
which is worse than a placeholder.

**When this gets revisited.** When the grid decodes thumbnails. At that point
a pool with release exists because the grid cannot work without one, and the
single-photo view should draw from it rather than keep a private number. The
sequencing is deliberate: build the lifecycle where it is forced, not where it
can be avoided.

**FOLLOW-UP, same day: the grid needed no pool, and the reasoning above was
half wrong.** The thumbnails landed within hours, on `gui/thumbs` (extracted
from `apps/explorer`, which had solved this long before I asked the question).
Ids there are **derived, not allocated**: `thumbs::image_id` hashes the file's
path, modification time and size. There is no free list, no allocator and
nothing to hand a number back to.

So of the two halves of "the lifecycle":

| Half | Predicted | Actual |
|---|---|---|
| **Allocation** | a pool to draw ids from | none exists; an id is a hash of the file's identity |
| **Release** | a rule for when a photograph is off screen | real, and the cache owns it -- eviction reports the ids the compositor must be told to drop |

The release half was genuine and is exactly as load-bearing as claimed; what
made it tractable is that an LRU already knows when something has left, so
nobody had to invent "no longer visible". The allocation half I worried about
does not exist as a problem.

**The decision stands; the reasoning that produced it does not.** One fixed id
for the single-photo view is still right -- that view holds a full decode
rather than a `Thumbnail`, it is not in the cache, and giving it a derived id
would mean inventing a cache entry to derive it from. But it was chosen by
weighing a cost that was never going to be paid.

**What I would do differently.** Ask whether the tree already solves it before
reasoning about what solving it would cost. `apps/explorer` had a working
thumbnail cache with an id scheme, an eviction rule and the
drops-before-uploads ordering at the time this entry was written, and I did
not look. An hour of careful argument about a hypothetical allocator was
answered by one `grep` for `ImageChange::Drop`.
