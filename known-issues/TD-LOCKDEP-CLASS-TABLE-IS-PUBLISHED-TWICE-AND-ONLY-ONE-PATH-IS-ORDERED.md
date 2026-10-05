## TD-LOCKDEP-CLASS-TABLE-IS-PUBLISHED-TWICE-AND-ONLY-ONE-PATH-IS-ORDERED (lane A, 2026-08-17) - **FIXED** (`e3ae7bae1`)

**In short:** the lock-order checker keeps a table of the locks it has seen. A
CPU adding a row to that table reserves the row first and fills it in second.
Three of the checker's own report-printing functions decide how many rows to
read from the reservation counter -- so they can read a row that is reserved but
not yet filled, and print a lock name and address that are still zero or, worse,
half-written garbage. This is diagnostics-only: it can corrupt a *report* about
a deadlock, never the deadlock detection itself. It has never been observed,
because in practice the only caller that hits the dump is the CPU that just
finished writing the row.

**Where:** `kernel/src/lockdep.rs`.

The writer, `find_or_register_class` (line ~675):

```rust
// Register new class.
let idx = CLASS_COUNT.fetch_add(1, Ordering::Relaxed) as usize;   // <-- reserve
if idx >= MAX_CLASSES {
    CLASS_COUNT.fetch_sub(1, Ordering::Relaxed);                  // <-- racy undo
    return None;
}
unsafe {
    CLASSES[idx].id = lock_addr;                                  // <-- fill
    let copy_len = name.len().min(16);
    CLASSES[idx].name[..copy_len].copy_from_slice(&name[..copy_len]);
    CLASSES[idx].name_len = copy_len as u8;
}
// Publish only after the entry is fully written -- `hash_insert`'s Release
// store is what makes it safe for another CPU to follow the index here.
Some(hash_insert(lock_addr, idx as u16))
```

The comment is correct as far as it goes, and the ordering it describes is real:
a reader that arrives through `hash_lookup` cannot see `idx` until the matching
Release store in `hash_insert`, by which time the slot is complete. **But
`hash_insert` is not the only way to reach a slot.** Three readers reach
`CLASSES` by counting instead, bounding their loop with `CLASS_COUNT` -- the very
counter that is incremented *before* the slot is written:

| Line | Function | What it does with the slot |
|---|---|---|
| ~548 | `snapshot()` | pushes `id`/`name`/`name_len` into a heap `Vec` |
| ~635 | `dump_held_locks()` | prints `name @ id` for each held lock |
| ~1022 | `verify_class_index()` | asserts the index agrees with a linear scan |

For each of these, the sequence `CPU0: fetch_add -> (interrupt / other CPU runs)
-> CPU1: load CLASS_COUNT, read CLASSES[idx]` observes a reserved-but-empty
slot. Both loads are `Relaxed`, so there is not even an acquire fence to pair
against; on x86-64 the store order happens to be preserved by the hardware, but
that is an accident of the target, not something the code establishes, and
`copy_from_slice` into `name` is not a single store in any case.

Two smaller defects in the same six lines:

- **The "undo" on a full table is racy.** If two CPUs both overflow
  concurrently, each does `fetch_add` then `fetch_sub`; interleaved with a third
  CPU's successful `fetch_add`, the counter can end up naming a slot nobody
  owns, or below the true number of live classes -- which would make the readers
  above *skip* real entries. `MAX_CLASSES` is 128 and the kernel currently
  registers 5, so this is unreachable today.
- **`fetch_sub` can transiently expose a count above `MAX_CLASSES`.** All three
  readers do `.min(MAX_CLASSES)`, so this is contained -- but it is contained by
  every caller remembering to clamp, rather than by the counter being correct.

**Why it has not bitten:** `dump_held_locks` is called from the violation
reporter on the CPU that is *itself* mid-`lock_acquire`, so the slot it names was
written by that same CPU before the call -- the held-lock stack only ever
contains indices whose slots that CPU has already filled. `snapshot()` is
`#[allow(dead_code)]`. `verify_class_index` runs at two fixed boot points that
are effectively single-threaded. So the window is real but currently
unreachable, which is exactly the kind of bug that surfaces the first time
someone calls the dump from a different CPU -- e.g. an NMI-time or watchdog-time
lock dump, which is precisely the feature this table exists to make possible.

**The proper fix** -- publish the slot, not the reservation:

1. Fill `CLASSES[idx]` completely, then make the count the publication point with
   a Release store: replace the eager `fetch_add` with a reservation counter that
   is *separate* from the published count, i.e. `NEXT_SLOT.fetch_add(1, Relaxed)`
   to claim, and after filling, `CLASS_COUNT.fetch_max(idx + 1, Release)` (or a
   CAS loop) to publish. Readers then use `CLASS_COUNT.load(Acquire)`.
   `fetch_max` also removes the need to undo anything on overflow: an overflowing
   claimer simply never publishes, so the racy `fetch_sub` disappears.
2. Give the three counting readers `Acquire` loads to pair with it.
3. Note that with `fetch_max` publication, a published count of `N` no longer
   guarantees every slot below `N` is filled if claims complete out of order.
   Either keep a per-slot `initialized: AtomicBool` (readers skip un-set slots),
   or -- simpler and adequate for a 128-entry diagnostic table -- serialise
   registration behind the lockdep-internal lock that `record_edge` already needs,
   and keep a single `CLASS_COUNT` written last with Release. Registration is
   cold by construction (it happens once per distinct lock address, ever), so the
   lock costs nothing measurable; the O(1) `hash_lookup` fast path at the top of
   `find_or_register_class` returns before reaching it on every subsequent
   acquire.

Option 3 is the recommended one: it makes "the count is the publication point"
true by construction instead of by a per-slot flag every future reader must
remember to check, and it fixes the overflow race in the same stroke.

**How it was found:** reading `find_or_register_class` while adding the class
address to `dump_held_locks`' output (the `@ {:#x}` field) -- i.e. while making
one of the three unordered readers print *more* of the slot it may be reading too
early.
