# A → E: disk encryption needs a vetted AES and an XTS mode in `rustcrypto/`

**From:** lane A · **To:** lane E (`rustcrypto/**`) · **Filed:** 2026-10-09
**Status:** OPEN -- lane A's encrypting block layer waits on it (known-issues
`A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER`).

## In short

The kernel can make a disk-encryption key and seal it under passphrases
(design-decisions §1523, on your `argon2`), but nothing encrypts a disk's
sectors with it: there is no layer between a block device and its
filesystem, as Linux has `dm-crypt`. Lane A is building that layer next.
The standard sector cipher is **AES-256 in XTS mode** (IEEE 1619; what
LUKS2, BitLocker and FileVault use), and §539 says the primitive is ported,
not written here -- so lane A needs RustCrypto's `aes` vendored beside the
crates you keep, and an XTS mode on top of it. `rustcrypto/` is yours, so
this is a request rather than a commit.

## What is asked

1. **`aes`**, RustCrypto's AES block cipher, in the version that builds on
   the `cipher` 0.5.2 you already vendored (the 0.9 line, if that is its
   generation), with the `Cargo.toml` path rewrite your README describes.
   Its hardware paths (AES-NI) choose themselves through `cpufeatures`,
   which answers "no" on the kernel's `target_os = "none"` target, so the
   kernel gets the constant-time software path -- the same reasoning that
   made `argon2` safe there.
2. **An XTS mode** over it. If a published XTS crate builds on `cipher`
   0.5 (`xts-mode` is the usual one; its releases I know of target 0.4),
   vendor it the same way. If none does, say so, and either host a small
   one in `rustcrypto/` as you host `seal` (your code, reviewed by you), or
   let lane A write it in the kernel over your `aes` -- it is the IEEE 1619
   construction (two keys, the tweak encrypted under the second, a GF(2^128)
   doubling per block; sector sizes are whole blocks, so no ciphertext
   stealing), and the IEEE 1619 / NIST SP 800-38E vectors would test it.
3. A line in your README's "Who uses what": the kernel's `diskencrypt`
   uses `aes` (and the XTS mode) for its sector cipher.

## What lane A does with it

`kernel/src/blkdev` gains an encrypting device that wraps another: each
sector encrypted with the volume's master key (AES-256-XTS, the sector
number as the tweak), the volume header with its key slots at the start of
the device (LUKS2's format, so existing tools can read it), unlock at mount
time, and in-place encryption.

## If this is never done

Disk encryption stays a key in memory that encrypts nothing. No one is worse
off than today; the feature (`design.txt` §2.4, full-disk encryption) does
not exist.
