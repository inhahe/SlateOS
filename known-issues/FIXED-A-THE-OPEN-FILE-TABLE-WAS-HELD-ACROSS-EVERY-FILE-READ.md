## FIXED-A-THE-OPEN-FILE-TABLE-WAS-HELD-ACROSS-EVERY-FILE-READ — and 31 more places do the same thing

**In short:** two parts of the kernel took the same two locks in opposite
orders, which is the textbook way to freeze a machine solid: CPU 1 holds lock
A and waits for B, CPU 2 holds B and waits for A, and neither ever moves
again. One of the two orders was on the path of *every single file read and
write* in the system. It is fixed; a new checker found 31 more places with
the same shape, and those are now fixed too. The checker is a build gate, so
the 32nd will be caught before it ships rather than after.

**Lane A. Found 2026-08-23, by lockdep, during boot batch 32.**

### What the two locks are

- **The filesystem lock** — one per mounted filesystem. `Vfs::readdir` takes
  it and then asks the filesystem to list a directory *while still holding
  it*. For `/proc` that listing is not a lookup, it is a *computation*:
  procfs makes its content up on demand, and doing so reaches into
  half the kernel — the open-file table, the process list, network state.
  So the order that really happens is **filesystem lock, then some
  module's own lock**.

- **`OPEN_FILES`** — the table of every open file handle, in
  `kernel/src/fs/handle.rs`. `read`, `write`, `read_at`, `write_at`,
  `fstat` and `ftruncate` all locked it and *kept it locked* while calling
  into the VFS, which takes the filesystem lock. That is **module's own
  lock, then filesystem lock** — the same two locks, backwards.

### Why it went unnoticed

The bug is older than the tooling that found it. Lockdep — the kernel's
lock-order validator — had been reporting this exact inversion for some
time, but it could only say *which types* were locked, because both sites
resolved to `kernel::sync::Mutex<T>::lock`, the generic function every lock
goes through. Commit `38f1d738e` taught it to walk one stack frame further
and print the *caller*, and the very next boot named both sides outright:

```
[lockdep]     held taken at:  ...Mutex<T>::lock+0x6f
[lockdep]                via: ...fs::vfs::Vfs::readdir+0x2ec
[lockdep]     acquiring at:   ...Mutex<T>::lock+0x6f
[lockdep]                via: ...fs::handle::list_handles+0x2c
```

`read`'s own comment had admitted the problem in the meantime: *"we hold the
lock across the VFS call — acceptable for early dev but should be improved."*
And three functions in the same file — `close`, `read_at_uncached`,
`read_dir_at` — already did it correctly. Six wrong, three right, in one
file, is what a rule with nothing enforcing it looks like after a while.

### The fix, and its shape

Commit `2b06e1070`. Snapshot what the VFS call needs (the path) under the
lock, drop the guard, make the call, then retake the lock and look the handle
up *again* by its number to write back the cursor and cached size. Never keep
a reference into the map across the call — the map may have been rebalanced,
and the handle may have been closed outright, which is not an error, since
the bytes really were transferred.

One deliberate semantic change: the cursor now advances with `max` rather
than `+=`. Two threads sharing one handle get an arbitrary interleaving
either way — Linux guards only the position update itself, with `f_pos_lock`,
and offers no more — but `max` keeps the cursor monotone, so a reader never
re-reads bytes it has already returned and a concurrent thread's progress is
never counted twice.

A side effect worth naming: file I/O no longer serialises the whole system
behind one global mutex.

### The other 31 — Status: FIXED, and now gated

`scripts/check-vfs-under-lock.py` (commit `7bc19a853`) enforces the rule
statically: *do not enter the VFS while holding a module-global lock.* It
shares `check-recursive-locks.py`'s parser, and it reported 31 further sites
when it was written. All 31 are fixed:

