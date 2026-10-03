## §116 — A recorded mtime is only evidence once it has outlived its own granularity

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Where:** `apps/diffcore/src/lib.rs` — `FileSync::{record, touch, stat, changed}`,
`FileSync::mtime_racy`, `MTIME_SETTLE`. Consumers: `Document::disk_changed` in
`apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`.

### The problem

`FileSync` is the external-edit detector both text editors embed. It records a
file's content (the merge ancestor) and its mtime, and `changed()` used the
mtime as a fast path: same mtime as recorded ⇒ `Unchanged`, skip the read.

That inference is false. Filesystem timestamps are coarse — FAT to 2 s, ext3 to
1 s, NTFS nominally to 100 ns but stamped from a system clock that advances in
~15.6 ms steps — so a write landing in the same tick as the recording carries
the *same* mtime. The editor then never raises the external-change prompt, and
the next save overwrites the external edit with no merge and no warning. This
is silent data loss, not a missed notification, and it was found by diffcore's
own test suite failing intermittently on a fast machine.

### The decision: take the raciness verdict at record time, git's way

`record`/`touch` funnel through a private `stat()` that captures the mtime and,
**at that moment**, decides whether it can ever be evidence:
`now - mtime < MTIME_SETTLE` sets `mtime_racy`, and `changed()` refuses the fast
path while it is set. This is git's "racily clean" rule (`read-cache.c`) applied
to a single file instead of an index.

**Alternatives rejected:**

- *Compare `SystemTime::now()` inside `changed()` instead of storing a flag.*
  Unsound, and the failure is the same silent one. The aliasing write happened
  **inside** the granularity window, which has already closed by the time anyone
  asks; the mtime it left behind is indistinguishable from the honest one
  forever. Raciness is a property of the *recording*, so it must be decided
  there.
- *Delete the fast path.* Sound, but it throws away a real optimisation: a file
  nobody has touched should not be re-read on every check.
- *Pick `MTIME_SETTLE` from the actual filesystem's granularity.* No portable
  way to ask, and the asymmetry makes guessing safe in one direction only — too
  large costs a content read, too small loses an edit. So the constant is sized
  for the coarsest filesystem we could plausibly be asked to watch (2 s), not
  the finest.

### The accepted cost: the flag is sticky

`mtime_racy` is cleared only by the next `record`/`touch`, never by the passage
of time — because ageing genuinely does not help: if the aliasing write already
happened, the file's mtime *equals* the recorded one permanently. So a file
recorded right after a save is content-compared on every check until the next
save.

Git accepts exactly this, smudging a racily-clean entry back to trusted only
when it rewrites the index. The equivalent refinement here — clear the flag from
`changed()` once a content comparison came back identical *and* the mtime has
outlived `MTIME_SETTLE` (at which point no future write can alias it) — is
sound, but forces `changed()` to take `&mut self`, which locks read-only UI code
out of asking. Deferred: `check_external_change` has no production caller yet,
so the poll rate that would justify the API cost is not merely unmeasured, it is
zero. Recorded in `known-issues.md` under the fixed entry.

### Consequences

- Any future mtime-based caching in this tree (a file indexer, a build-stamp
  check, a package-manager freshness test) inherits the same trap. The rule to
  carry over is that a timestamp is evidence of *absence* of change only once it
  is older than the clock's own granularity; before that it is evidence of
  nothing.
- The regression test deliberately contains no `sleep`. A sleep would separate
  the two writes into different ticks and hide the exact adjacency under test.
