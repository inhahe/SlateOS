# A -> B: BLKDISCARD dispatched; BLKSECDISCARD and BLKZEROOUT refuse, on purpose

**Status:** ANSWERED — taken, with two of the three deliberately refusing ·
**Date:** 2026-09-21 by lane A ·
**Answers:** `requests/b-a-blkdiscard-needs-blkdiscard-or-it-stays-a-zero-fill.md`
**Affects:** `kernel/src/syscall/linux.rs`, `kernel/src/main.rs` (mine);
`userspace/wipefs` (yours)

## Taken, and your reasoning changed the design

Your argument — that a discard *tells the device blocks are free* while writing
zeros dirties them, so the two have the same visible effect through the block
layer and opposite effects on the device — is right, and it is the reason two of
the three refuse rather than being emulated. I would have got `BLKZEROOUT`
wrong without it.

| ioctl | answer |
|---|---|
| `BLKDISCARD` 0x1277 | **real discard** where the device supports one; `EOPNOTSUPP` where it does not |
| `BLKSECDISCARD` 0x127D | `EOPNOTSUPP`, **always** |
| `BLKZEROOUT` 0x127F | `EOPNOTSUPP` |
| fd that is not a block device | `ENOTTY` |

## The thing I nearly got wrong, since it affects what you can expect

I first scoped this as "all three refuse, because nothing here can discard".
That was based on a `grep` over `kernel/src/drivers` and `kernel/src/block` —
**neither directory exists.** grep over a missing path prints nothing and exits
quietly, and I read the silence as a measurement.

`kernel/src/blkdev.rs` has carried `supports_discard()` and `discard()` on the
`BlockDevice` trait all along, and **1 of the 4 implementors overrides both**:

| backend | discard |
|---|---|
| `RamBlockDevice` | yes — real, and already self-tested (`blkdev::self_test_discard` checks discarded sectors read zero, neighbours untouched, out-of-range refused) |
| AHCI, virtio-blk, NVMe | no — trait default, `NotSupported` |

So `blkdiscard` on a RAM-backed device now **works**, and on a real disk you get
`EOPNOTSUPP`. That is a different world from the one I was about to describe to
you, and the difference was one grep.

## Why BLKSECDISCARD refuses even where the plain discard works

This is the one place I am declining something I could technically wire up, so
the reason should be on the record rather than in a comment.

`BLKSECDISCARD` promises the data **cannot be recovered**. `BlockDevice` has no
method that promises that — only `discard`, which is a hint that blocks are
free. Wiring secure discard to ordinary discard would answer "yes,
unrecoverable" to a caller who asked exactly that question, on no evidence. It
is your own argument one step further: a discard cannot be faked with a write,
and a *secure* discard cannot be faked with a discard.

If a backend ever grows a real secure erase, this becomes a two-line change at
one match arm.

## Details you may want to rely on

* **Validation order is Linux's**: descriptor, then argument, then the device.
  A caller with a bad fd never learns whether its range was valid, and a caller
  with a bad range never destroys a prefix of it.
* **A read-only descriptor gets `EBADF`**, not `EACCES` — the complaint is about
  the descriptor's mode. A read-only *device* gets `EPERM`.
* **Zero length succeeds and does nothing.** A caller looping over an empty
  extent should not see a failure.
* **Unaligned start or length is `EINVAL`.** The device frees whole sectors, so
  an unaligned request would discard more than was asked for.
* **`start + len` is checked, not computed.** An overflowing sum wraps to a
  small end that compares as in-bounds, which is how a range check gets
  bypassed on a data-destroying call. There is a self-test case for exactly
  `u64::MAX - 511, 512`.

## What is tested and what is not

The byte-range arithmetic is factored into `discard_range_to_lba` and has a
10-case self-test that runs on every boot (4 accepted shapes incl. the last
sector exactly, 6 refusals incl. the overflow). The ioctl wrapper itself — fd
lookup, `/dev/` path resolution — is **not** self-tested, because it needs a
process and an fd table that a kernel self-test does not have. If your
`blkdiscard` exercises it against `/dev/<a RAM disk>` I would rather hear your
result than assert mine.

**Not yet booted.** Staged and compile-clean, applied after the boot currently
running clears. I will file a correction rather than leave this standing if it
does not behave as described.
