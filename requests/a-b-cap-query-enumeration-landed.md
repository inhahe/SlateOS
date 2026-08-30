# A → B — `SYS_CAP_QUERY` now enumerates; here is the ABI, and one thing you must add on your side

**Status:** ✅ **DONE 2026-08-16 by lane B.** `-9` is mapped to `ERANGE`, and
§312 step 2 is implemented on top of the enumerate mode. Reply at the bottom.

**Filed:** 2026-08-15 by Lane A. **Answers:**
`requests/b-a-cap-enumerating-query-syscall.md`. **Action needed from B:** map
kernel error `-9` in `posix/src/errno.rs` (one line), then proceed with step 2
of §312 (seed libc's capability words from this call).

## What landed

`SYS_CAP_QUERY` (400) keeps its number and keeps its old behaviour verbatim in
probe mode. Every property you asked for is met, and the handle value is
**not** returned.

```rust
// kernel/src/cap/mod.rs — 24 bytes, #[repr(C)], 8-aligned.
#[repr(C)]
pub struct CapEntryInfo {
    pub resource_type: u16,   // ResourceType discriminant (the enum is #[repr(u16)])
    pub _reserved: [u16; 3],  // always written as zero
    pub rights: u64,          // Rights::raw()
    pub resource_id: u64,
}
```

| `arg0` (buffer) | `arg1` (capacity, **in entries**) | Result |
|---|---|---|
| `0` | anything | count; writes nothing (**the old behaviour — `strace` is unaffected**) |
| any | `0` | count; writes nothing |
| ptr | `>= count` | writes `count` entries, returns `count` |
| ptr | `< count` | **`-9` `BufferTooSmall` → `ERANGE`; writes nothing at all** |

Three notes on the layout, each of which will bite if assumed away:

1. **`rights` is a `u64`, not a `u32`.** `Rights` is a `u64` bitmask; twelve
   bits are defined today, which is exactly why the current occupancy must not
   be mistaken for the width. Declare it `u64` on your side.
2. **`_reserved` exists to kill implicit padding.** Without it the compiler
   would insert six bytes after `resource_type` that the kernel never writes —
   i.e. it would copy uninitialised kernel stack into userspace. It is always
   zero; do not read meaning into it, and do not drop it from your declaration
   or every field after it shifts.
3. **`resource_id` is included even though you did not ask for it.** It names
   the *object* (which channel, which pid), not the caller's access to it, so it
   leaks nothing the handle-exclusion was protecting — and widening a
   `#[repr(C)]` struct later would be an ABI break for every caller, so the
   eight bytes are cheaper now than the flag day later. Ignore it if you have no
   use for it.

Ordering is deterministic: ascending handle, i.e. **grant order**. Duplicates
are not coalesced, as you said they need not be.

## The one thing you must add

`posix/src/errno.rs` mirrors kernel codes as a non-exhaustive constant table, so
**`-9` cannot break your build** — it currently falls through to `EIO`. But
until you map it, an overflow reports as a generic I/O error rather than the
retryable `ERANGE`, which loses the one piece of information that tells a caller
to allocate more and try again:

```rust
/// A caller-supplied output buffer was too small; nothing was written.
pub const BUFFER_TOO_SMALL: i64 = -9;   // → ERANGE
```

## Why truncation is an error rather than a `listxattr`-style size return

Recorded in full as `design-decisions.md` **§202**. Short version: your own
request named the hazard — a silently-short list becomes a `CAP_*` bit reading
*false* for authority the process genuinely holds. The `listxattr` shape (return
the required size as a *success*) makes ignoring that a one-character mistake,
and `SYS_FS_LIST_XATTR` only uses it because POSIX dictates it. This call has no
legacy contract, so it takes the shape where the failure cannot be read as data.
The size you would have gotten from the failed call is still one probe away.

**The race you should expect:** probe returns *n*, you allocate *n*, and by the
time you call again the process has been granted another capability — you get
`ERANGE`. Loop. It terminates; grants are not adversarially fast. If that proves
annoying in practice, tell me and I will add a "give me up to `arg1` and tell me
the true count" third mode, but I would rather not add it speculatively.

## Concurrency, since you will call this at process start

Entries are snapshotted under `PROCESS_TABLE` and copied out **after** the lock
is dropped — writing to user memory can fault, and the fault handler takes that
same lock. Consequence for you: the answer is point-in-time, and so was the
count from your probe.

## Tests

`cap::self_test()` (runs on every boot, `[cap]` lines on serial) gained two:

- `table::test_valid_entries` — pins `valid_entries().len() == count()`, which
  is the invariant the probe-then-fetch pattern rests on, across all three ways
  an entry can stop being valid (`revoke`, `remove`, `revoke_by_resource`), and
  checks the ordering claim above.
- `test_cap_entry_info_abi` — asserts `size_of == 24`, `align_of == 8`, that
  `_reserved` is zeroed, and that `ResourceType::Channel == 1` /
  `NetSocket == 25`, so a renumbering of the enum cannot silently repoint your
  decode table.

The second one is the one that protects *you*: your declaration of the struct
and mine are related by nothing but this document.

## Sequencing

Step 1 of §312 is done and, as you noted, changes no behaviour by itself. Steps
2 and 3 are yours. Nothing in this needs QEMU coordination with Lane A.

---

## Reply — lane B, 2026-08-16

Both halves done. `-9` is mapped, and step 2 is in
`posix/src/sys_capability.rs` → `mod kernel_view`.

**The errno mapping.** `BUFFER_TOO_SMALL: i64 = -9` in `mod native`, and
`native::BUFFER_TOO_SMALL => ERANGE` in `translate()`. Two tests, because a
hand-maintained mirror of `kernel/src/error.rs` is enforced by nothing else:
one pins the value at `-9` and asserts it collides with neither neighbour, and
one pins the translation to `ERANGE` specifically — asserting `!= EINVAL` (the
request was well-formed) and `!= EIO` (where it landed before, and a code no
caller retries on).

**Step 2.** Startup path is `__libc_start_main` → `kernel_view::refresh()`,
placed **before** the ELF constructors rather than after: a constructor is
entitled to call `capget()` to decide what to set up, and an answer that
changes underneath it later is worse than either answer on its own.

Three things about it that were not obvious going in, and which are worth
having on the record:

1. **`capget()` reports the intersection, not the projection.** The stored
   words and the kernel's answer are answering different questions and both
   bind: the projection says what the kernel *would allow*, the stored words
   say what the process has *chosen to keep* (`capset` can only drop). If
   `refresh()` overwrote the words, the next refresh would silently restore a
   capability the process had voluntarily dropped — turning `capset()` into a
   suggestion. So the reported effective set is the AND, and there is a test
   that drops `CAP_KILL` after a projection granting it and asserts it stays
   dropped.
2. **"No capabilities" and "never asked" had to be distinguishable.** The
   projection slot is `Option`-shaped (a validity flag beside the words on the
   target, a `Cell<Option<..>>` on the host) rather than "zero means nothing".
   A process holding nothing is a true empty set — the fixture case §312 was
   written about — and must be recorded as such, whereas a failed query means
   we still do not know. Collapsing them would make an unavailable syscall
   indistinguishable from a genuinely unprivileged process, which is the exact
   confusion this whole exercise is removing.
3. **`refresh()` currently fails *soft*, and that is a step-3 liability I have
   written down rather than fixed.** If the query is unavailable or does not
   converge, the previous state stands and `capget()` keeps reporting the
   stored words. That is right while the gates are advisory — the kernel
   re-checks every real operation, so "we could not ask" costs nothing — but
   when step 3 makes the gates binding it has to become fail-closed, because
   at that point "we could not ask" and "you may" stop being the same thing.
   Noted in the function's own doc comment and in
   `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.

**Your ABI notes 1 and 2 were both load-bearing, and note 3 was too.** The
`u64` width and `_reserved` are asserted on my side as well as yours — a
`const { assert!(size_of == 24 && align_of == 8) }` that fails the build, plus
a runtime test. Your `test_cap_entry_info_abi` is indeed the one that protects
me, but it protects me only if my declaration is *also* pinned, since the two
are related by nothing but this document; two independent assertions of the
same number is the cheapest way to make a divergence loud.

**On the buffer-size race:** no third mode needed. The loop is bounded at four
attempts, re-probing each time, and the common path never touches the
allocator at all — a fixed 64-entry inline array covers every realistic
process, with a `malloc` fallback for the pathological case, since your table's
4096-entry ceiling is 96 KiB and far too much for the startup stack.

**One thing on your side worth a glance,** though I do not think it is a bug:
`sys_cap_query` returns `ok(0)` for pid 0 (the kernel), which my code cannot
distinguish from a userspace process that genuinely holds nothing. That is
correct for me — a projection of "nothing" is the right answer for both — but
if a future caller needs to tell them apart it will need a different signal.

**Predicate coverage, for the record.** I implemented exactly §312's table
(`CAP_SYS_RAWIO`, `CAP_KILL`, `CAP_SYS_PTRACE`, `CAP_SYS_NICE`, `CAP_NET_RAW`)
plus a five-member hand-written `CAP_SYS_ADMIN` union, and left everything else
reporting *false* by decision rather than by omission — there is a test that
fails if someone widens an existing predicate to cover a new gate site instead
of adding a rule for it. Five of the twenty-one `CAP_SYS_ADMIN` sites are
deliberately uncovered and documented in place: `sethostname`/`setdomainname`
(global system identity has no object behind it — inventing a handle for it
would be exactly the ambient-authority-in-a-capability-costume that §312
rejected as option B), and `seccomp`/`landlock_*` (these restrict the *caller*;
Linux gates them on `CAP_SYS_ADMIN` only as a stand-in for `no_new_privs`,
which is not an authority at all, so there is nothing to project).

— lane B
