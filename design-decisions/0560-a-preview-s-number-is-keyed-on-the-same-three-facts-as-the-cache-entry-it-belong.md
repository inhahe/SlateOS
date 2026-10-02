## 560. A preview's number is keyed on the same three facts as the cache entry it belongs to

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** each preview picture is given a number, and the display server
stores the pixels under it. That number used to be computed from the file's path
and its last-modified time. But the file manager's own cache files previews
under path, modified-time **and length**. Those two disagree for a file that is
written twice within the same second — the clock does not move, the length does
— which produced two different cache entries claiming the same number. The
second upload silently overwrites the first, and one of the two entries then
draws the other's picture. The fix is to key the number on all three.

**Why a second's resolution is not a corner case.** `mtime_secs` truncates to
whole seconds because that is what the filesystem records. Any program that
writes a file, then rewrites it — a download that resumes, an editor that saves
twice, a build that regenerates an image — does this routinely. The old code was
not wrong about a rare race; it was wrong about a common one that happened to be
invisible because nothing had ever uploaded the pixels.

**The decision.** `thumbnail_image_id(&Thumbnail)` is deleted and replaced by
`pub fn image_id(path: &Path, mtime: u64, size: u64) -> u64`, which mixes the
length's eight bytes into the existing `simple_hash` with an FNV round.
`CacheKey::image_id()` calls it with the key's own three fields, so the id and
the key cannot drift apart by construction.

*Alternative considered — a counter,* handing out a fresh number per upload and
storing it in the cache entry. *For:* collisions become impossible rather than
improbable; it is what the wallpaper does (`alloc_image_id`). *Against:* the id
would then be state that must be carried through every path that produces a
`Thumbnail`, and a cache miss followed by a re-generation of the *same* file
would get a new number and a new upload where the derived id gets a free
replacement of the existing one. The wallpaper holds one picture and can afford
a counter; the file manager holds hundreds.

**A deliberate non-change.** `simple_hash(path, mtime)` is left exactly as it
was, because it also names the on-disk thumbnail cache *files*. Changing it
would invalidate every user's saved thumbnails for no benefit — a disk cache
keyed on two facts is merely conservative (it re-generates when it need not),
whereas an *id* keyed on two facts is incorrect (it conflates).

**The signature change this forced.** `render_thumbnail` now takes the id as a
parameter rather than deriving it, because a `Thumbnail` knows its pixels and
its dimensions and knows nothing about the path, time or length it came from.
Deriving an id inside it would have meant either passing those three in anyway
or storing them on every thumbnail.

**Where it lives.** `apps/explorer/src/thumbs.rs` (`image_id`,
`CacheKey::image_id`, `render_thumbnail`), `apps/explorer/src/main.rs`
(`pump_thumbnails`, `drawable_thumb`). The test is
`two_versions_of_a_file_written_in_the_same_second_get_different_ids`.
