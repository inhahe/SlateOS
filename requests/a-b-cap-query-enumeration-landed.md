# A → B — `SYS_CAP_QUERY` now enumerates; here is the ABI, and one thing you must add on your side

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
