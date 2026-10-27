## 1577. Hardware without a standard interface gets Linux's own drivers, through a compatibility layer

**Date:** 2026-10-27 · **Decided by:** Operator (Claude proposed this option, and recommended it over hand-porting) · **Lane:** A

**In short:** some hardware follows a public standard that one driver can
cover for every vendor -- disks, USB, basic display and sound. The rest --
graphics cards, WiFi, Bluetooth, laptop sound chips -- is each vendor's own
design, and only that vendor's very large driver knows how to run it. For
the first kind SlateOS writes its own drivers; for the second it runs
Linux's drivers, unchanged, through a layer that gives them the internal
functions they expect from Linux. Writing or hand-converting those drivers
instead would be millions of lines that Linux keeps changing every few
months.

### The decision

Drivers come two ways (`roadmap-detailed.md` §2.1a has the full split):

| Handwritten native driver (Rust) | Linux driver through the layer |
|---|---|
| Storage (NVMe, AHCI), USB host and class drivers, virtio, the UEFI GOP framebuffer, HD Audio and generic codecs, common wired network (Intel `e1000e`/`igb`/`igc`, Realtek `r8169`), the platform (ACPI, IOMMU, APIC, HPET, PCI, RTC, PS/2, I2C HID) | GPUs (`i915`/`xe`, `amdgpu`, `nouveau`/`nova`), WiFi (`iwlwifi`, `rtw88`/`rtw89`, `mt76`, `ath1xk`, `brcmfmac`), Bluetooth controllers, laptop audio DSPs (SOF/AVS, AMD ACP), Thunderbolt/USB4, card readers, laptop platform drivers, sensors |

Native when a public standard defines the device, when the driver is small,
when it is on the boot path (nothing may depend on the layer to start), or
when it moves data fast enough that crossing into a driver process per
request matters; the layer for one vendor's design that only its vendor's
driver knows.

Each Linux driver runs in a driver process of its own, never in the kernel,
its device's DMA confined by an IOMMU domain (§1576): the C code keeps
Linux's trust model, so the sandbox is the security boundary. The drivers are
GPLv2, so those processes and the layer linked into them are GPL; the kernel
and the native services stay MIT and talk to them only by IPC.

### The alternatives

| Alternative | For | Against |
|---|---|---|
| **The layer for vendor-specific hardware, native drivers for the standard core** (chosen) | Linux's drivers, kept current by their vendors, for the hardware nobody could write alone; a newer driver is a newer import, not a new port; native, fast, Rust drivers where a standard makes them small; the boot path depends on no foreign code | The layer must follow Linux's internal interfaces, which change every release, so new hardware arrives some versions late (as in FreeBSD's `drm-kmod`); emulating Linux's rules for sleeping, deferred work and memory ordering slightly wrongly makes bugs that show only under load; the drivers' processes are GPL |
| Hand-port each Linux driver to SlateOS's own driver interface | No emulation layer; each driver fits the native model | Several million lines for "most machines" -- `amdgpu` alone is over a million -- and the result is a fork of code Linux changes every nine weeks and extends for every year's new hardware; FreeBSD's hand-ported graphics drivers fell years behind before it moved to a layer |
| Write every driver natively from vendor documentation | Everything in Rust, in the native model | GPU and WiFi vendors publish too little; Linux needed thousands of contributors and the vendors' own engineers; not possible for one person with or without AI |
| Support only standard hardware | Smallest and simplest | No accelerated graphics, no WiFi, no Bluetooth, no sound on most laptops: not a usable desktop |

### Speed

The layer's own cost is small: most of it is thin wrappers onto native
primitives, and FreeBSD's GPUs perform close to Linux's through its layer.
The larger cost is SlateOS's design, not the layer's -- each interrupt and
request crossing into a driver process -- which native drivers pay too and
shared-memory rings and batching mostly hide. It matters only for the
fastest devices (NVMe, 10 GbE), which stay native.

### Where it lands

- `roadmap-detailed.md` §2.1a (the split, the rule of thumb, the work), §3.1
  (the GPU drivers come through the layer), §2.4 (WiFi's drivers do too).
- `roadmap.md`: the layer's task, lane A, with lane F's graphics bridge and
  the WiFi question flagged for when it gets there.
