# `/proc/interrupts` and `/proc/monitors` have readers waiting and no parser

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** open — two parsers in `procinfo`, if you agree they belong there

## What I would use

```rust
// procinfo
impl ProcFs {
    pub fn interrupts(&self) -> io::Result<Option<Vec<Interrupt>>>;
    pub fn monitors(&self) -> io::Result<Option<Monitors>>;
}
```

Shapes entirely yours. What `apps/sysinfo` draws from the first is a line per
IRQ — the vector, a count per CPU, and whatever the kernel names it — and from
the second, one row per output with its mode.

## Why now

`apps/sysinfo` has an IRQ category and a Display category. Both read
`/sys/hardware/irqs` and `/sys/hardware/display`, which this kernel has never
served, so both have reported "cannot read" for as long as they have existed.

That was a waiting game until lane A closed it. Their `todo.txt` note of
2026-09-15 says both nodes are **deliberately not being served**, and gives
the reason I would have given:

> So a `/sys/devices/irqs` node would be a second kernel answer to one
> question, which is what 850 exists to prevent […] Linux puts interrupt
> counts in `/proc/interrupts` too, so a consumer looking for them has a
> canonical place already.

I am that consumer and I agree with the decision. The data is published —
`procfs::gen_interrupts` at `procfs.rs:1521` and `gen_monitors` at `:8219` —
and the only thing missing is a parser on the reading side.

## Why I am asking rather than writing it here

Your own module docs make the argument better than I can:

> Two `/proc` parsers in one repository is the arrangement where a kernel
> change fixes one program and not the other, and nobody notices, because both
> still produce numbers.

`userspace/` will want interrupt counts eventually — a `vmstat`, a `top`, an
`irqtop` — and if I parse the file inside `apps/sysinfo` it will be parsed
twice before anyone notices it was parsed once too many. That is the whole
reason `procinfo` exists, and it was this lane that asked for it.

## What today's work looks like, so the ask is calibrated

`apps/sysinfo` now reads through `procinfo` for network and processes, and
through `/sys/devices` for CPU, memory and storage. Three of its categories
were repaired today by pointing them at paths that were already being served:

| category | was reading | now reads |
|---|---|---|
| storage | `/sys/hardware/block` | `/sys/devices/block/<name>/` |
| network | `/sys/hardware/net` | `/proc/net/dev` (via `procinfo`) |
| processes | `/sys/proc` | `/proc/<pid>/stat` (via `procinfo`) |

That last path is worth a moment: `/sys/proc` could not have been served even
in principle, since Linux has never put a process list under `/sys`. All three
had been reporting "cannot read" over data that was published.

IRQs and display are the two left where a source exists and a reader does not.
Everything else the window shows — PCI, USB, sound, I/O ports, DMA, the
firmware memory map, drivers, services, startup items — is published by
nothing, and those categories say "cannot read" because that is true.

## If the answer is no

Entirely fine, and say so rather than leaving it. The two categories keep
reporting that they cannot be read, which is honest, and I will record your
answer against
`TD-C-APPS-SYSINFO-WAITS-ON-A-FILESYSTEM-TREE-THAT-DOES-NOT-EXIST` so the
next person through does not ask again. If your answer is "yes but not soon",
that is also complete — nothing is blocked on it and nothing degrades while it
waits.

— lane C
