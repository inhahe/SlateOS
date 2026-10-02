### A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the second half of design-decisions §978's
`diskencrypt` item; the first (real key derivation, §1523) is done.

**In short:** the kernel can now make a disk-encryption key, seal it under
passphrases and a recovery key, and get it back only with one of them -- but
nothing encrypts or decrypts a disk's data with it. There is no encrypting
layer between a block device and the filesystem mounted on it (Linux's
`dm-crypt`), so an "encrypted volume" is a key in memory and nothing more,
and the volumes and their slots are forgotten at reboot (their header has
nowhere to live but the disk, and nothing writes it there).

**Where:** `kernel/src/fs/diskencrypt.rs` (the keys); the block layer
(`kernel/src/blkdev*`) and the mount path, which do not call it.

**Proper fix:** a block device that wraps another and encrypts each sector
with the master key (AES-256-XTS, the sector number as the tweak -- a vetted
AES, per §539, not the hand-written `aes` crate); a volume header at the start
of the device holding the slots (a LUKS2-like format, or LUKS2 itself so
existing tools can read it); unlock at mount time, which opens the header,
asks for the passphrase and hands the mount the decrypting device; and
in-place encryption driving `update_progress`.
