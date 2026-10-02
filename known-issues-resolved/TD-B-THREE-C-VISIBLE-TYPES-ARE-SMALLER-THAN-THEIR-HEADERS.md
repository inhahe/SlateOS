## TD-B-THREE-C-VISIBLE-TYPES-ARE-SMALLER-THAN-THEIR-HEADERS (lane B, 2026-08-21) — FIXED 2026-08-21

**In short:** three of our opaque libc types are *smaller* than the type the C
header declares — `sem_t` is 4 bytes where `<semaphore.h>` says 32, `regex_t` is
16 where `<regex.h>` says 64, `glob_t` is 24 where `<glob.h>` says 72. This is
safe today and is not the bug that
`BUG-POSIX-SPAWN-FILE-ACTIONS-IS-4624-BYTES-IN-AN-80-BYTE-SLOT` was; it is the
opposite direction. A C caller reserves more room than we use, and we simply
leave the tail untouched. Logged because "safe" is not the same as "right".

**Where.** `posix/src/semaphore.rs` (`SemT`), `posix/src/regex.rs` (`RegexT`),
`posix/src/glob.rs` (`GlobT`). Each now carries a `const` assertion of the form
`size_of::<T>() <= <header size>`, which is the property that actually matters
and which will fire if one ever grows past its slot.

**When it would bite.** Undersizing is invisible as long as the caller only ever
passes us the *address* of its object. It stops being invisible if the caller
**copies the object by value** — `sem_t a = b;` moves 28 bytes of whatever the
caller's stack happened to hold along with our counter, and a `memcpy` or a
struct-return does the same. Nothing we ship does that today, and POSIX does not
promise these types are copyable, so no conforming program will. A ported one
might.

**`glob_t` is the least exposed of the three** despite the largest gap: its
three POSIX-visible members (`gl_pathc`, `gl_pathv`, `gl_offs`) sit at 0, 8 and
16, which is where glibc puts them, so a caller reading any documented field
reads ours. The 48 missing bytes are `gl_flags` and the six replacement-function
pointers (`gl_closedir`, `gl_readdir`, …) that `GLOB_ALTDIRFUNC` uses, which we
do not implement.

**Proper fix.** Pad each to its header's size with a `_reserved` tail, the way
`PosixSpawnattrT` already does to reach musl's 336, and tighten the `const`
assertions from `<=` to `==`. That is a few lines per type and costs only stack
bytes. It is deliberately *not* bundled into the file-actions fix, which was a
memory-safety bug on a boot-blocking path; this one changes no behaviour and
should land on its own so that it is separately revertible.

**Credit.** Found by lane A's sweep of every C-visible opaque struct in
`posix/src`, done while filing the file-actions bug — the same table that
confirmed `posix_spawn_file_actions_t` was the only one in the dangerous
direction.

### Resolution — 2026-08-21, lane B

Done as described: each of the three grew a `_reserved` tail to its header's
size, each gained a `const fn new()` so the tail is not a call-site concern,
and each `const` assertion tightened from `<=` to `==`.

| type | was | now | tail |
|---|---|---|---|
| `SemT` | 4 | 32 | `_reserved: [u8; 28]` |
| `RegexT` | 16 | 64 | `_reserved: [u8; 48]` |
| `GlobT` | 24 | 72 | `_reserved: [u8; 48]` |

`==` says strictly more than `<=`: it fires on a field *removal* as well as an
addition, and a removal is exactly what would silently shorten the by-value
copy this entry was about. The four pthread types keep `<=` — they have no
`_reserved` tail because POSIX leaves it undefined to move an initialised
`pthread_mutex_t` at all, so their by-value copy is already wrong whatever the
size. `posix/src/pthread.rs`'s module note now records that split.

**`sem_t`'s alignment is 4, and that is the correct answer, not a leftover.**
The two headers disagree — musl's `sem_t` is `struct { volatile int __val[8]; }`
(align 4), glibc's is `union { char __size[32]; long int __align; }` (align 8).
Ours must be no *stricter* than the loosest caller, because the caller supplies
the storage: had we forced 8 and a musl-compiled program handed us a `sem_t`
its compiler had placed at a 4-aligned address, every access would be
misaligned. Size must match exactly (it is the caller's slot); alignment must
only be no stricter. The first draft of the new test asserted 8 and failed,
which is how this got noticed.

**Two more tests were asserting the bug.** Fixing the sizes broke
`semaphore::tests::test_sem_size` (`assert_eq!(size_of::<SemT>(), 4)`) and
`glob::tests::test_glob_t_size` (`assert_eq!(size_of::<GlobT>(), 24)`). Both
had written our own undersizing down as the expected answer, so the one test
that could have caught each gap instead certified it. Both were derived from
our struct definition rather than from the header, which is why they could only
ever agree with whatever we had written. This is the third instance of the
species today — the other two were the three `malloc` tests that asserted a
NULL return (see `B-HOST-MALLOC-NEVER-WORKED-…` above). Replaced by
`test_sem_t_matches_musl_layout` and `test_glob_t_matches_glibc_layout`, which
assert size *and* every field offset against the header, plus
`test_regex_t_matches_musl_layout` for the third type, which had no size test
at all.

**Verified:** `cargo test -p posix --target x86_64-pc-windows-gnu` →
20,491 passed, 0 failed; `cargo clippy -p posix --target x86_64-pc-windows-gnu
--all-targets` → clean.
