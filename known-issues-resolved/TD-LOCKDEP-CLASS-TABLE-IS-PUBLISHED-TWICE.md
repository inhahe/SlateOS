### TD-LOCKDEP-CLASS-TABLE-IS-PUBLISHED-TWICE -- RESOLUTION 2026-08-17 -- fixed lock-free; the recommendation above was wrong

**In short:** the defect is fixed, but *not* the way this entry recommended. Its
"Option 3" -- serialise class registration behind "the lockdep-internal lock that
`record_edge` already needs" -- cannot be implemented, and would deadlock the
kernel on the first lock acquisition if it were. There is no such lock, and there
can never be one. What actually landed is the per-slot flag the entry dismissed
as the inferior option; it turns out to be the only option.

**Why Option 3 is impossible.** `sync::Mutex::lock` calls
`lockdep::lock_acquire` (`kernel/src/sync.rs:438`, and again at 534). So a
`sync::Mutex` *inside* lockdep would re-enter lockdep on every acquire of the
lockdep lock itself -- infinite recursion on the first lock in the boot, not a
subtle ordering bug. This is why `lockdep` holds no lock of any kind: a grep of
the module confirms it contains none, and instead relies on
`IN_LOCKDEP: [bool; MAX_CPUS]`, a per-CPU re-entrancy guard, precisely because a
real lock is unavailable to it. The phrase "the lockdep-internal lock that
`record_edge` already needs" in the recommendation above describes a lock that
does not exist and was never a candidate; it was written from the assumption that
a cold path can always afford a mutex, without checking whether *this* module is
allowed to hold one. Recorded here rather than deleted so the reasoning error is
visible: the cost of a lock is not always its contention.

For the same reason, the entry's aside that "registration is cold by
construction, so the lock costs nothing measurable" is beside the point. The
problem was never the cost.

**What landed instead** -- Option 1 of the entry's step 3, a parallel
publication-flag array:

```rust
/// Publication flag per class slot: `true` once `CLASSES[i]` is fully written.
static CLASS_READY: [AtomicBool; MAX_CLASSES] =
    [const { AtomicBool::new(false) }; MAX_CLASSES];

#[inline]
fn class_is_ready(idx: usize) -> bool {
    CLASS_READY.get(idx).is_some_and(|f| f.load(Ordering::Acquire))
}
```

A parallel array rather than an `initialized: AtomicBool` field inside
`LockClass`, because `LockClass` is `Copy` and several readers depend on that:
they snapshot a whole slot in one indexing operation (`let c = unsafe {
CLASSES[i] };`) rather than borrowing it. An atomic field would make the struct
non-`Copy` and force those readers back to holding a reference into a mutable
static, which is the very thing the fix removes.

The reservation is now a bounded CAS loop, which deletes the racy `fetch_sub`
undo the entry describes as its second smaller defect -- an overflowing claimer
never increments the counter at all, so there is nothing to undo and the count
can never transiently exceed `MAX_CLASSES`:

```rust
let idx = loop {
    let cur = CLASS_COUNT.load(Ordering::Relaxed);
    if cur as usize >= MAX_CLASSES {
        return None;
    }
    if CLASS_COUNT
        .compare_exchange_weak(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        break cur as usize;
    }
};
```

and the slot is published with a `Release` store *after* it is filled and
*before* `hash_insert`, so both publication paths -- the counting one and the
hash one -- are now ordered:

```rust
CLASS_READY[idx].store(true, Ordering::Release);
Some(hash_insert(lock_addr, idx as u16))
```

All three counting readers named in the table above (`snapshot`,
`dump_held_locks`, `verify_class_index`) now skip un-published slots.
`dump_held_locks` says so explicitly rather than silently omitting the row --
`<class N still being registered>` -- because this entry's sibling
(`BUG-BOOT-SPINLOCK-STALL-UNNAMED`) exists entirely because a dump printed a
confident answer it had not earned. `verify_class_index` needed the gate in two
places, not one: its linear-scan oracle would otherwise "find" an unpublished
slot whose `id` is still zero and report the *index checker* as broken.

**Two extra sites found while auditing, both now fixed.** A grep for `CLASSES[`
turned up `class_name` (line ~913) forming `&CLASSES[idx]` -- a shared reference
to a mutable static, which is UB the moment another CPU appends, and which this
same file's `class_id` doc comment already warns against. `snapshot()` did the
same. Both now read by `Copy` or via `&raw const`, and `class_name` returns `"?"`
for an unpublished index. Confirmed *not* needing a gate: `class_id`'s hot
callers (`hash_lookup:213`, `hash_insert:244`) arrive through the ordered hash
path, where the Release/Acquire pair already guarantees the slot is complete.

**Regression test.** A new "Test 9: slot publication" in `lockdep::self_test()`
asserts that every slot below `CLASS_COUNT` is ready, that the slot *at*
`CLASS_COUNT` is not, that `class_is_ready(MAX_CLASSES)` is false, and that
`class_name` of an unpublished index is `"?"`. It deliberately does **not** try
to reproduce the race -- a two-CPU interleaving inside a six-line window is not
reproducible on demand -- and instead pins the failure mode that a future edit
would actually cause: forgetting the `CLASS_READY` store, which would make all
three counting readers silently skip every real class and turn every lock dump
into a blank one. A quiet diagnostic is the failure mode worth a boot-time
assert.

**Verified** by boot #16 (`PASS`, every gate green, consecutive clean streak 6):

```
1957:[lockdep]   Slot publication (ready flags vs count): OK
1959:[lockdep] Self-test PASSED
1958:[lockdep]   Stats: 5 classes, 5 edges, 3 violations
```

The 3 violations are the intentional ones the self-test provokes (two AB/BA
warnings and one `*** SELF-DEADLOCK ***`), unchanged from before this fix. The
new Test 9 line sits between the pre-existing `class hash (early)` check and the
stats line, i.e. it runs at "Step 20z" ahead of the whole self-test battery,
where a broken publication would be caught before any of the code that depends
on it. `cargo clippy -p kernel` exits 0 with **54** warnings naming
`lockdep.rs`, against a 55 baseline -- one fewer, none new, despite the module
growing by a static array, a helper and a test.
