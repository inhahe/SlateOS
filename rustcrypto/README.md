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
| `kernel` diskencrypt (lane A) | `seal` | each key slot: the disk's master key sealed under a passphrase's Argon2id key (§978, A-Q21, §1523) |
| `kernel` diskencrypt (lane A) | `aes`, `xts-mode` | the sector cipher: AES-256 in XTS mode, the sector number as the tweak (`aes-xts-plain64`, as LUKS2 and dm-crypt) -- `requests/a-e-vendor-aes-and-xts-mode-for-disk-encryption.md` |
| `userspace/sshwire` (lane B) | `x25519-dalek` | `ssh`'s and `sshd`'s key exchange, `curve25519-sha256` (RFC 8731): X25519 (RFC 7748) in place of a Diffie-Hellman whose time gave its secret away -- `requests/b-e-vendor-x25519-for-ssh.md` |

Use `seal` unless there is a reason not to: it is the one place the choice of
cipher, nonce size and Argon2 variant is made, so two callers cannot drift into
two formats. The kernel uses it too: it once took `argon2` alone, "because it
needs no cipher", and then each key slot came to seal the volume's master key
under the passphrase's key (`requests/a-e-the-kernel-links-seal-not-argon2-directly.md`).

**The kernel's build.** `seal` and what it pulls in run in the kernel's
`x86_64-unknown-none` target as they are: `cpufeatures` answers "no" on
`target_os = "none"`, so `chacha20`, `poly1305` and `argon2` take their
portable paths and the soft-float kernel never touches a vector register.
Keep that true when updating any of them -- a backend that assumed SSE2, or
chose AVX2 some other way, would corrupt user registers in the kernel. The
kernel calls `derive_key`, `encrypt` and `decrypt`, with `KdfParams` of
19 MiB x 2 x 1 for passphrases and 8 KiB x 1 x 1 for 256-bit recovery keys.

## What is here

Every crate is the `.crate` archive crates.io serves, unpacked (`cargo vendor`,
2026-09-27). The checksum is crates.io's SHA-256 of that archive, as
`Cargo.lock` records it; the revision is the upstream commit the archive was
cut from, from its own `.cargo_vcs_info.json`.

