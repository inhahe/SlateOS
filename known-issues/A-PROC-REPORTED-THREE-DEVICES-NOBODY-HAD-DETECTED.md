## A-PROC-REPORTED-THREE-DEVICES-NOBODY-HAD-DETECTED (lane A, 2026-09-15) — **Status: FIXED**

**What it was.** `kernel/src/fs/dmevent.rs`'s `init_defaults()` seeded three
`KnownDevice` entries and `procfs::gen_dmevent` published them, so `/proc`
reported hardware that was never detected:

| devname | devpath | state |
|---|---|---|
| `sda` | `/sys/block/sda` | `online: true` |
| `eth0` | `/sys/class/net/eth0` | `online: true` |
| `keyboard0` | `/sys/class/input/keyboard0` | `online: true` |

Each carried `first_seen_ns: <now>` — the moment the table was initialised — so
the record did not merely list them, it claimed a time of observation.

**Three things made it worse than a placeholder.**

1. `eth0` is the exact invented interface name lane A declined to publish under
   `/sys/devices/net` earlier the same day, on the grounds that `InterfaceInfo`
   carries no name field so any name would be fabricated. It was already being
   published one file over, and the refusal one directory away did not prompt
   anyone (me) to look here.
2. The devpaths name `/sys/block` and `/sys/class`, trees this kernel does not
   serve at all. Unresolvable as well as untrue.
3. **Nothing produces real events.** `dmevent::notify` is called only by a
   manual `kshell` command and by this module's own self-test — no driver calls
   it. So the three were not a seed that real data grows from; they were the
   only content the file would ever have.

**The self-test asserted the fixture** (`list_devices().len() == 3`), which made
the fabrication load-bearing: deleting it would have turned the suite red, and
the suite would have looked like the thing that broke. That is the same shape
lane C reported repeatedly on 2026-09-15 while removing invented behaviour from
`apps/` — several of their tests were asserting the invention. It is worth
stating as a general property: **a fabrication that has a test is harder to
remove than one that does not, and the test is the reason it survived.**

**Fix.** The devices are gone; the assertion is now `0`, and the post-`notify`
assertion is `1` rather than `4`, which also makes the increment the case
actually proves visible instead of buried in fixture noise.

**The two default `EventRule`s are KEPT** — automount on block `sd`, notify on
`usb`. Shipped policy is not an invented inventory: a rule that has not fired
claims nothing about the hardware present, the same way a stock udev ruleset
claims nothing.

**And `/proc` now says what zero means.** Deleting the seeds leaves
`devices: 0`, which reads as *this machine has no devices* when the truth is
*nobody reports any*. `gen_dmevent` emits a note saying `dmevent::notify` has no
producer, so zero is unreported rather than none. That correction is owed
directly to lane C's finding the same day that rendering an unread partition
table as `0 partition(s)` tells the reader a drive is blank — the removal of a
fabrication leaves an absence, and an absence renders as a claim unless
something says otherwise.

**Still open, deliberately:** `dmevent` has no producer. Wiring real driver
events into it is a separate piece of work; an empty, honest table is the
correct state until then, and is now labelled as such rather than filled.
