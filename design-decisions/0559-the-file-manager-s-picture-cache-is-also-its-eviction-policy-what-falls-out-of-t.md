## 559. The file manager's picture cache is also its eviction policy: what falls out of the cache is what the display server is told to forget

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the file manager makes small previews ("thumbnails") of the files
in a folder and asks the display server to hold the pixels so it can draw them
by number. The display server never throws any of them away on its own — it
holds whatever it is given until it is explicitly told to stop — and there is a
limit on how much one program may have it hold at once. So somebody has to
decide *when* a preview stops being held. The decision: the answer is already
sitting in the file manager, in the size-limited preview cache it keeps in its
own memory, and the rule is simply "whatever falls out of that cache is what the
display server is told to forget."

**What was actually at stake.** Walk a photograph library — a hundred folders,
tens of thousands of files — and without an eviction rule the file manager
accumulates every preview it has ever produced on the display server's side.
Eventually the per-connection image budget (`MAX_IMAGE_BYTES_PER_LINK`, 256 MiB,
`gui/compositor/src/wire.rs:726`) refuses an upload. A refusal is
`?`-propagated by `apply_images` (`gui/window/src/app.rs:490`), which ends the
event loop — so the failure mode is not "previews stop appearing", it is "the
file manager quits". This had to be solved before the window could ship, not
after.

**The decision.** `ThumbnailCache` — the bounded LRU already in `thumbs.rs`,
already the thing the renderer reads to decide what to draw — grew a
`evicted: Vec<u64>` field. Every route out of the map records the image id that
left with it: LRU fall-off on insert, `invalidate` (a file changed on disk),
and `clear`. `take_evicted_image_ids()` drains that list, and `take_images()`
turns each into an `ImageChange::Drop`.

*Alternative considered — a separate residency tracker beside the cache,* with
its own size limit and its own policy (say, "hold the current folder plus the
last one"). *Against:* two bounded structures over the same set of files, which
must agree about what is held or else the renderer draws an id the server has
forgotten (which renders nothing, silently, by design). The cache is already
bounded, already ordered by use, and is already the authority on what can be
drawn; a second structure adds a way for those two answers to differ and no new
capability. *For:* it could hold more on the server than in local memory, which
would let the cache be small (memory) while the residency set is large (drawing
without re-decoding). That is a real property, and it is the reason this is a
tradeoff rather than an obvious call — but the two limits would then need
tuning against each other, and the win only shows up in a directory larger than
the local cache and smaller than the budget.

**The consequence, stated plainly.** The size of `ThumbnailCache` is now also
the size of the file manager's footprint inside the display server. Changing one
changes the other. That is written at `take_evicted_image_ids`.

**Two details that are easy to get wrong.**

- **Only actual removals are recorded.** `invalidate` takes a path and is called
  routinely for files that were never cached. Recording a drop for an id the
  server never held is not harmless: ids are derived from the file, so that same
  id may have been *re-uploaded* by a later insert of the same file, and the
  stale drop would forget the fresh pixels. `note_removed` records only when
  `map.remove` returned `Some`.
- **Drops go out before uploads, in one ordered list.** The budget arithmetic is
  `after = held - freed + incoming`, so a batch that uploads first is charged
  for both sets at once. Same reason as the wallpaper's slideshow (§557).

**Where it lives.** `apps/explorer/src/thumbs.rs` (`ThumbnailCache::evicted`,
`note_removed`, `clear`, `take_evicted_image_ids`, and the four tests under
"What leaves the cache must leave the compositor"),
`apps/explorer/src/main.rs` (`take_images`).
