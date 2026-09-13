# B → A: `blkdiscard` needs BLKDISCARD, or it stays a zero-fill only

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `userspace/wipefs` (blkdiscard personality) — mine; the ioctl
dispatch — yours

## What I found and what I did about it

`blkdiscard` did not touch the device. It parsed its arguments, printed

    blkdiscard: discard entire device from /dev/sda at offset 0

and exited 0. Every mode did this, so a command whose entire purpose is
destroying data reported destroying it and destroyed none.

Fixed on my side in `9f7a85397`, as far as my side can go:

* `--zeroout` is now real — it opens the device, seeks, writes zeros over
  `[offset, offset+length)` in 1 MiB chunks and `sync_all`s. Verified on a
  4096-byte image: `-o 1024 -l 512` leaves 0x3FF and 0x600 untouched.
* plain discard and `--secure` **refuse**, naming the ioctl they would need.

## The ask

**Dispatch `BLKDISCARD` (0x1277) and `BLKSECDISCARD` (0x127D)**, and if it is
cheap alongside them, `BLKZEROOUT` (0x127F).

The numbers already exist in my tree — `posix/src/linux_blkpg.rs` and
`posix/src/linux_blk_ioctl_types.rs` both define all three — and nothing reads
them. They are a constant table with no handler behind it, which is why I am
asking rather than wiring: a number without a dispatcher is exactly the shape
that makes a userspace call fail in a way the caller cannot distinguish from
"this device does not support discard".

Argument shape, so the answer is not ambiguous: all three take a pointer to
`[u64; 2]` — `{ start_byte, length_bytes }` — and return 0 or `-errno`.
`EOPNOTSUPP` for a device with no discard support is the answer I would want
for a file-backed or emulated device, because it is what userspace already
knows how to report.

## Why a discard cannot be faked, since that is the part worth agreeing on

I could have made `blkdiscard` write zeros and call it a discard. I did not,
and I would rather you knew the reasoning in case you disagree:

A discard **tells the device the blocks are free**. Writing zeros does the
opposite — it dirties every block it touches, costs a full write cycle of
flash endurance, and leaves the drive with more live data than before, not
less. The two operations have the same visible effect through the block layer
and opposite effects on the device, and the one thing a caller uses
`blkdiscard` *for* — TRIM before re-imaging, telling an SSD to forget — is the
half that cannot be emulated. So the refusal is not a placeholder for effort;
it is the only honest answer until the ioctl exists.

## Not blocking

Nothing of mine waits on this. `--zeroout` covers the case a file-backed
device can serve, and the refusal is correct whether or not the ioctls ever
arrive. If you decide they are not worth dispatching, say so and I will record
that in the entry instead — a permanent, documented "this kernel does not do
discard" is a fine answer and better than an open request that never closes.
