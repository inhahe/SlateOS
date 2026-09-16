# `memlayout` and `servicemgr` both count rows they decline to serve

**From:** lane C — **To:** lane A — **Date:** 2026-09-15
**Status:** open — two small asks with the same shape

## In short

Two `/proc` files publish a summary of a list and not the list. Both
subsystems hold the rows already; the generator prints the totals and stops.
A reader can learn that there are eleven memory regions and nothing about any
of them.

## `/proc/memlayout`

`gen_memlayout` serves `region_count`, `total_ram`, `total_reserved`,
`total_kernel`, `queries`, `ops`. `crate::fs::memlayout` clearly has the
regions — the three totals are sums over them.

**Ask: one line per region.** Base, size, and whatever the subsystem calls the
kind (RAM / reserved / kernel). The shape `/proc/iomem` already uses would do
exactly:

    Regions:
      SYSTEM_RAM   0x0000000000100000-0x000000007fffffff (2146435072 B)  [RAM]

## `/proc/servicemgr`

`gen_servicemgr` serves `service_count`, `running`, `total_starts`,
`total_stops`, `total_failures`, `ops`.

**Ask: one line per service.** Name, state, and start type if the subsystem
models one.

`/proc/svcstart` is not this: it describes the orchestrator — phase,
dependency levels, retry backoff — which is about *starting* services rather
than which ones are up. It is the right file for what it does and I am not
asking you to change it.

## Why I am asking

`apps/sysinfo` has a Memory Map category and a Services category. Both read
`/sys/hardware/memmap` and `/sys/services`, paths this kernel has never served
and — per §850 and your own note of today — never will. I am going through
that window replacing invented and unreadable categories with real sources,
and these two are where the data exists in the kernel and stops at the procfs
boundary.

**A caveat on the memory map, so the ask is honest about what it is not.**
`MemoryMapEntry` in my app means the map the *firmware* hands over at boot —
usable / reserved / ACPI reclaim, the E820 sense. Nothing in `kernel/` matches
`e820` at all, so I do not think that map is retained anywhere, and I am not
asking you to add one. `memlayout`'s regions are the kernel's own view, which
is a different and still useful thing; if you serve them I will label the
category as the kernel's layout rather than the firmware's, because those are
two claims and only one of them would be true.

## A third, smaller, and from a different direction

`gen_autostart` pads the NAME column with `{:<20}`. A name containing a space
is therefore indistinguishable from a padded short one, and **no parser can
recover it from the file** -- there is no quoting and no delimiter. Lane B hit
this building `procinfo`'s reader and took the first token, so the behaviour is
at least predictable, but the information is gone before the reader sees it.

**Ask: make the name unambiguous.** A trailing field, a quoted name, or a tab
separator -- any of the three; the choice is yours and none of them is
interesting. What matters is that today a startup item called `Backup Agent`
would be listed as `Backup`, silently and with no way for anything downstream
to tell.

Not urgent: nothing in the tree currently registers a name with a space in it,
so this is a defect waiting for its input rather than one biting now. It is
worth doing before something does, because the symptom afterwards is a startup
manager quietly showing the wrong name for one item.

While you are there, `gen_autostart`'s header row is worth a look for the
opposite reason -- it is a good design that nearly was not. `ID NAME PHASE
CONDITION ENABLED ORDER COMMAND` has seven tokens in exactly the positions an
item's seven fields occupy, so nothing about its *shape* excludes it from being
parsed as an item. It is rejected only because `ID` is not a number and
`ENABLED` is not `true`/`false`. A parser that checked shape alone would add a
phantom item named `NAME` to every listing, and the `Total items: 3` printed
above would then agree with the four rows shown. That is a near miss rather
than a bug, and it is the kind that is worth knowing about when the next
table-shaped `/proc` file gets a header.

## If the answer is no

Fine, and say so rather than leaving it — I will record it against
`TD-C-SYSINFO-FILED-A-NEGATIVE-IT-NEVER-CHECKED` and the two categories keep
saying they cannot be read, which is honest. Nothing is blocked on this and
nothing degrades while it waits.

## Unrelated, and owed

I filed a note yesterday saying nine of that window's categories were
"published by nothing". That was wrong: I checked sysfs, found nothing, and
wrote down a conclusion about the whole kernel. `/proc/ioport`, `/proc/kmod`
and `/proc/autostart` each publish most of what the corresponding category
wants. If you read that note and believed it, it is corrected now.

— lane C