| Crate | Version | Role | Upstream repository and revision | crates.io SHA-256 |
|---|---|---|---|---|
| [`aes`](aes/) | 0.9.3 | the AES block cipher (FIPS-197); constant-time software path, AES-NI chosen at run time (2026-10-09) | https://github.com/RustCrypto/block-ciphers `c1534361e7549e29a16c3505f45a04c92d26b62a` | `35f0f96ce78e38c3dc6d8948aa8163d06385be74000f3c7a95bf1eef35d3ea32` |
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
| [`cpubits`](cpubits/) | 0.1.1 | the target's word size as a cfg, which `aes` 0.9 selects its software backend by (2026-10-09) | https://github.com/RustCrypto/utils `bff92a8c33629ae8e9d1407f3b7fea604992dd0f` | `15b85f9c39137c3a891689859392b1bd49812121d0d61c9caf00d46ed5ce06ae` |
| [`cpufeatures`](cpufeatures/) | 0.3.1 | run-time CPU feature detection (the AVX2 / SSE2 paths) | https://github.com/RustCrypto/utils `e3ac92bfd33051e146025baa2d0ce79891b7fc73` | `5ca28b0ae3115b884660db4118d803791fd6756b6e88f39c0f3f7859060d7566` |
| [`cpufeatures-0.2`](cpufeatures-0.2/) | 0.2.17 | the same, at the version `curve25519-dalek` 4 asks for: its AVX2 backend's choice, on x86_64 (2026-10-10) | https://github.com/RustCrypto/utils `9d92d5e95ab4c07c5d8bfd024bf2a17e96d20feb` | `59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280` |
| [`curve25519-dalek`](curve25519-dalek/) | 4.1.3 | Curve25519 arithmetic, under X25519; constant-time throughout (2026-10-10) | https://github.com/dalek-cryptography/curve25519-dalek `5312a0311ec40df95be953eacfa8a11b9a34bc54` | `97fb8b7c4503de7d6ae7b42ab72a5a59857b4c937ec27a3d4539dba95b5ab2be` |
| [`curve25519-dalek-derive`](curve25519-dalek-derive/) | 0.1.1 | the procedural macro that writes `curve25519-dalek`'s AVX2 backend's per-feature functions -- x86_64 only (2026-10-10) | https://github.com/dalek-cryptography/curve25519-dalek `72761ca6b4772af985f969db53faf7accbad9b36` | `f46882e17999c6cc590af592290432be3bce0428cb0d5f8b6715e4dc7b383eb3` |
| [`crypto-common`](crypto-common/) | 0.2.2 | traits shared by the above | https://github.com/RustCrypto/traits `93dee26c6bde3741a197f1c5f6b7baac277705f3` | `ce6e4c961d6cd6c9a86db418387425e8bdeaf05b3c8bc1411e6dca4c252f1453` |
| [`ctutils`](ctutils/) | 0.4.2 | constant-time comparison and selection | https://github.com/RustCrypto/utils `53f7fc3fa806e6d4e9650675e7b97d3621cff340` | `7d5515a3834141de9eafb9717ad39eea8247b5674e6066c404e8c4b365d2a29e` |
| [`digest`](digest/) | 0.11.3 | the hash traits | https://github.com/RustCrypto/traits `2fb9ed8922e244117040bb037a7d141a6a2b8228` | `f1dd6dbb5841937940781866fa1281a1ff7bd3bf827091440879f9994983d5c2` |
| [`hex-literal`](hex-literal/) | 1.1.0 | `hex!` literals -- tests only | https://github.com/RustCrypto/utils `5c88fe4aa935251bb8285b9e8f8c7433c464e833` | `e712f64ec3850b98572bffac52e2c6f282b29fe6c5fa6d42334b30be438d95c1` |
| [`hybrid-array`](hybrid-array/) | 0.4.15 | fixed-size arrays sized by `typenum` | https://github.com/RustCrypto/hybrid-array `09310c55b75e2cd408951c7e2e86dba16c63fc68` | `27f864f10dfb56725ce5ce5472bc52252c8f93a4ab86327122cebf62c5f59a17` |
| [`inout`](inout/) | 0.2.2 | in-place / out-of-place buffers for the cipher traits | https://github.com/RustCrypto/utils `2d5eac49c253b19a064b33fc1f9ca7839732cb30` | `4250ce6452e92010fdf7268ccc5d14faa80bb12fc741938534c58f16804e03c7` |
| [`password-hash`](password-hash/) | 0.6.1 | PHC password-hash strings -- optional in `argon2`, not enabled | https://github.com/RustCrypto/traits `d1954d88cbde6b6ee839d3a2f78e5b03fd4eaa1d` | `aab41826031698d6ffcd9cff78ef56ef998e39dc7e5067cdfebe373842d4723b` |
| [`phc`](phc/) | 0.6.1 | PHC string format under `password-hash` -- not enabled | https://github.com/RustCrypto/formats `02a43929d75c8f7a515dcc240648c26d2fe4ee5f` | `44dc769b75f93afdddd8c7fa12d685292ddeff1e66f7f0f3a234cf1818afe892` |
| [`poly1305`](poly1305/) | 0.9.1 | the Poly1305 one-time authenticator | https://github.com/RustCrypto/universal-hashes `4f5691919d96ec0089ecca4492be7f1848c78fdf` | `6e2d0073b297041425c7c3df6eb4792d598a15323fe63346852b092eca02904c` |
| [`rand_core`](rand_core/) | 0.6.4 | the random-generator traits `x25519-dalek` names; nothing here draws from it -- the caller passes the secret's bytes (2026-10-10) | https://github.com/rust-random/rand `89a1336b934c68ddce548127c6f8afd910b35a18` | `ec0be4795e2f6a28069bec0b5ff3e2ac9bafc99e6a9a7dc3547996c5c816922c` |
| [`subtle`](subtle/) | 2.6.1 | constant-time choices and comparisons, under `curve25519-dalek` (2026-10-10) | https://github.com/dalek-cryptography/subtle `5457b5448b021d1da101ababbb854e6657233943` | `13c2bddecc57b384dee18652358fb23172facb8a2c51ccc10d74c157bdea3292` |
| [`typenum`](typenum/) | 1.20.1 | type-level numbers for `hybrid-array` | https://github.com/paholg/typenum `0db9a0f731981f29266b63586c29fa07e4477b1a` | `b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20` |
| [`universal-hash`](universal-hash/) | 0.6.1 | the universal-hash traits | https://github.com/RustCrypto/traits `82279a5a9ff2af5f10194b9147fe60050cda1851` | `f4987bdc12753382e0bec4a65c50738ffaabc998b9cdd1f952fb5f39b0048a96` |
| [`x25519-dalek`](x25519-dalek/) | 2.0.1 | X25519 Diffie-Hellman (RFC 7748), for ssh's `curve25519-sha256`; features `static_secrets`, `zeroize`, `precomputed-tables` (2026-10-10) | https://github.com/dalek-cryptography/curve25519-dalek `4ac84dd0668b1d2e51654fcdffe4ae6a687bef00` | `c7e468321c81fb07fa7f4c636c3972b9100f0346e5b6a9f2bd0603a52f7ed277` |
| [`xts-mode`](xts-mode/) | 0.6.0 | XTS (IEEE 1619) over a 128-bit block cipher, for disk sectors (2026-10-09) | https://github.com/pheki/xts-mode `ce5a8efae75b4bdfe43abf0c6e2b008771896668` | `e2acb658219ff8afdcedc1ea506709b2b32812719eb6ac36f47ae43f50404157` |
| [`zeroize`](zeroize/) | 1.9.1 | wiping secrets from memory when they are dropped, which `x25519-dalek`'s secrets do (2026-10-10) | https://github.com/RustCrypto/utils `34a6ebcb9b920ec7ae5809e470fce94fe7c0cb55` | `e13084392c5e4bc371903e2935a5eaeed24905a7511356b883835e18a78f6879` |
| [`zeroize_derive`](zeroize_derive/) | 1.5.0 | `#[derive(Zeroize)]`, the procedural macro `zeroize` uses for that (2026-10-10) | https://github.com/RustCrypto/utils `1ea42bb68f560a2909c856b20aa3c62901f6305c` | `3c50655cbb0fe3fc43170059e702f1ce5e19b84cec58dc87b037a09935c2f328` |

