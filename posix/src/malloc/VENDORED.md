# `dlmalloc.rs` — vendored allocator core

`dlmalloc.rs` in this directory is the allocator core of **dlmalloc-rs**, Alex
Crichton's Rust port of Doug Lea's `malloc`. It is what `posix`'s `malloc`,
`free`, `realloc`, `calloc` and the aligned forms run on (see `../malloc.rs`).

| | |
|---|---|
| Upstream | <https://github.com/alexcrichton/dlmalloc-rs> |
| Crate | `dlmalloc` **0.2.14** from crates.io (published 2026-05-16) |
| `.crate` SHA-256 | `ad5208a115eaba24916f7456929832e310a81518c641f93fee4f89aa93aa3675` (matches the checksum crates.io lists) |
| Upstream file | `src/dlmalloc.rs`, SHA-256 `e476e331d4b74cd7ae7b8689242169dd25aa9c4141d5a062b4debd7158cf5492` |
| Licence | MIT or Apache-2.0, at your option: `LICENSE-MIT`, `LICENSE-APACHE`, copied from the crate unchanged. The C original it ports is Doug Lea's, released to the public domain (CC0). |

Only `src/dlmalloc.rs` is taken. The crate's `lib.rs` (a `GlobalAlloc` wrapper and
per-platform system allocators) is not: `../malloc.rs` supplies the system
interface (`SlateSystem`) and the C API, and the `Allocator` trait the core is
written against is declared there too, with upstream's documentation.

## Why vendored rather than a Cargo dependency

The core has to be changed for this kernel, and the changes are not
expressible through the crate's `Allocator` trait: whether two neighbouring
regions are merged is decided inside `sys_alloc`, not asked of the system. A
dependency would also have been `posix`'s first registry dependency, pulled into
the sysroot build of every program.

## Local changes

Every change is marked in the source with `LOCAL CHANGE (SlateOS)` or
`LOCAL ADDITION (SlateOS)`. To see them all: `diff` this file against
upstream's `src/dlmalloc.rs` at the version above.

1. **Large requests get a mapping of their own.** Added `MMAP_THRESHOLD`
   (256 KiB, C dlmalloc's `DEFAULT_MMAP_THRESHOLD`), `mmap_alloc` (a port of C's
   `mmap_alloc`, with checked arithmetic in place of C's wrap-then-test) and the
   call to it at the head of `sys_alloc`, under C's condition that the heap is
   already initialised. The Rust port omits this path — it has no `mmap_alloc`
   — although it kept everything that *consumes* such chunks (`free`'s and
   `realloc`'s "mmapped" branches, `mmap_resize`), so the chunk layout is the
   one those already expect. Without it, a large block lives in a segment that
   is released only when every block in it is free.

2. **Address-contiguous regions are never merged.** Upstream's `sys_alloc`
   extends an existing segment when a new region happens to sit immediately
   after it, or before it. Both branches are replaced by `add_segment`. The
   reason is the kernel: native `munmap` removes a VMA record only when given
   that mapping's own base address, so a merged segment released as one range
   would leave the second mapping's record behind. With merging gone, every
   segment is exactly one `mmap` and is released as one.

3. **`usable_size`** added — C dlmalloc's `dlmalloc_usable_size`, for
   `malloc_usable_size`.

4. **Consistency checks: the heavy walks off, the assertions on in tests.**
   Upstream gates both its `check_*` heap walks and its own `debug_assert!` /
   `debug_assert_eq!` macros (which shadow the standard ones) on
   `cfg!(all(feature = "debug", debug_assertions))`. `posix` has no such
   feature, and `unexpected_cfgs` would flag the name. So the nine `check_*`
   walks read `cfg!(any())` — always false — because `check_malloc_state`
   visits every chunk of the heap on every call, and on the host that heap is
   shared by the whole test suite. The two macros read `cfg!(test)`: upstream's
   cheap pointer and size assertions run in this crate's unit tests, where a
   broken invariant should fail the test at the point it breaks.

5. **`use crate::Allocator` → `use super::Allocator`**, since the trait lives in
   `../malloc.rs`.

6. **The test module runs against `SlateSystem`** instead of upstream's
   `System`, through a three-line shim at its top. The tests themselves are
   upstream's, unchanged.

The file is also exempt from this workspace's lints (the `#![allow]` at its top
says which and why): it is upstream's code, and restyling it would make every
future comparison with upstream a diff of noise.

## Updating

Download the new `.crate`, check its SHA-256 against crates.io's, replace this
file with its `src/dlmalloc.rs`, and re-apply the six changes above. Run
`cargo test -p posix --lib --target x86_64-pc-windows-gnu`: the upstream tests
and `malloc.rs`'s own run the new core on the host. Update the table above.
