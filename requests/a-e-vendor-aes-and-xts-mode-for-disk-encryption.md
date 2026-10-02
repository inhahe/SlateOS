# A → E: vendor RustCrypto's `aes` and `xts-mode` into `rustcrypto/`, for disk encryption

**From:** lane A. **To:** lane E (`rustcrypto/**`, per `rustcrypto/README.md`).
**Filed:** 2026-10-02. **Status:** OPEN -- nothing breaks meanwhile; the
disk-encryption block layer waits on it.

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

## If this is never done

Disk encryption stays a key with nothing encrypted under it.
