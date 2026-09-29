# Vendored code under `posix/`

Code other people wrote and maintain, copied here exactly as they publish it,
so the C library can depend on it without the network. Nothing under this
directory is edited in place: a change is a new version from upstream, with
this file updated to say which. The workspace excludes the directory (the root
`Cargo.toml`'s `exclude` list), so the tree's lints and `clippy --workspace`
judge our code and not upstream's, and upstream's dev-dependencies are never
fetched; each crate builds only as a path dependency of `posix`.

| Crate | Version | From | SHA-256 of the `.crate` | Licence | Used for |
|---|---|---|---|---|---|
| `libm` | 0.2.16 | <https://static.crates.io/crates/libm/libm-0.2.16.crate> | `b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981` (the crates.io index's `cksum`) | MIT (`libm/LICENSE.txt`) | every function of `<math.h>` in `posix/src/math.rs` (design-decisions §1132) |

`libm` is rust-lang's port of musl's libm -- the one `compiler-builtins`
builds for every Rust target that has no system libm -- and is tested upstream
against MPFR. The copy here is the published `.crate` unpacked, every file as
it came, `Cargo.toml.orig` included.

## Updating

1. Download the new `.crate` from `https://static.crates.io/crates/<name>/<name>-<version>.crate`,
   and check its SHA-256 against the `cksum` in `https://index.crates.io/`.
2. Replace the directory's contents with the unpacked crate, whole.
3. Update the table above, rebuild the sysroot, and run
   `cargo test -p posix` -- `math::tests::every_answer_is_glibcs_or_within_its_error`
   replays glibc 2.39's answers, so a regression in the new version shows there.