`aes`, `cpubits` and `xts-mode` were vendored on 2026-10-09 the same way, by
`cargo vendor --versioned-dirs` on a scratch crate depending on `aes =
"=0.9.3"` and `xts-mode = "=0.6.0"`; every other crate that pulled in was
already here at the same version.

`x25519-dalek` and what it needs were vendored on 2026-10-10 the same way, on
a scratch crate depending on `x25519-dalek = "=2.0.1"` with
`default-features = false` and the features `zeroize`, `static_secrets` and
`precomputed-tables` -- `curve25519-dalek`, `curve25519-dalek-derive`,
`subtle`, `zeroize`, `zeroize_derive`, `rand_core` and `cpufeatures` 0.2.17
(beside the 0.3.1 already here, as `cpufeatures-0.2/`); `cfg-if` was already
here.

**What comes from crates.io instead: the compile-time tools.** Everything
compiled *into a program* is vendored here. What runs only while compiling
is not: the two procedural macros above are built with `proc-macro2`,
`quote`, `syn` and `unicode-ident`, and `curve25519-dalek`'s build script with
`rustc_version` and `semver`, all from crates.io as every other procedural
macro and build script in the workspace is, pinned by checksum in
`Cargo.lock`. (The first four were in the workspace's graph already.) They
read source and emit source; the code that ends up in a program is the
macros' own, which is here.

**Two packages named `aes`.** The hand-written `/aes` (lane A's, version
0.1.0, a workspace member) and this one (0.9.3, a dependency of `seal`'s
tests) are both in the workspace's dependency graph. `cargo build`, `test`
and `clippy -p aes` still mean the workspace member (checked 2026-10-09); the
commands that search the whole graph -- `cargo pkgid`, `cargo tree -p`,
`cargo update -p` -- need `aes@0.1.0` or `aes@0.9.3`, and `Cargo.lock` names
each dependent's as `aes 0.1.0` or `aes 0.9.3`. `/aes` is the hand-written
primitive §539 says to retire in favour of this one.

`xts-mode` is not a RustCrypto crate: it is
the XTS implementation the RustCrypto ecosystem uses, by its own author, and
its README says plainly that it has not been independently audited -- which
is why its vectors (below) are NIST's, not its own.

**The one change to the published sources** is in each `Cargo.toml` -- the
normalised manifest cargo generated when the crate was published.
(`Cargo.toml.orig`, upstream's own manifest, is untouched.)

- A dependency on another crate in this directory gains `path = "../<name>"`
  beside its version requirement, so it resolves here and never over the
  network. The version requirement stays, so a copy of the wrong version is
  refused rather than used. A *dev*-dependency on one gains it too
  (`x25519-dalek`'s and `curve25519-dalek`'s `rand_core`, 2026-10-10): cargo
  refuses a manifest whose dependency comes from two places by kind, though
  nothing here builds those crates' own tests.
- `cpufeatures` loses its four `libc` dependencies. They exist only for
  aarch64 (Android, Linux, Apple) and loongarch64 Linux; SlateOS is x86_64,
  and vendoring the 5 MB `libc` crate for code no build compiles was not worth
  it. Those targets would not build from this copy. `cpufeatures-0.2` loses
  the same four, for the same reason (2026-10-10).
- `curve25519-dalek` loses its `fiat-crypto` dependency, which exists only
  under `--cfg curve25519_dalek_backend="fiat"` -- a backend nothing here
  selects -- and would otherwise sit in `Cargo.lock` as a 2.5 MB crate no
  build compiles (2026-10-10). That backend would not build from this copy.
- `curve25519-dalek` and `x25519-dalek` lose their `[[bench]]` targets,
  which need `criterion`, not vendored here (2026-10-10).
- `curve25519-dalek`, `x25519-dalek` and `rand_core` gain a `[lints.rust]`
  table for the lints of compilers newer than they are (2026-10-10). A path
  dependency's warnings show in every build that uses it, and these would
  have printed thirty-three in every build of `sshwire`:
  `curve25519-dalek`'s `unused_unsafe` (an intrinsic inside a
  `#[target_feature]` function needs no `unsafe` block since Rust 1.87) and
  `stable_features` (on nightly, AVX-512 features stable since 1.89), and the
  `cfg`s the other two test without declaring (`feature = "bench"`,
  `doc_cfg`) -- declared, not silenced. The code is as published.
- `base64ct`, `blake2`, `chacha20` and `poly1305` lose their `[[bench]]`
  target (2026-09-27). Each is `#![feature(test)]`, which only a nightly
  compiler accepts, and the published manifests name them explicitly --
  `autobenches = false` does not stop an explicit one. `cargo clippy
  --all-targets`, which the pre-push cfg(unix) gate runs on `base64ct`,
  compiled them on stable and failed with E0554, refusing every push that
  touched a `.rs` file (lane F's finding). The files under `benches/` are
  kept as published; to run one, put its `[[bench]]` back on a nightly
  toolchain. `aes` loses its `[[bench]]` for the same reason (2026-10-09),
  and `xts-mode` its own, which needs `criterion`, not vendored here.

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
- **`aes` in the kernel** needs a third flag, `--cfg aes_backend="soft"`,
  and it is not optional either: without it LLVM aborts compiling `aes`'s
  AES-NI backend for `x86_64-unknown-none` with the same "Do not know how to
  split the result of this operator!" as `poly1305`; with it, AES-256-XTS over
  a sector builds for that target (both checked 2026-10-09, from a scratch
  crate on these copies). The flag selects the constant-time software
  implementation (upstream's `soft` backend, fixsliced -- a bitsliced AES with
  no secret-dependent table lookups) instead of the AES-NI one `cpufeatures`
  would choose at run time; AES-NI uses the XMM registers, which a soft-float
  target does not save. `xts-mode` has no backend of its own: it runs
  whichever `aes` has.
- **`curve25519-dalek`'s backend.** On x86_64 it builds two -- the portable
  64-bit one and an AVX2 one -- and chooses at run time through
  `cpufeatures` 0.2, which says yes to AVX2 only when the CPU has it *and*
  `XCR0` says the system saves its registers. Both are constant-time; X25519
  needs neither for that. It is for programs (`sshwire`); it has not been
  built for the kernel's target and should not be put there without
  `--cfg curve25519_dalek_backend="serial"` and a check like `aes`'s above.

## Tests

The vendored crates' own suites cannot run in this workspace. Their published
vectors run in [`seal/tests/vectors.rs`](seal/tests/vectors.rs) instead,
against these exact copies: Project Wycheproof's ChaCha20-Poly1305 and
XChaCha20-Poly1305 files (upstream's `.blb` data, read in place), RFC 8439
§2.4.2, §2.5.2 and §2.8.2, draft-irtf-cfrg-xchacha-03 §A.3.1, RFC 9106 §5.3,
the Argon2 reference implementation's Argon2id vectors, and RFC 7693
Appendix A.

The disk-encryption primitives' vectors run in
[`seal/tests/disk_vectors.rs`](seal/tests/disk_vectors.rs): FIPS-197 Appendix C
and upstream `aes`'s NESSIE files (read in place) for AES-128/192/256, and NIST
CAVP's XTS-AES-256 vectors for `xts-mode` over `aes`. NIST's file is
[`seal/tests/data/XTSGenAES256.rsp`](seal/tests/data/XTSGenAES256.rsp), kept
byte for byte: the "format tweak value input - data unit seq no" copy from
NIST's `XTSTestVectors.zip` (zip SHA-256
`67bb04b018182f65530596786e7783f817d2e56509bf3b1f066609b8e3e29c36`, file
SHA-256 `8b72c26e9a9405524e4139bba36619fff80e1ef3ef1f317bf36f5e968a133fd1`),
the form whose tweak is the data unit's number, as a disk's sector number is.

X25519's run in [`seal/tests/x25519_vectors.rs`](seal/tests/x25519_vectors.rs):
RFC 7748 §5.2 (the function on its two worked examples, and iterated once and
a thousand times from the base point) and §6.1 (a whole exchange through the
calls `sshwire` makes), and RFC 8731 §3's check that a small-order public
key's all-zero secret is told apart (`SharedSecret::was_contributory`).

## Updating

Re-run `cargo vendor --versioned-dirs` on a scratch crate that depends on
`chacha20poly1305`, `argon2`, `aes`, `xts-mode` and `x25519-dalek` (and on
`aead` with its `dev` feature, and on `hex-literal`, for the test readers),
replace the directories, re-apply the `Cargo.toml` changes above, update the
table, and run `seal`'s tests.
