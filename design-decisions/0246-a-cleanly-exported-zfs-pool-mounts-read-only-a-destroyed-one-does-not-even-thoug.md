## §246 — A cleanly exported ZFS pool mounts read-only; a destroyed one does not, even though it would work

**Date:** 2026-08-20
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** A ZFS disk records what it currently *is* — in use, cleanly
unplugged, deleted, or a spare kept in reserve. SlateOS was ignoring that field
entirely and just trying to read whatever disk it was handed. Two of those
states are disks that hold no file data at all, so reading them produced a
misleading "this is not a ZFS disk" error about something that plainly is one.
The third, a *deleted* pool, is the interesting one: deleting a ZFS pool barely
touches the disk, so SlateOS would have mounted it and shown the files as if
nothing had happened. The decision is to read the field, to mount a cleanly
unplugged pool without complaint, and to refuse a deleted one by name — even
though refusing it means turning away a disk we could in fact read.

**Decision.** `label::parse_config` screens `state` and accepts only
`POOL_STATE_ACTIVE` (0) and `POOL_STATE_EXPORTED` (1). `SPARE` (3), `L2CACHE`
(4) and `DESTROYED` (2) return `NotSupported`. An absent `state` key is read as
`ACTIVE`.

**Why accept `EXPORTED`.** An export is the ordinary, correct way to hand a
disk from one machine to another: ZFS quiesces the pool, commits, and writes
the state. The on-disk image is consistent *by construction*, which is a
stronger guarantee than the `ACTIVE` case gives — an active pool's labels were
written by a machine that may still be writing. This driver is read-only, so
the only thing `ACTIVE` could offer over `EXPORTED` is the risk of racing a
live writer. Refusing `EXPORTED` would mean refusing the single commonest way a
ZFS disk legitimately arrives in another computer, and `zpool import` itself
takes an exported pool with no flag. There is no case for refusing it.

**Why refuse `DESTROYED`, which is the actual tradeoff.** `zpool destroy`
rewrites the state field and, essentially, nothing else — the uberblocks stay
valid, the object tree stays intact, and that is precisely why `zpool import -D`
can undo it. So a destroyed pool is *readable*, and refusing it turns away data
we could have shown.

- **For mounting it:** we are read-only, so we cannot make anything worse; the
  data is right there; a user plugging in a disk to recover files from a pool
  they deleted is a real and sympathetic case; and "it works" is a better
  outcome than an error.
- **Against, which won:** `destroy` is a deliberate statement that this pool is
  gone. A filesystem that silently resurrects it makes deletion meaningless —
  and worse, does so *invisibly*, since a mounted destroyed pool looks
  identical to a live one. ZFS's own design already settled this argument: it
  could have made `import` find destroyed pools by default and chose to require
  an explicit `-D`, so the recovery case is served by an extra gesture rather
  than by ambiguity. Matching that costs a recovery user one flag we have not
  built yet; matching the other way costs every user the ability to trust that
  a deleted pool is deleted.

The asymmetry is the point: mounting a destroyed pool fails *silently and
plausibly*, refusing one fails *loudly and recoverably*. When one error is
detectable by the user and the other is not, take the detectable one.

**Why `SPARE` and `L2CACHE` are not a tradeoff at all.** Both are genuine pool
members with complete, valid labels and no part of any object tree behind them
— a spare is unwritten reserve, an L2ARC device is an evictable copy of blocks
whose home is elsewhere. Before this change they reached `find_uberblock` and
failed as `InvalidArgument`, whose meaning is "this is not a ZFS device". That
is simply false about a device ZFS labelled itself, and a false diagnosis sends
whoever reads it looking in the wrong place.

**Why absent means `ACTIVE`.** Every real label carries `state`. Defaulting the
other way would convert *any* future failure to parse one nvlist key into a
refusal to mount a healthy pool — trading a bug that never fires for one that
fires on good hardware.

**Folded in, and why here rather than separately.** `SPA_VERSION_FEATURES` was
documented as "highest SPA version this driver will mount" while being compared
against nothing. It is now enforced (`version > 5000 → NotSupported`). No such
version exists, so this fixes no live bug; it makes an existing doc comment true
and costs one `u64` compare on a once-per-mount path. It rode along with the
state check rather than consuming its own eight-minute boot-test cycle, because
a check that cannot fire today cannot be validated by anything except the fact
that the ordinary pool still mounts — which the same boot proves.

**What would change this.** If a recovery workflow is ever wanted, the shape is
an explicit mount option that carries the user's intent, mirroring `-D` — not
relaxing the default. Relaxing the default would put the two indistinguishable
outcomes back.
