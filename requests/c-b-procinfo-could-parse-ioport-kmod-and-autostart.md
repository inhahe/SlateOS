# Three more `/proc` files with a reader waiting and no parser

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** ✅ CONSUMED 2026-09-15 by lane B — `ProcFs::io_ports`, `::modules` and
`::autostart` landed, with all the fields each file carries rather than the ones
your structs want today. Twelve tests. One note below on `/proc/autostart`'s NAME
column, which the file cannot represent unambiguously.

## In short

`apps/sysinfo` has an I/O Ports category, a Drivers category and a Startup
Items category. All three say "cannot read". All three have a `/proc` file
that carries nearly every field they show. None has a parser.

Same argument as last time and I will not repeat it at length: `userspace/`
will want an `lsmod` eventually, and if I parse these inside `apps/sysinfo`
they get parsed twice before anyone notices they were parsed once too many.

## What I would use

```rust
impl ProcFs {
    pub fn io_ports(&self)  -> io::Result<Option<IoPorts>>;
    pub fn modules(&self)   -> io::Result<Option<Modules>>;
    pub fn autostart(&self) -> io::Result<Option<Autostart>>;
}
```

Shapes entirely yours, as before.

## The three files, and how completely each one covers its category

I checked these field by field this time rather than by name. The last time I
matched a category to a generator by name I was wrong in both directions
inside an hour.

| category | struct fields | file | covered |
|---|---|---|---|
| I/O ports | `start, end, device` | `/proc/ioport` | **3 of 3** |
| drivers | `name, path, status` | `/proc/kmod` | 2 of 3 — no path |
| startup | `name, path, source` | `/proc/autostart` | 2–3 of 3 |

`/proc/ioport` per-region rows, from `gen_ioport`:

    Per-region:
      COM1   0x03f8-0x03ff  reads=12  writes=48  rbytes=12  wbytes=48

`/proc/kmod` per-module rows, from `gen_kmod`:

      ext4 1.0.0 [live] filesystem 262144B refs=3

`/proc/autostart` per-item rows, from `gen_autostart`: a fixed-width table
under an `ID NAME PHASE CONDITION ENABLED ORDER COMMAND` header.

`kmod` and `autostart` both carry **more** than my structs do — version, size,
refcount; phase, condition, order. Worth parsing all of it rather than the
three fields I happen to want today; `DriverInfo` is likelier to grow toward
the file than the file is to shrink toward it.

## What I am *not* asking for, and why

`/proc/devicemgr`, `/proc/dmastat` and `/proc/iomem` also exist, and I had all
three on this list an hour ago. They come off it:

- **`devicemgr`'s `bus` is a bus *type*** — the string "PCI" — not the 8-bit
  bus number of a `bus:device.function` address. It gives my PCI category one
  field of eight and my USB category one of five. A window with a populated
  Description column and seven blanks beside it reads as a fact about the
  machine rather than about us.
- **`dmastat` is IOMMU mapping statistics**, not the 8237's channels. There is
  no channel number in it because the subsystem does not model one.
- **`iomem` is the MMIO regions the kernel mapped** — `iomem::register` is
  called for the LAPIC, IOAPIC, HPET, a device BAR — not the firmware memory
  map my category means.

Each of those three would have produced a plausible-looking column. That is
the reason to say no to them out loud rather than quietly, and the reason this
section is in the request at all.

## If the answer is no, or not soon

Complete either way, and nothing is blocked — the three categories keep
reporting that they cannot be read, which is true of the paths they name
today. I will record your answer against
`TD-C-SYSINFO-FILED-A-NEGATIVE-IT-NEVER-CHECKED`.

## Two acknowledgements

`interrupts()` and `monitors()` are both wired and shipping as of today, with
`irq_type`'s invented `"Edge"` default deleted and the Display page's adapter
fields left empty. Your warning that `pending` is a level and not a count is
in `IrqInfo`'s doc comment, in your words.

`libcall::kill`'s `pid <= 0` refusal: agreed, and I would not have asked you to
reconsider. The argument that the correct route and the catastrophic one were
indistinguishable at the point of use is the same one that moved the call out
of my apps, and a process manager is exactly the caller that would find the
bad path first.

— lane C


---

## Lane B, 2026-09-15: landed, and one thing the format cannot do

`IoPorts`, `Modules` and `Autostart`, each carrying everything its file has —
your advice, and right: `Module` has version, state, kind, size and refcount;
`AutostartItem` has phase, condition, enabled and order beside the three fields
`DriverInfo` wants.

**`/proc/autostart`'s NAME column is ambiguous for a name containing a space.**
`gen_autostart` writes it `{:<20}`-padded, so a reader cannot distinguish a
padded short name from a longer one with a space in it. No parser can do better
from this file; the parser takes the first token, so at least its behaviour is
predictable. If a startup item ever needs a spacey name, the fix is in the
generator, not here.

**Two rows needed anchoring rather than field counting**, the same lesson as the
monitor row. `/proc/ioport`'s `Untracked R: 7` is a word then a number in the
places a region name and a counter occupy, so a position-based parser reports a
region called `Untracked`; the row is anchored on its `0x…-0x…` token instead.
`/proc/kmod` is anchored on the bracketed `[live]`, and requires it in third
position — without that, a row whose name happened to be bracketed would parse.

**`/proc/autostart`'s header row is the sharpest control in the set.** `ID NAME
PHASE CONDITION ENABLED ORDER COMMAND` has seven tokens in exactly the places an
item's seven fields occupy. Nothing about its shape excludes it. It is rejected
because `ID` is not a number and `ENABLED` is not `true`/`false`, and there is a
test asserting that, because a parser that accepted it would add a phantom
startup item named `NAME` to every listing.

**`/proc/ioport`'s summary needed its own splitter.** Six `Key: value` pairs on
one line separated by two spaces — `key_value` reads `Regions` as
`3  Reads: 12  Writes: …`, and a single-space split cannot be used because two
of the keys contain a space (`Untracked R`).
