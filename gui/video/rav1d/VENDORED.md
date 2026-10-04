# rav1d, vendored

rav1d is the Rust port of dav1d, the AV1 decoder Chrome and Firefox run. It is
here because AVIF pictures are AV1 frames (design-decisions.md §1333), and
because an AV1 video decoder belongs beside the other video codecs in
`gui/video/`.

## Where it came from

| | |
|---|---|
| Upstream | <https://github.com/memorysafety/rav1d> |
| Version | 1.1.0, the crates.io package |
| Package SHA-256 | `1932f060d5e7bd49dc9f8b272c1dc5e9ce0ffe141c28be900265d3989b36c9ed` |
| Licence | BSD-2-Clause, `COPYING` (dav1d's, which rav1d keeps) |

## What was taken

`lib.rs`, `include/**/*.rs` and `src/*.rs` -- all of the Rust -- with
`COPYING`, `README.md`, `THANKS.md` and `NEWS.dav1d`. Not taken: the x86 and
Arm assembly (`src/x86/`, `src/arm/`, `src/ext/`), the build script that
assembles it, the `tools/` binaries, `rust-toolchain.toml` and the lock file.
The decoder is built without its `asm` feature, as pure Rust: slower than
with the assembly, and identical in output, since rav1d's assembly and Rust
paths are held to the same results by its test suite.

## What was changed

1. **Formatting.** Every `.rs` file was run through `rustfmt --edition 2024`,
   which this repository's pre-push gate requires of every Rust file it
   publishes. Upstream formats with the 2021 style; the difference is layout
   only (import order, line breaks).
2. **The manifest** (`Cargo.toml`) is this project's: the dependencies and
   features are upstream's, the assembly build-dependencies and the
   `[profile]` and `[workspace]` sections are gone (a workspace member's
   profiles are ignored and warned about), and the library is an `rlib` only.
3. **A safe interface, `safe.rs`, is added** and declared in `lib.rs`
   (`pub mod safe;`). rav1d exports only dav1d's C interface, whose every
   function is `unsafe`; the Rust functions behind it are private to the
   crate. `safe.rs` wraps them for Rust callers. It is the one file here that
   is this project's own.
4. **A worker thread that cannot be started is an error, not a panic.**
   Upstream unwraps `thread::Builder::spawn` in `rav1d_open` (`src/lib.rs`),
   which on a machine out of threads would take down whatever program was
   showing a picture; it now stops the workers already started and returns
   `ENOMEM`. For that to be safe, a worker (`rav1d_worker_task`,
   `src/thread_task.rs`) waits for its context in a loop, and returns if
   told to die first -- which also covers `thread::park` returning
   spuriously, where upstream would have unwrapped a context not yet set.
5. **Warnings from a newer compiler.** rav1d 1.1.0 was built against
   nightly-2025-05-01; today's compiler warns about three lifetimes left
   elided in return types (now `'_`, in `include/dav1d/picture.rs` and
   `src/cdf.rs`), a comparison derived for function pointers that nothing
   calls (`src/wrap_fn_ptr.rs`, allowed on the item), a struct nothing
   constructs (`src/internal.rs`, allowed on the item), and -- in a release
   build only -- a method only the debug build's overlap checks call
   (`Bounds::overlaps`, `src/disjoint_mut.rs`, now compiled with them, as
   is its test, `test_range_overlap`, so the crate's tests build in release
   too), and in the release build's own test, `test_pointer_write_release`,
   three `unsafe` blocks around `DisjointMut::index`, which this release
   made safe (removed). Its
   clippy also asks for a safety comment on four `unsafe impl Send`/`Sync`
   (`src/internal.rs`) that upstream marks with a TODO to remove once the
   types are thread-safe; those are allowed on the item as upstream wrote
   them, since the TODO is the whole of upstream's argument. This
   project builds with no warnings, and item-level changes keep each one to
   the lines concerned.
6. **A damaged frame is an error, not a panic.** When `rav1d_decode_frame`
   fails, `rav1d_submit_frame` (`src/decode.rs`) cleans up through its
   `on_error`, which unwrapped the frame header to ask whether the frame
   refreshed its entropy context -- but `rav1d_decode_frame_exit` has already
   taken the header by then (and released that context if so), so any AV1
   frame whose tile data failed to decode panicked the decoder. `on_error`
   now skips the context when there is no header. Found by `imagecodec`'s
   test that decodes every byte-damaged copy of its AVIF fixtures
   (`a_damaged_file_decodes_or_is_refused_but_never_panics`); worth reporting
   to rav1d.

Every change is marked in the source with `SlateOS (VENDORED.md, change N)`,
except the formatting and the lifetimes.

## Updating

`vendor.py` does step 1 and the copying: `python vendor.py <new .crate>
<empty directory>`, after updating the checksum it holds. Carry changes 2 to 6
across from this copy (`git diff` of this directory against a fresh run shows
exactly them), drop any the new release makes unnecessary, and update this
file's table.
