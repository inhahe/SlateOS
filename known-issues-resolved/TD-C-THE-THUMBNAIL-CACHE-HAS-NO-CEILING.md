## TD-C-THE-THUMBNAIL-CACHE-HAS-NO-CEILING -- FIXED 2026-09-16

**Fixed the same day.** `DiskCache::enforce_cap` deletes oldest-first until the
directory is under a byte budget, run once when the generator takes its default
cache. The budget is a *parameter*; `DEFAULT_DISK_CACHE_BYTES` (256 MiB) only
supplies it, so the install-time sizing §4.1 wants can pass a number later
without touching the eviction.

**What is still not done, and is not this entry's:** the sizing itself, which
needs a filesystem-capacity reading nothing here can make, and the
pressure-aware shrinking, which is lane A's kernel shrinker. Unbounded growth
is what got fixed; "the right bound, adjusted under pressure" is what remains.

Eviction is oldest-*written*, not least-recently-*used*, because a true LRU
needs a last-read time that is not reliably available and would cost a write
per thumbnail served. Recorded here because it is the first thing someone will
want to change, and the reason is not obvious from the code.

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/explorer/src/thumbs.rs` — `DiskCache`.

**In short:** every thumbnail the file manager makes is kept on disk forever.
Nothing deletes old ones and nothing limits how much room they take, so the
folder grows for as long as the machine is used. Browsing a large photo
collection once leaves its thumbnails behind permanently.

**What is right, and was checked rather than assumed.** The cache is wired in
production, not only in tests; it lives under the per-user path and never
beside the source files, which is what §4.1 requires; and invalidation is
sound, because `cache_filename` keys on the path, the source mtime *and* the
size cap, so a changed file or a changed cap simply misses and regenerates.

**What is missing** is §4.1's "size cap" bullet: no byte budget and no notion
of cold entries. The `evicted` field nearby belongs to the *in-memory* LRU,
which bounds a `HashMap` and not the directory.

**Correction, made within the hour of filing this.** This entry first said
there was "no eviction" at all. That is wrong: `DiskCache::purge_stale` exists,
is careful -- it compares cache filenames as *bytes*, with a comment noting
that rendering a name lossily first could make a foreign file *look* like one
of ours and get it deleted -- and is tested. **It is called by nothing outside
those tests.** So the unbounded growth is not a missing mechanism; it is an
unused one, which is the seventh capability found in this state today.

**But it cannot simply be wired up, which is the useful part.** `purge_stale`
takes the set of entries that are still valid and deletes everything else. The
only such set explorer can produce is *the directory it is currently showing*,
and calling it with that would delete every other folder's thumbnails on every
navigation -- bounding growth by destroying the cache. Its contract assumes a
global "files we still care about", and nothing in the tree keeps one.

So the two halves are: `purge_stale` answers "this source is gone or changed",
and the missing cap answers "this cache is too big". They are different
questions, and only the first has code.

**Why this is not simply "add a cap".** The spec asks for two things, and only
one is ours:

* An install-time byte budget "based on total disk size at install (default
  ~0.5% of the system drive)". Sizing it needs the capacity of the drive, which
  this tree can read for a block device but not for the filesystem a home
  directory is on -- the same gap that leaves `disk_fraction` at `None` in the
  shell's widget.
* Pressure-aware shrinking "via the kernel shrinker subsystem", which is lane
  A's and does not exist.

**A fixed default cap with LRU eviction is buildable here today** and would
turn unbounded growth into bounded growth, which is most of the value. It
should be written so the budget is a parameter rather than a constant, so that
the install-time sizing can supply it later without touching the eviction
logic -- the shape `save_within` uses in `apps/archivemanager`, where the
constant is the shipped value and a test reaches the branch honestly.

**Worth stating plainly:** the project's own operating instructions single out
disk space as a real, finite constraint that build output has already filled
once. A cache with no ceiling is the same failure with a slower fuse.