| File | Locks that were held across a VFS call | Fixed in |
|---|---|---|
| `fs/changetrack.rs` | `STATE` × 7 (`init`, and six via `ensure_init`) | `568306227` |
| `fs/fileselect.rs` | `SETS` × 6 (via `make_item`) | `caaa2dcb3` |
| `fs/filepicker.rs` | `PICKER` × 4 (via `build_listing`) | `6ac16124d` |
| `fs/tags.rs` | `INDEX` × 2 (via `read_tags`) | `97f63de75` |
| `logpersist.rs` | `STATE` × 3 (`flush`, `init_with_config`, `prune`) | `eb843d19d` |
| `fs/journal.rs`, `fs/mount_ns.rs`, `fs/rundialog.rs` | `JOURNAL`, `NAMESPACES`, `STATE` | `5606ba9bc` |
| `fs/overlay.rs` | `OVERLAYS` × 2 (`copy_up`, `which_layer`) | `3c157b9be` |
| `fs/sidebar.rs` | `HIDDEN_SECTIONS`, `EXPANDED_STATE` (`build`) | `ec9c5ff54` |
| `net/tftp.rs` | `SERVER_STATE` (via `handle_rrq`, and two more) | `d750e2d28` |
| `net/ssh.rs` | `STATE` (via `process_message`) | `48acb081f` |

Not all were equally live — several were `init` paths that run once at boot
with nothing racing them — but each was a standing invitation for the next
caller to be a live one, and the checker cannot tell the difference.

Most took the shape `fs::handle`'s fix did: snapshot under the lock, release,
call, retake and re-look-up by key. Three did not fit that mould and are
worth knowing about:

- **`logpersist.rs`** — its `STATE` was doing two jobs, guarding bookkeeping
  *and* serialising flushes against each other. Only the first survives being
  released across the writes, so the second moved to a `flushing` flag with an
  RAII `FlushGuard`.
- **`net/tftp.rs`** and **`net/ssh.rs`** — both tick functions held their
  state lock top to bottom, which was also what stopped two CPUs ticking at
  once. Same remedy: a `SERVER_TICKING` / `TICKING` flag with an RAII guard.
  (The rate limiter above each one is *not* a substitute — its
  load/compare/store is not atomic, so two CPUs can both pass it. That was
  already true before these changes.)
- **`net/ssh.rs`** additionally could not take its lock in bursts, because the
  protocol state machine works through one `&mut Session` for its whole
  duration. The session is *checked out* instead: `mem::replace`d out of its
  slot, processed on the tick's stack, then moved back, with a placeholder
  left behind flagged `checked_out` so `shutdown` does not send a disconnect
  to its zeroed `tcp_handle` — which is a valid handle belonging to somebody
  else.

**Now a boot-test gate** (commit `a57da9164`), next to `check_recursive_locks`.
It was deliberately not one during the sweep, because a gate then would have
failed every build for a backlog rather than for a regression.

One sharp edge for whoever trips it: the checker reports only the **first**
VFS call under a given guard. `net/tftp.rs` was named once and had three.

### Two unrelated bugs found in passing

- **`logpersist::prune` deleted the newest log first.** It sorted paths
  descending and popped, which takes the lexicographically *smallest* — and
  for `combined.jsonl` plus its rotations that order is
  `combined.1 < combined.2 < … < combined.jsonl`. So over the storage cap the
  kernel discarded the most recent rotation and kept the stalest, and the gap
  it left was the window immediately before the live file: the part anyone
  diagnosing a crash reads first. String order was wrong a second way, too —
  `.10` sorts between `.1` and `.2`. Fixed in `4de424aee` by carrying rotation
  age as an explicit `u32` rather than recovering it from the filename. The
  pre-existing `Test 6: Prune (empty)` passed just as happily with the order
  reversed, so `Test 7` was added to actually pin it.
- **TFTP counted a failed upload as a success.** `server_tick` wrote the
  received file with `let _ =` and incremented `SERVER_COMPLETED` regardless,
  so a full disk was indistinguishable from a completed transfer in both the
  log and the stats. Fixed in `d750e2d28`. The same commit replaced the WRQ
  existence probe — a full `read_file` whose result was dropped, i.e. a
  file's worth of kernel heap per *rejected* request — with `Vfs::exists`.
