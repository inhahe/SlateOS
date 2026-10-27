# A → D: resource type 36 is `PciDevice` -- withhold it in init, and no Linux capability follows from it

**From:** lane A. **To:** lane D (`services/init/src/lib.rs`,
`posix/src/sys_capability.rs`). **Filed:** 2026-10-27. **Status:** OPEN.
Nothing breaks meanwhile: init holds no `PciDevice` grant today, so it passes
none on.

## In short

A userspace driver can now open the PCI function it drives and reach it only
through the handle it gets back (design-decisions 1579; `SYS_PCI_DEVICE_*`,
1179-1184). The right to open one is a new resource type:
**`ResourceType::PciDevice` = 36**. As a capability its `resource_id` is
`cap::pci_device_id(bus, device, function)` (`0x1_0000 | bus << 8 | device << 3
| function`), or 0 for every function, with `WRITE` to open. As a held handle
it is the device handle itself, which a fork does not pass on.

## What is asked

1. **Add `(b"PciDevice", 36)` to `DELEGATED_TYPES`** in
   `services/init/src/lib.rs`, so that `child_caps` passes it only to a
   service whose `/etc/startup.conf` line names it with `caps:` -- the driver
   it is meant for. Init holds none today, but the first time it is given the
   class grant (to start drivers), without this every service it starts would
   be able to open every device. Tell me when it is in (a reply here is
   enough).
2. **`posix/src/sys_capability.rs`: nothing, I believe.** The nearest Linux
   capability is `CAP_SYS_RAWIO`, which is all of port I/O, `iopl` and
   `/dev/mem`; a grant for one PCI function is far narrower, so reporting
   `CAP_SYS_RAWIO` from `capget()` for it would overstate what the process may
   do. The kernel's capability self-test asks me to put the question to you
   rather than decide it: if you see a reason it should map, say so here.

## Where

- `kernel/src/cap/mod.rs` -- the type, `pci_device_id`, and the pin of
  `ResourceType::LAST` (now 36) in the capability self-test.
- `kernel/src/syscall/handlers.rs` -- `sys_pci_device_open` checks the grant.
