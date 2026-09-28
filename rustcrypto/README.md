# rustcrypto/ -- vetted cryptography, vendored

**Owner:** lane E (`scripts/which-lane.py` reserves `rustcrypto/**`).

**In short:** SlateOS does not write its own cryptographic primitives.
`design-decisions.md` §539 (the operator's answer to C-Q5) settled that the
cipher, the password hash and the other primitives are *ported from
implementations other people have spent years attacking*, and only the file
formats and plumbing around them are this project's. This directory is that
port: the RustCrypto crates for authenticated encryption
(XChaCha20-Poly1305) and password hashing (Argon2id), exactly as they were
published to crates.io, plus [`seal`](seal/) -- the one small crate of our own
here, which is the plain API every caller in the tree uses.

Why a port rather than our own code: a cryptographic primitive can compute the
right answer and still give its secret away through *how long it took*. No
test this project writes can see that; the authors of these crates design
against it (the ciphers are constant-time by construction, the comparisons go
through `ctutils`/`cmov`).

## Who uses what

| Caller | Uses | For |
|---|---|---|
| `apps/credmanager` (lane E) | `seal` | the password vault on disk, and its encrypted backup (C-Q25 / §1417) |
| `gui/credentials` (lane C) | `seal` | the system keyring's vault (moving over from its SHA-256 keystream) |
| `kernel` diskencrypt (lane A) | `argon2` | the disk key's password derivation (§978, A-Q21) |

Use `seal` unless there is a reason not to: it is the one place the choice of
cipher, nonce size and Argon2 variant is made, so two callers cannot drift into
two formats. The kernel uses `argon2` directly because it needs no cipher.

## What is here

Every crate is the `.crate` archive crates.io serves, unpacked (`cargo vendor`,
2026-09-27). The checksum is crates.io's SHA-256 of that archive, as
`Cargo.lock` records it; the revision is the upstream commit the archive was
cut from, from its own `.cargo_vcs_info.json`.

| Crate | Version | Role | Upstream repository and revision | crates.io SHA-256 |
|---|---|---|---|---|
| [`aead`](aead/) | 0.6.1 | the AEAD traits; its `dev` module reads the Wycheproof vectors | https://github.com/RustCrypto/traits `3d2d90045bffc402af4edb6a5a4dbb1e217329d1` | `1973cfbc1a2daf9cf550e74e1f088c28e7f7d8c1e1418fb6c9dc5184b7e84c99` |
| [`argon2`](argon2/) | 0.6.0 | the Argon2 password hash (Argon2id is the one used) | https://github.com/RustCrypto/password-hashes `b1e0ad6fe229b1ba74e4696c7359ab45d7e931f0` | `134c52ddac6d63c576bef8168db10c83c49c26444ecbc68060fef078925a901c` |
| [`base64ct`](base64ct/) | 1.8.3 | constant-time Base64, which `argon2` depends on | https://github.com/RustCrypto/formats `9adf88fe3e6e0fb9f8cf20b54747aff67a3eca6e` | `2af50177e190e07a26ab74f8b1efbfe2ef87da2116221318cb1c2e82baf7de06` |
| [`blake2`](blake2/) | 0.11.0 | BLAKE2b, which Argon2 is built on | https://github.com/RustCrypto/hashes `fa3084083d946ac12436567d5a59c0935d5db1fa` | `5b5d4d889834ee8ecfc0f8426ad30faf7cdcb10f741a8e6d7224d95325479f6f` |
| [`blobby`](blobby/) | 0.4.0 | the binary test-vector format -- tests only | https://github.com/RustCrypto/utils `c210dd6c674fdb039e3d19ad4bec818be1faeafe` | `89af0b093cc13baa4e51e64e65ec2422f7e73aea0e612e5ad3872986671622f1` |
| [`block-buffer`](block-buffer/) | 0.12.1 | block buffering for `digest` | https://github.com/RustCrypto/utils `cbd0963c685df025c42bed31f78c50d1bada3805` | `d2f6c7dbe95a6ed67ad9f18e57daf93a2f034c524b99fd2b76d18fdfeb6660aa` |
| [`cfg-if`](cfg-if/) | 1.0.5 | compile-time backend selection | https://github.com/rust-lang/cfg-if `8b0a2b0fc1fa3b3741b9e72895f390cf499fe36f` | `4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600` |
| [`chacha20`](chacha20/) | 0.10.2 | the ChaCha20 / XChaCha20 stream cipher | https://github.com/RustCrypto/stream-ciphers `6b236b758a0279f64d777797514813b2cb572c8b` | `65c35e4b699c7e15ccbe7ee35c005e4fc0a278d22238a2857e6ce2dadeda1b06` |
| [`chacha20poly1305`](chacha20poly1305/) | 0.11.0 | the AEAD: ChaCha20-Poly1305 and XChaCha20-Poly1305 | https://github.com/RustCrypto/AEADs `e37a978ccf0992d9053fbc039470d6527108e393` | `9b89e1c441e926b9c82a8d023f6e1b7ae0adcfaa7d621814e4d60789bac751cb` |
| [`cipher`](cipher/) | 0.5.2 | the stream-cipher traits | https://github.com/RustCrypto/traits `f836f71fadb3b975b577a293d57851d044f0a44b` | `e8cf2a2c93cd704877c0858356ed03480ff301ee950b43f1cbe4573b088bfa6c` |
| [`cmov`](cmov/) | 0.5.4 | constant-time conditional moves, under `ctutils` | https://github.com/RustCrypto/utils `5c7e4f9bb31af81bf766360e836b6d633b84dbff` | `0c9ea0ac24bc397ab3c98583a3c9ba74fa56b09a4449bbe172b9b1ddb016027a` |
| [`cpufeatures`](cpufeatures/) | 0.3.1 | run-time CPU feature detection (the AVX2 / SSE2 paths) | https://github.com/RustCrypto/utils `e3ac92bfd33051e146025baa2d0ce79891b7fc73` | `5ca28b0ae3115b884660db4118d803791fd6756b6e88f39c0f3f7859060d7566` |
| [`crypto-common`](crypto-common/) | 0.2.2 | traits shared by the above | https://github.com/RustCrypto/traits `93dee26c6bde3741a197f1c5f6b7baac277705f3` | `ce6e4c961d6cd6c9a86db418387425e8bdeaf05b3c8bc1411e6dca4c252f1453` |
| [`ctutils`](ctutils/) | 0.4.2 | constant-time comparison and selection | https://github.com/RustCrypto/utils `53f7fc3fa806e6d4e9650675e7b97d3621cff340` | `7d5515a3834141de9eafb9717ad39eea8247b5674e6066c404e8c4b365d2a29e` |
| [`digest`](digest/) | 0.11.3 | the hash traits | https://github.com/RustCrypto/traits `2fb9ed8922e244117040bb037a7d141a6a2b8228` | `f1dd6dbb5841937940781866fa1281a1ff7bd3bf827091440879f9994983d5c2` |
| [`hex-literal`](hex-literal/) | 1.1.0 | `hex!` literals -- tests only | https://github.com/RustCrypto/utils `5c88fe4aa935251bb8285b9e8f8c7433c464e833` | `e712f64ec3850b98572bffac52e2c6f282b29fe6c5fa6d42334b30be438d95c1` |
| [`hybrid-array`](hybrid-array/) | 0.4.15 | fixed-size arrays sized by `typenum` | https://github.com/RustCrypto/hybrid-array `09310c55b75e2cd408951c7e2e86dba16c63fc68` | `27f864f10dfb56725ce5ce5472bc52252c8f93a4ab86327122cebf62c5f59a17` |
| [`inout`](inout/) | 0.2.2 | in-place / out-of-place buffers for the cipher traits | https://github.com/RustCrypto/utils `2d5eac49c253b19a064b33fc1f9ca7839732cb30` | `4250ce6452e92010fdf7268ccc5d14faa80bb12fc741938534c58f16804e03c7` |
| [`password-hash`](password-hash/) | 0.6.1 | PHC password-hash strings -- optional in `argon2`, not enabled | https://github.com/RustCrypto/traits `d1954d88cbde6b6ee839d3a2f78e5b03fd4eaa1d` | `aab41826031698d6ffcd9cff78ef56ef998e39dc7e5067cdfebe373842d4723b` |
| [`phc`](phc/) | 0.6.1 | PHC string format under `password-hash` -- not enabled | https://github.com/RustCrypto/formats `02a43929d75c8f7a515dcc240648c26d2fe4ee5f` | `44dc769b75f93afdddd8c7fa12d685292ddeff1e66f7f0f3a234cf1818afe892` |
| [`poly1305`](poly1305/) | 0.9.1 | the Poly1305 one-time authenticator | https://github.com/RustCrypto/universal-hashes `4f5691919d96ec0089ecca4492be7f1848c78fdf` | `6e2d0073b297041425c7c3df6eb4792d598a15323fe63346852b092eca02904c` |
| [`typenum`](typenum/) | 1.20.1 | type-level numbers for `hybrid-array` | https://github.com/paholg/typenum `0db9a0f731981f29266b63586c29fa07e4477b1a` | `b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20` |
| [`universal-hash`](universal-hash/) | 0.6.1 | the universal-hash traits | https://github.com/RustCrypto/traits `82279a5a9ff2af5f10194b9147fe60050cda1851` | `f4987bdc12753382e0bec4a65c50738ffaabc998b9cdd1f952fb5f39b0048a96` |

**The one change to the published sources** is in each `Cargo.toml` -- the
normalised manifest cargo generated when the crate was published.
(`Cargo.toml.orig`, upstream's own manifest, is untouched.)

- A dependency on another crate in this directory gains `path = "../<name>"`
  beside its version requirement, so it resolves here and never over the
  network. The version requirement stays, so a copy of the wrong version is
  refused rather than used.
- `cpufeatures` loses its four `libc` dependencies. They exist only for
  aarch64 (Android, Linux, Apple) and loongarch64 Linux; SlateOS is x86_64,
  and vendoring the 5 MB `libc` crate for code no build compiles was not worth
  it. Those targets would not build from this copy.

Nothing under `src/` has been edited. `.cargo-checksum.json` in each directory
is `cargo vendor`'s record of every file's hash as published; every file but
`Cargo.toml` still matches it, and that is how to check that nothing else
has drifted.

## Building

- These crates are **not workspace members** (the root `Cargo.toml` excludes
  `rustcrypto/`); they build as path dependencies of whatever uses them, with
  their own lint settings rather than the workspace's. `seal` is a member.
- **`no_std` + `alloc` throughout.** Use `default-features = false` and ask
  for `alloc`; nothing here needs `std`, and `seal` builds for
  `x86_64-unknown-none`.
- **The vector instructions.** On x86_64, `chacha20`, `poly1305` and `argon2`
  choose an AVX2 (or SSE2) path at run time through `cpufeatures`. In a
  program that is right. **For `x86_64-unknown-none` (the kernel's target)
  two flags are required**, not optional:

  ```text
  --cfg poly1305_backend="soft"  --cfg chacha20_backend="soft"
  ```

  Without the first, LLVM aborts compiling `poly1305`'s AVX2 backend for a
  soft-float target ("Do not know how to split the result of this
  operator!"); with both, `seal` builds for `x86_64-unknown-none` (checked
  2026-09-27 with `cargo build -p seal --target x86_64-unknown-none --config
  'target.x86_64-unknown-none.rustflags=[...]'`). **`argon2` has no such
  switch**: it compiles, and at run time it takes its AVX2 path whenever the
  CPU and `XCR0` allow -- so kernel code that calls it must either save the
  vector registers around the call or be sure `XCR0` does not enable AVX
  there. That is the kernel's decision to make, and a reason to derive a disk
  key in a program rather than in the kernel if it can be.

## Tests

The vendored crates' own suites cannot run in this workspace. Their published
vectors run in [`seal/tests/vectors.rs`](seal/tests/vectors.rs) instead,
against these exact copies: Project Wycheproof's ChaCha20-Poly1305 and
XChaCha20-Poly1305 files (upstream's `.blb` data, read in place), RFC 8439
§2.4.2, §2.5.2 and §2.8.2, draft-irtf-cfrg-xchacha-03 §A.3.1, RFC 9106 §5.3,
the Argon2 reference implementation's Argon2id vectors, and RFC 7693
Appendix A.

## Updating

Re-run `cargo vendor --versioned-dirs` on a scratch crate that depends on
`chacha20poly1305` and `argon2` (and on `aead` with its `dev` feature, and on
`hex-literal`, for the test readers), replace the directories, re-apply the
two `Cargo.toml` changes above, update the table, and run `seal`'s tests.
