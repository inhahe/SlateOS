## §247 — One leg of a ZFS mirror is a whole pool, and the vdev-type gate is a whitelist so that the two mirror-shaped layouts that are *not* whole pools stay out

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** A "mirror" is two or more disks kept as identical copies of each
other, so that either one alone still has everything. SlateOS's ZFS reader
refused them: hand it one disk out of a mirrored pair and it said the layout was
unsupported, even though that disk holds a complete, perfect copy of the files.
It now reads them. The interesting part is what the change was *not* — no code
that reads data was touched, because a mirror leg already lands every block at
exactly the same place a single disk would. Only the check that decides which
layouts to accept changed. That check is written as a list of what is allowed
rather than a list of what is banned, specifically so that two other layouts
which *look* like mirrors but are not safe to read stay excluded on purpose
rather than by luck.

**Decision.** `label::parse_config` accepts a top-level vdev whose `type` is
`disk`, `file` or `mirror`, and returns `NotSupported` for everything else. A
`mirror` must carry a non-empty `children` array; a non-mirror must carry none.
`ashift` is read from the top-level vdev, falling back to `children[0]`.
`dmu::Reader::read_block` and the whole zio path are unchanged.

**Why the read path needed no change, which is the whole decision.** A DVA (the
address stamped in every block pointer) names a *top-level* vdev and a byte
offset within it. A mirror is one top-level vdev whose legs hold byte-identical
content at identical offsets — that identity is not an implementation detail but
the definition, and it is exactly what lets ZFS itself satisfy a read from
whichever leg is idle. So a DVA resolves to the same physical offset on the
device in hand as on any other leg, and "read one leg" is not an approximation
of reading the mirror; it is reading the mirror. What was actually wrong before
was never the reader — it was a gate refusing an input the reader could already
handle.

Note also what did *not* change conceptually: a mirror is still a *single
top-level vdev*. It changes the vdev's type, not the pool's vdev count. Every
existing "this driver reads one top-level vdev" statement elsewhere in the
driver stayed true and stayed put.

**Why a whitelist, which is the actual tradeoff.** The obvious implementation is
a blacklist: reject `raidz`/`draid`, accept the rest. It is shorter, and it
reads more naturally ("parity is the problem, so exclude parity").

- **For the blacklist:** it names the real technical obstacle. A raidz leg holds
  interleaved data and parity columns, so the bytes at a DVA's offset are a
  stripe fragment and reconstruction needs every column — that *is* why raidz is
  refused, and encoding the reason is usually better than encoding a list.
- **Against, which won:** the rule "has children, children are replicas,
  therefore readable" is true of `mirror` and also true of `replacing` and
  `spare` — and those two are unsafe. A `replacing` vdev is a parent whose
  children are a dying disk and its incoming replacement; a `spare` vdev is the
  same shape with a hot spare attached. One child is **mid-resilver**, i.e. not
  yet a complete copy, and a single label does not tell us *which* child the
  device in hand is. Accepting them would be a coin flip on whether the reads
  are real data or a half-populated replacement. A blacklist admits them
  silently, and would admit any future ZFS vdev type the same way.

So the choice is between a rule that states the reason and fails open, and a
rule that states the permission and fails closed. For a gate whose failure mode
is *reading a disk incorrectly and reporting the result as file contents*,
failing closed wins, and the reasoning that a blacklist would have encoded lives
in a comment above the whitelist instead — where it costs nothing and cannot
admit anything. `replacing` and `spare` each get their own named test, so their
exclusion is asserted rather than incidental.

**Why the self-contradiction cases are `InvalidArgument`, not `NotSupported`.**
A `mirror` with no `children`, or a `disk`/`file` that has them, is not a layout
we decline to support — it is a config that disagrees with itself, and no real
ZFS writes one. The mirror case matters concretely: without the check we would
go on to trust a `children` list that is not there. `NotSupported` would tell
the user to go find a feature we lack; `InvalidArgument` correctly points at the
label.

The leaf-with-children branch was changed from `NotSupported` to
`InvalidArgument` as part of this, because **the same line of code now means
something narrower than it used to**. Before the whitelist, "has children" was
precisely how a mirror or raidz was *detected*, and `NotSupported` was the
correct answer to it. With type decided first, nothing but a malformed leaf can
reach that branch — so the old error kind had quietly stopped describing the
condition it guarded. Both contradictions now carry a named test, the leaf one
specifically so that a future reader who assumes the branch still guards raidz
is contradicted by a test rather than by nothing.

**Why the `ashift` fallback is safe.** Some labels record `ashift` only on the
leaves rather than on the mirror. Reading `children[0]` is legitimate because
every leg of a mirror shares one `ashift` by construction — a mirror of devices
with differing ashifts is not a thing ZFS creates. And the failure mode is not
silent: the uberblock stride derives from `ashift`, and an uberblock is
checksummed against the offset it was written at, so a wrong stride finds no
valid uberblock and the mount *fails* rather than reading the pool at the wrong
granularity.

**How this was verified, and why a mount-succeeds test would not have been
enough.** The decisive test mounts a mirror config against **unmodified image
bytes** — only the config nvlist differs from the single-disk case — and reads
`/hello` back byte-for-byte. Asserting only that the mount succeeded would prove
very little, because a mount touches one uberblock; it is the object tree
beneath it that would expose a wrong offset. The per-block checksum is a
backstop here (a misread block surfaces as `IoError`, not as wrong data), but it
is a backstop, not the argument.

**Where it lives.** `kernel/src/fs/zfs/label.rs` (`parse_config`, the vdev-type
whitelist and the `ashift` fallback); tests in `kernel/src/fs/zfs/tests.rs`;
scope statement in `kernel/src/fs/zfs/mod.rs`.

**How to reverse.** Delete `mirror` from the whitelist and the `children`
handling reverts to the old "any children → `NotSupported`". Nothing else
depends on it, because nothing else changed.

**What would change this.** raidz becomes readable only by presenting *all* legs
at once — a multi-device `SectorSource` and a stripe-reassembly layer, which is
a different feature, not a relaxation of this gate. `replacing`/`spare` would
need the label's own vdev GUID matched against the child list to identify which
child is in hand, plus a resilver-completion check on it; that is tractable and
deliberately not done here.
