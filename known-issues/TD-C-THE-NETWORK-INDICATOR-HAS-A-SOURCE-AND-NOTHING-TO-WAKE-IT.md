## TD-C-THE-NETWORK-INDICATOR-HAS-A-SOURCE-AND-NOTHING-TO-WAKE-IT

**Date:** 2026-09-26. **Lane:** C. **OPEN.**

**In short:** the taskbar has no network icon, though the Aero reference's
tray has one and `design.txt` lists it. The code for one exists --
`gui/desktop/src/network_indicator.rs`, 1,347 lines, on
`scripts/orphan-modules-baseline.txt` -- and so, now, does a place to read the
state from: the kernel serves `/sys/devices/net/{up,mac,ip,subnet_mask,
gateway,dns}` (absent with no network adapter). What is missing is a way for
the desktop to learn that the state *changed* without polling.

**Why not poll.** design-decisions §812: an idle desktop parks with no
wake-up registered at all, and a timer that reads a file which almost never
changes ends that for good. Reading on the wake-ups the desktop has anyway (a
pointer move, a key) keeps an active desktop current, but an idle one shows a
cable pulled an hour ago as connected.

**What the proper fix needs.**

- **An event when the link changes** -- up, down, a new address -- that the
  desktop's loop can wait on beside its compositor connection. The kernel's
  device-event registry (`fs::dmevent`) is the natural carrier; waiting on it
  from `oswindow::EventLoop` is lane F's side of the same change.
- **Pictures, not emoji**: the module draws `🔌`, `📶` and `✕`, which the
  default face lacks; the icon theme has `network-wired`, `network-offline`
  and the wireless ladder (§881 moved every other shell glyph to icons).
- **A flyout for a wired link**: the module's flyout is a Wi-Fi network list,
  and the kernel models one wired interface and no Wi-Fi.

**Where:** `gui/desktop/src/network_indicator.rs` (the indicator, unreached);
`kernel/src/fs/sysfs.rs` `gen_net_file` (the source); `gui/desktop/src/lib.rs`
`render_taskbar` (where the icon would go, left of the bell).
