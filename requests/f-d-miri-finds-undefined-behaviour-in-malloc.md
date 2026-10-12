# F → D — Miri finds undefined behaviour in `malloc` and `envz_merge`; and what `posix` needs before Miri can test the rest

**From:** Lane F. **To:** Lane D (`posix/src/malloc.rs`, `posix/src/malloc/dlmalloc.rs`, `posix/src/argz.rs`, `posix/src/string.rs`).
**Filed:** 2026-10-12, on the operator's instruction. **Status:** open.

**In short:** the operator adopted Miri for `unsafe` code that has host
tests (design-decisions §1393). Its first run over `posix` found undefined
behaviour in two places: `envz_merge` (section 1b), and the allocator every
SlateOS program links:

- the dlmalloc port keeps raw pointers into its own bin array inside heap
  chunks;
- every `malloc` and `free` then takes a fresh `&mut` to the whole
  allocator (`HeapGuard::heap`, `malloc.rs:302`);
- under Rust's aliasing rules that `&mut` invalidates the stored pointers,
  and the next `free` writes through one of them.

The native tests pass, because nothing has yet compiled the code in a way
that breaks. The compiler may, though: it treats a `&mut self` as unaliased
(LLVM's `noalias`), and may reorder or drop accesses on that basis.

Miri also cannot yet run most of `posix`. Its SSE2 string scanners and
x87 helpers are inline assembly, which Miri cannot interpret.

## 1. The finding

`posix`'s own test `search::tests::test_tdelete_keeps_the_tree_valid` reaches it (`tdelete` frees a node):

    error: Undefined Behavior: attempting a write access using <17659920> at alloc187645[0x1f8],
           but that tag does not exist in the borrow stack for this location
        --> posix\src\malloc\dlmalloc.rs:1329:13
    1329 |             (*f).next = b;
    help: <17659920> was created by a SharedReadWrite retag at offsets [0x120..0x330]
        --> posix\src\malloc\dlmalloc.rs:1173:9
    1173 |         self.smallbins.as_mut_ptr().wrapping_add(idx).cast()
    help: <17659920> was later invalidated at offsets [0x0..0x3c8] by a Unique retag
        --> posix\src\malloc.rs:302:18
     302 |         unsafe { &mut *HEAP.0.get() }
    stack: Dlmalloc::unlink_small_chunk <- unlink_chunk <- free <- malloc::free
           <- search::tdelete <- search::tests::test_tdelete_keeps_the_tree_valid

Reproduce (Windows host; Miri interprets the tests as Linux code):

    rustup component add miri rust-src --toolchain nightly-x86_64-pc-windows-gnu
    cargo +nightly-x86_64-pc-windows-gnu miri test -p posix --lib \
        --target x86_64-unknown-linux-gnu -- search::

The usual fixes:
- reach the allocator's state through one raw pointer for the whole call,
  derived once, rather than a `&mut` per call, and have the methods take
  `*mut Self` or use `ptr::addr_of_mut!` for the bins;
- or keep the bin heads outside the struct the `&mut` covers.

`MIRIFLAGS=-Zmiri-tree-borrows` runs the newer, more permissive aliasing
model. Whether it passes there says whether the pattern is wrong only under
Stacked Borrows' rules or under both. Either way the `noalias` reasoning
above applies.

## 1b. A second finding: `envz_merge` reads a byte past the slice it was given

`argz::tests::envz_is_glibcs`:

    error: Undefined Behavior: trying to retag from <12578132> for SharedReadOnly permission
           at alloc5914211[0x58], but that tag does not exist in the borrow stack for this location
       --> posix\src\argz.rs:768:37
    768 |  let with_nul = unsafe { core::slice::from_raw_parts(e.as_ptr(), e.len() + 1) };
    help: <12578132> was created by a SharedReadOnly retag at offsets [0x55..0x58]
    stack: argz::envz_forms::envz_merge <- argz::tests::envz_is_glibcs

`e` is the entry without its NUL. The pointer `e.as_ptr()` carries
permission for `e`'s bytes only, so a slice one byte longer reads outside
it. The NUL is really there, but this pointer may not reach it. The fix is
to take the pointer, or the slice with its NUL, from the buffer that holds
the NUL: the argz vector, sliced to `e`'s offset and `e.len() + 1`.

## 1c. The other modules tried

| Module | Result under Miri |
|---|---|
| `obstack`, `list`, `objtable` | pass (5, 18 and 2 tests) |
| `environ`, `getopt`, `libgen` | stop at the string functions' inline assembly (section 2) |
| `malloc` | still running after 40 minutes: needs a narrower filter |

## 2. What keeps Miri from the rest of `posix`

Host builds already turn system calls into ENOSYS, so the remaining
blockers are assembly that compiles on the host. Miri stops a test binary
at the first one it meets. `string::`'s first test stops at `zeros16`
(`string.rs:720`), the SSE2 scanner behind `strlen` and the rest. The
assembly sites by file: `ld80.rs` 23, `string.rs` 16, `fenv.rs` 11,
`syscall.rs` 9 (all behind `target_os = "none"` already), and
`ucontext`, `signal`, `crt`, `besl`, `pthread`, `narrow`, `c23math`,
`wchar`, `tls`, `random` and `process` 2 to 4 each.

Asked:
- a portable `#[cfg(miri)]` twin for each scanner leaf in `string.rs` and
  `wchar.rs`, byte at a time, so that the string functions -- the most
  pointer-heavy code in the library -- can run under Miri;
- `#[cfg_attr(miri, ignore)]`, with a comment saying why, on tests that
  only make sense with real x87 or FPU-environment instructions (`ld80`,
  `fenv`).

Then run Miri module by module as you change `unsafe` code (CLAUDE.md,
"Fuzzing, Miri and mutation testing"). The 9,382 tests take hours under
Miri all at once.

## Replies
