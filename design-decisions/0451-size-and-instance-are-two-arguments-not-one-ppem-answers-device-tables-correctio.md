## §451 — Size and instance are two arguments, not one: `Ppem` answers device tables, `Corrections` answers both

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** fonts can ship hand-written nudges that say "at *this* size, move
this letter 1 pixel left" — a *device table*. Adjustable ("variable") fonts can
instead ship a nudge that says "at *this weight*, move it 5 units left". The two
live in the same slot in the file and are told apart by one marker word, so the
code reading that slot has to be handed two different things: a pixel size for
the first kind, and the reader's chosen weight/width/etc. for the second. The
decision is to pass those as **two types** — the existing `Ppem` for callers who
only have a size, and a new `Corrections` for callers who have both — rather
than widening `Ppem` to carry a weight it usually does not have.

### The problem

`Ppem` is a two-field `Copy` value: pixels-per-em and units-per-em. It is
constructed everywhere and passed by value down through `gpos`, `kern` and
`mark`. The `VariationIndex` arm needs two more things that `Ppem` has no
business owning: a **borrow** of `GDEF`'s `ItemVariationStore` and a **slice**
of normalized coordinates. Both are borrows, so carrying them turns `Ppem` from
a plain value into a lifetime-parameterised one at every one of those call
sites.

### Options

| | *What changes:* |
|---|---|
| **Widen `Ppem`** | `Ppem<'a>` everywhere; every existing caller that has only a size must invent an empty instance to satisfy the type. |
| **Two types (chosen)** | `Ppem` keeps its current shape and meaning; a new `Corrections<'a>` composes it with the instance and is what `gpos`/`kern`/`mark` thread. |
| **Two separate `delta` calls** | Callers ask `Ppem` first and the store second and add the answers; every call site has to know which arm applied. |

**Two types, because a caller that knows a size but not an instance is not
making an error.** A non-variable face drawn at 11px is exactly that case, and
it is the overwhelmingly common one — 549 of this host's 556 faces. Widening
`Ppem` would make all of them spell out an emptiness that means nothing, and
would put a lifetime on a value whose whole point is that it is a trivially
copyable pair of numbers. `Corrections::at(ppem)` is the bridge, `const`, and
reads as what it is.

The third option was rejected outright: the two arms are *alternatives*, not
addends — a slot holds one table or the other, never both — so a shape that
lets a caller add them is a shape that lets a caller be wrong.

### The asymmetry that makes this more than plumbing

**The two corrections are not in the same units, and this is the specification's
doing, not ours.** A device table's delta is in **pixels**, so `Ppem::delta`
converts it back through the em and it correctly contributes nothing at
`Ppem::NONE` (no size means no pixel to correct). A `VariationIndex`'s delta is
already in **font units**, so it bypasses that conversion — and consequently
**applies at `Ppem::NONE` too**, where the other kind cannot.

This matches HarfBuzz, where `VariationDevice::get_x_delta` scales the store's
value by the font scale while `DeviceTable::get_x_delta` scales it by `x_ppem`.
A reader that routes both arms through one conversion is wrong twice: wrong
magnitude, and silently zero whenever the caller supplied no ppem. That is the
single most important fact in this section, and it is why `Corrections` owns the
dispatch rather than `pixel_delta` growing a branch — `pixel_delta` is the
pixel-units function, and the other arm is not in pixels.

### `store` is `None` at the default instance, not just for a face without one

At the default instance every delta in the store is zero by construction, so
there is nothing to compute. Collapsing the two cases means the common path does
no work *and* cannot be told apart from the case where there is nothing to do —
which is the right invariant, because a difference between them would be a bug
either way.

### What this cost to prove, which is the part worth remembering

Nothing about the arm is reachable from a differential test that only ever asks
at the default instance: there, every `VariationIndex` is zero, every `avar`
segment is the identity, and every `HVAR` advance is the stored one. The reader
runs, agrees with HarfBuzz, and proves nothing. Teaching the oracle to ask at an
instance (`shape_dump`'s third argument, `harfbuzz_sweep.py --axes`) turned up
**three** real bugs on the first sweep, and **two of them had nothing to do with
variable fonts** — a `PairPos` format 1 device base measured from the wrong
parent table, and `avar` interpolating with truncating division. Both had
survived a 556-face sweep because the default instance never reaches them.

The general lesson, which is not specific to fonts: **a differential test proves
only the questions it asks.** A dimension the oracle is never varied along is a
dimension in which both sides can be equally wrong, indefinitely, while the
sweep reports agreement. When adding a feature that introduces a new input
dimension, the oracle has to learn that dimension before the feature can be
called proved.

**Where:** `gui/font/src/device.rs` (`Corrections`, `variation_delta`),
`gui/font/src/sfnt.rs` (`gdef_store`, `Face::corrections`),
`gui/font/src/gpos.rs`, `gui/font/src/kern.rs`, `gui/font/src/mark.rs`,
`gui/font/src/scaled.rs`. See `known-issues.md` →
`TD-FONT-DOES-NOT-READ-VARIATION-STORES` (now FIXED) for the three bugs.
