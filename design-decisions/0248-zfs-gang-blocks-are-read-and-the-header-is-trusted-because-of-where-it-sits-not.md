## §248 — ZFS gang blocks are read, and the header is trusted because of *where* it sits, not because a parent vouched for it

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** When a ZFS pool gets full enough that there is no longer one
unbroken run of free space big enough for the file block being written, ZFS
writes the block in pieces and leaves behind a little index — a *gang header* —
listing where the pieces went. SlateOS's ZFS reader used to refuse any block
stored that way, which is exactly backwards: a pool only starts doing this when
it is nearly full, and a nearly-full pool is the one whose owner most urgently
wants to copy their files off it. It now reassembles them. The part worth
recording is how the index itself is proved genuine, because it cannot be proved
the ordinary way, and the two obvious guesses at how it *is* proved are both
wrong in ways a synthetic test would happily agree with.

**Decision.** `dmu::Reader::read_block` follows a DVA whose gang bit is set into
`read_gang`, which verifies the header, walks its block pointers in order, reads
each piece (recursively, since a piece may itself be a gang block), and
concatenates them. The tree is capped at `MAX_GANG_DEPTH = 6`. A tree whose
pieces do not add up to exactly the parent's `psize` — short *or* long — is
rejected as `InvalidArgument`.

**Why the header is checksummed against its own address.** Every other block in
ZFS is verified against a hash stored in the pointer that names it. A gang header
cannot be: the pointer that would hold its hash is the same pointer whose target
the header describes, so ZFS would have to know the header's hash before it had
finished deciding the header's contents. The way out is that the header carries
its own hash, in a 40-byte `zio_eck_t` trailer, and what is fed into that hash in
place of the checksum field is the header's *address* — vdev, offset, and the
transaction group it was written in. Forging a header therefore means placing it
at precisely the address the parent already names, in the transaction the parent
already records, which is not a forgery so much as a rewrite of the parent.

**Why DVA 0's address specifically.** A pointer may name up to three copies of
the same header (`copies=2` and metadata get more than one). The address hashed
into the trailer is always **copy 0's**, whichever copy is actually being read.
That is not an oversight in OpenZFS — it is what makes the copies byte-identical
to each other, so any of them satisfies the parent and a resilver can substitute
one for another without rewriting anything. A reader that verifies against "the
address I read from" works on a single-copy pool and fails on a healthy
multi-copy one, which is the worst possible failure ordering: it passes the easy
test and breaks on the configuration that was chosen for extra safety. Two
matching details: the offset used is the raw DVA offset, *not* the
label/boot-reserve-adjusted one (the 4 MiB reserve is a property of the device,
not of the block's identity), and the transaction group is the *physical* birth
word falling back to the logical one when it is zero — zero there means "same as
logical", the common case, not "transaction zero".

**Why the parent's checksum is deliberately not checked.** The parent pointer of
a gang block carries a `blk_cksum` field, and it does *not* describe the
assembled bytes — OpenZFS's `zio_checksum_error` substitutes the gang-header
verifier whenever the pointer's gang bit is set, so nothing ever hashes the
concatenation. Coverage is nonetheless complete, by a different route: each piece
is verified against its own pointer, each of those pointers lives inside the
header, the header is verified by its trailer, and the trailer is anchored to the
address the parent names. The chain is unbroken; it just runs through addresses
rather than through one end-to-end hash. This is stated here because a reader
that "helpfully" also checked the parent's field would reject every real gang
block, and the code would look more careful while being strictly wrong. The test
suite pins it the only way that works: `make_gang_blkptr` writes
`0xDEAD_BEEF_DEAD_BEEF` into that field, so every happy-path check fails the
moment anyone starts consulting it.

**Why the header's size is resolved by trying and falling back.** A gang header
used to be 512 bytes on every pool. The `dynamic_gang_header` feature made it one
minimum allocation on its vdev instead — 4096 on a typical `ashift=12` pool. This
driver does not parse `features_for_read`, and does not need to: it does what
OpenZFS itself does, trying the larger size that the DVA's own `asize` admits and
falling back to 512 when that does not verify. The self-checksum is what decides,
so a wrong guess cannot be mistaken for a right one — it fails the trailer magic
or it fails the hash. The alternative, parsing the feature flags, would add a
dependency on a list that grows with every OpenZFS release in order to answer a
question the data already answers.

**Why there is a depth cap.** A header whose child pointer addresses the header
itself verifies at every level — it is a genuine header at a genuine address —
and contributes no bytes, so the length check that catches every other malformed
tree never fires. Nothing but a cap terminates it, and the thing it exhausts is
the kernel stack. Six levels is far past anything real: even the three-pointer
old-style header spans 729 pieces at that depth, which for a 16 MiB block means
pieces of 22 KiB. A pool fragmented worse than that is failing allocations, not
writing deeper trees.

**How it was verified.** Thirteen new checks in the ZFS self-test, run under QEMU
(`[zfs] Self-test passed (144 checks)`, up from 131). They cover reassembly in
header order, a zero physical birth falling back to the logical txg, a 4096-byte
dynamic header, an old 512-byte header inside a 4096-byte allocation, a nested
tree, a ditto copy hashed against copy 0's address, and seven rejections: wrong
address, wrong txg, short tree, long tree, self-referential cycle, corrupt
header, missing trailer magic. The on-disk facts were re-derived from freshly
fetched OpenZFS `zio.c`/`zio.h`/`zio_checksum.c` rather than from memory, because
a synthetic test encodes whatever assumption its author made and therefore cannot
catch a wrong one — self-consistent and wrong is the outcome to fear here.

**Where it lives.** `kernel/src/fs/zfs/dmu.rs`: `read_raw`, `read_copy`,
`read_gang`, `read_gang_header`, and the free function `gang_verifier`. Tests in
`kernel/src/fs/zfs/tests.rs` under the `gang` group. Scope statement in
`kernel/src/fs/zfs/mod.rs`.

**How to reverse.** Make `read_copy` return `NotSupported` when `dva.gang` is
set; nothing else depends on the gang path.

**What would change this.** If a pool is ever found whose gang header is neither
512 nor its vdev's minimum allocation, the two-try resolution becomes a real
feature-flag parse. And if gang *writes* are ever wanted, none of this transfers
— the allocator side is the hard half, and reading is deliberately the only half
built.
