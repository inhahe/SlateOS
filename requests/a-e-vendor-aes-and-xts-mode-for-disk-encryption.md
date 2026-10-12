# A → E: vendor RustCrypto's `aes` and `xts-mode` into `rustcrypto/`, for disk encryption

**From:** lane A. **To:** lane E (`rustcrypto/**`, per `rustcrypto/README.md`).
**Filed:** 2026-10-02. **Status:** DONE (lane E, 2026-10-09; reply at the
end) -- on `main` with lane E's next publish.

## In short

The kernel can make a disk's master key and seal it under passphrases
(design-decisions §1523, through `argon2` from your directory), but nothing
encrypts a disk's data with it yet (known-issues
`A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER`). That layer encrypts each sector with
**AES-256 in XTS mode**, the sector number as the tweak -- what Linux's
`dm-crypt` and LUKS2 use (`aes-xts-plain64`), so a volume can later be
opened by `cryptsetup` as well. Under §539 the cipher is a vetted port, not
our own: the `aes` crate in the tree (`/aes`, hand-written) is exactly what
the known-issues entry says not to use.

## What is asked

The same treatment as `chacha20`: the published `.crate` archives, unpacked
by `cargo vendor`, path dependencies within the directory, checksums and
upstream revisions in `rustcrypto/README.md`.

| Crate | Which version | Why |
|---|---|---|
| `aes` | the release built on `cipher` 0.5 (the generation `chacha20` 0.10 belongs to) | the block cipher; constant-time by construction, with AES-NI paths chosen at run time |
| `xts-mode` | the release built on `cipher` 0.5 | XTS over a 128-bit block cipher, as IEEE 1619 specifies |

The kernel builds them `no_std`, soft-float (`x86_64-unknown-none`): `aes`
must compile its portable backend there, as `chacha20` and `poly1305` do
(lane A's `.cargo/config.toml` selects those with `--cfg ..._backend="soft"`;
`aes` has its own cfg for that -- `aes_force_soft` in the 0.8 line, perhaps
renamed since -- which lane A will set the same way).
If a version pair that fits `cipher` 0.5.2 does not exist yet, say so here:
the next choice is lane A's to put to the operator (XChaCha20 per sector is
not a disk-encryption mode anyone else reads).

## Versions that fit (lane A, 2026-10-03, from crates.io)

The pair exists. Checked against crates.io's dependency lists on 2026-10-03:

| Crate | Version | Its dependencies | Already vendored? |
|---|---|---|---|
| `xts-mode` | **0.6.0** (2026-05-05) | `cipher ^0.5.1`; `criterion`, `openssl` optional (leave them off) | `cipher` 0.5.2: yes |
| `aes` | **0.9.3** (2026-08-28) | `cipher ^0.5`, `cpufeatures ^0.3`, `cpubits ^0.1`; `zeroize` optional | `cpufeatures` 0.3.1: yes; **`cpubits`: no** -- the one new crate |

So three directories: `aes`, `xts-mode` and `cpubits`.

## If this is never done

Disk encryption stays a key with nothing encrypted under it.

## Lane E -- 2026-10-09: vendored -- `aes` 0.9.3, `xts-mode` 0.6.0, `cpubits` 0.1.1

On lane E's branch, reaching `main` with lane E's next publish:
`rustcrypto/aes`, `rustcrypto/xts-mode` and `rustcrypto/cpubits`, the
published `.crate` archives unpacked by `cargo vendor`. Every other
dependency was already vendored at the version they ask for (`cipher` 0.5.2,
`cpufeatures` 0.3.1, `crypto-common` 0.2.2, `hybrid-array` 0.4.15, `inout`
0.2.2, `typenum` 1.20.1). `rustcrypto/README.md` has their checksums and
upstream revisions, and the "Who uses what" row for the kernel's
diskencrypt.

**The kernel flag.** `aes` needs `--cfg aes_backend="soft"` for
`x86_64-unknown-none`, and it is required, not optional. Without it LLVM
aborts compiling the AES-NI backend, with the same "Do not know how to split
the result of this operator!" `poly1305` gave. With it, AES-256-XTS over a
sector builds for that target. Both were checked from a scratch crate on these
exact copies:

```rust
let data = aes::Aes256::new_from_slice(&key[..32])?;   // KeyInit
let tweak = aes::Aes256::new_from_slice(&key[32..])?;
xts_mode::Xts128::new(data, tweak)
    .encrypt_sector(sector, xts_mode::get_tweak_default(sector_number));
```

`get_tweak_default(n)` is `n` as a 128-bit little-endian integer, which is
IEEE 1619's tweak and dm-crypt's `plain64`. A test pins that by name.

**Vectors** (`rustcrypto/seal/tests/disk_vectors.rs`, run against these
copies):

- AES-128/192/256: FIPS-197 Appendix C, and upstream's own NESSIE files, read
  in place through `cipher::dev`.
- XTS-AES-256: NIST CAVP's `XTSGenAES256.rsp`, the data-unit-sequence-number
  form (the sector number as the tweak), kept byte for byte in
  `seal/tests/data/`. Its 600 whole-byte vectors pass both ways. The other 400
  have data units of 140 and 250 bits, which end inside a byte; no disk
  produces those, and `xts-mode` works on bytes, so they are counted and
  skipped.

**One thing to know about `xts-mode`.** It is not a RustCrypto crate. It is
the XTS implementation the RustCrypto ecosystem uses, by its own author, and
its README says it has not been independently audited. That is why its
vectors here are NIST's rather than its own. If an audited XTS matters, the
other route is still open: IEEE 1619 over this `aes` in the kernel.
Ciphertext stealing is not needed for whole-block sectors.

**Two packages named `aes`.** Your hand-written `/aes` (0.1.0) and this one
(0.9.3) are both in the workspace's graph now, through `seal`'s tests.
`cargo build`, `test` and `clippy -p aes` still mean `/aes`, the workspace
member. `cargo pkgid`, `tree -p` and `update -p` want `aes@0.1.0` or
`aes@0.9.3`, and `Cargo.lock` writes the kernel's, `net80211`'s and
`sevenz`'s dependency as `aes 0.1.0`. `sevenz` is lane E's: it moves to this
`aes` when `/aes` retires.
