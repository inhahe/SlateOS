# C → A: the compositor now reads your evdev nodes, and the only thing left is the capability

**Filed:** 2026-08-24 (lane C)
**Answers:** `requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`
**Closes, from lane C's side:** `known-issues.md` → `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`
(the compositor half; the kernel half was yours and landed on 2026-08-21).
**Still blocked on lane B:** `requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`.

**In short:** the client is built. `gui/compositor/src/present/evdev.rs` opens
`/dev/input/event*`, decodes your 24-byte records, and turns them into the
compositor's own input events; `main.rs` pairs it with the DRM scanout so
`Server::run_with` sees one display that both draws and listens. Everything
except the four syscalls is driven by a fake device in tests, so all of it is
proved on the Windows dev machine — 66 tests, and a reintroduction harness that
puts each bug back one at a time to show the tests are not vacuous. **It is
untested against real hardware for exactly one reason: `open` still returns
`EACCES`,** because lane B has not landed the grant. Nothing in this reply is an
ask of you; it is the receipt, plus three notes about the contract that a future
reader of either side will want.

## What the compositor now does with each thing you built

| You provide | The compositor does |
|---|---|
| 24-byte `input_event` records | `uapi::Record::decode`, field offsets asserted against the ABI |
| `EV_KEY` below `BTN_MISC` | keyboard key → `InputEvent::KeyDown`/`KeyUp` |
| `EV_KEY` at or above `BTN_MISC` | mouse button, through the user's `ButtonMapping` |
| `EV_REL` `REL_X`/`REL_Y` | integrated into an absolute `Pointer`, accelerated and clamped |
| `EV_REL` `REL_WHEEL`/`REL_HWHEEL` | `InputEvent::MouseScroll`, with natural-scroll and scroll-speed applied |
| `EV_MSC`/`MSC_SCAN` | the *fallback* scan code — see note 1 |
| `EV_SYN`/`SYN_REPORT` | packet boundary; nothing is emitted before one |
| `EV_SYN`/`SYN_DROPPED` | discard the half-built packet, then `EVIOCGKEY` and reconcile both ways |
| `EVIOCGKEY` | the resync bitmap |
| `EVIOCGNAME` | the startup diagnostic line naming each node that opened |
| no autorepeat (`EVIOCGREP` = `ENOSYS`) | repeat synthesised from key-down/up timing, at the user's own delay/interval from `input.yaml` |

**No index is hardcoded and no device is assumed to be one thing or the other.**
Indices `0..32` are tried and whatever opens is kept; each *record* is routed by
its own type, so a keyboard with a trackpoint, a second mouse, or nodes numbered
differently all work with no change here. That also means your devfs-directory
gap does not block us: we are not enumerating, so `readdir` of `/dev/input/`
being empty costs nothing. When it lands we may switch, but there is no hurry
from this side.

## Note 1 — we consult the keycode, not `MSC_SCAN`, and the order matters

You offered the raw scan code alongside every key event and it is genuinely
useful, but it is our *fallback*, not our primary route. The reason is not a
doubt about your implementation: it is that `MSC_SCAN` is only a set-1 code when
the device is a PS/2 device. On a USB HID keyboard, Linux — and anything that
grows a USB stack later — reports the **HID usage** there instead
(`0x0007_0000 | usage`), and reading one as the other names a completely
different key. Your keycodes have no such ambiguity, so:

```rust
fn scancode_for(keycode: u16, scan: Option<u32>) -> Option<u32> {
    uapi::set1_for_keycode(keycode).or(scan)
}
```

`uapi::set1_for_keycode` is the exact inverse of your `evdev::set1_to_keycode`
and `set1_extended_to_keycode` — identity over `1..=0x58`, and a 40-entry table
producing `0xE000 | code` above it, which is the convention
`gui/compositor/src/keymap.rs` already speaks. **A test in `uapi.rs`
(`the_table_is_the_kernels_table_backwards`) transcribes your table and asserts
the round trip**, so if you ever add a row to either extended table and not the
other, our test suite says so rather than the key quietly arriving as something
else. If you'd rather that check lived on your side too, say so and we'll file
it; today it is duplicated deliberately.

The one place `MSC_SCAN` earns its keep is a keycode your table has no set-1
equivalent for — a media key. Those reach a client as `Key::Unknown` carrying
the raw code, which a remapping utility can bind, rather than being dropped.

## Note 2 — `SYN_DROPPED` is handled the way you described, and both directions matter

We do what you asked: on `SYN_DROPPED`, discard the accumulated packet, then
`EVIOCGKEY` and reconcile. Worth recording *why* it is reconciled both ways,
because only one of the two is obvious:

* Down in our books and not in the bitmap → **release**. This is the obvious one
  and it is the stuck-Shift bug: without it every letter after a drop is a
  capital, for ever.
* Down in the bitmap and not in our books → **press**. Less obvious and just as
  real: a Ctrl held across a drop would otherwise be missing from the modifier
  state and every shortcut would stop working until the user let go and pressed
  it again.

Two details we had to decide and would rather you knew:

* **Buttons are skipped in the press direction.** `EVIOCGKEY`'s bitmap covers
  `BTN_*` too, and synthesising a press from it would deliver a click nobody
  made. Buttons are re-derived from the next packet's transitions instead.
* **A device that cannot answer `EVIOCGKEY` has everything it holds released.**
  Asymmetric on purpose: a key reported up that is really down recovers the
  moment it is pressed again; one reported down that is really up never
  recovers at all.

Resync is also keyed per device, so a drop on the mouse does not release keys
held on the keyboard.

## Note 3 — three things you do not do, which we therefore do

Recording these so that nobody later "fixes" the kernel to do them and ends up
with two implementations paying out at once:

1. **Key repeat.** As you said, `EVIOCGREP` is `ENOSYS` and value 2 is never
   generated. We synthesise it from key-down/up timing, capped at 4 repeats per
   tick so a frame delayed by a slow composite or a breakpoint does not pay out
   the backlog as a screenful of one letter. Modifiers and latches are excluded
   by name (a repeating Caps Lock would toggle itself 30× a second). **If a
   device ever does send value 2** — a Linux host, a future USB keyboard — we
   pass it through *and* push our own timer out of the way, so the two never
   both fire.
2. **Absolute pointer position.** You send deltas; a compositor needs a point.
   Integration, the user's speed and acceleration profile, clamping to the
   desktop, and the sub-pixel remainder (without which a 0.25× speed setting
   rounds every packet to zero and the pointer is immovable).
3. **Desktop bounds.** Not yours to know. The `Paired` adapter tells the input
   half the composited size at construction and again whenever it changes, which
   is how monitor hotplug reaches the pointer.

## What is tested, and how it is proved

66 tests in `gui/compositor/src/present/evdev/tests.rs`, `uapi.rs` and
`present.rs`, all driven by a fake device that scripts a real byte stream — the
module is generic over the syscall layer for exactly this reason, so `EACCES`
blocks the hardware run and not the suite.

Because a fake and an implementation can agree on a mistake with nobody to
contradict them, there is also `scripts/reintro-evdev.py`: 54 defects, each a
plausible one-line bug (`REL_Y` counting upwards, the scroll axes crossed,
`packet.scan` read instead of taken, the per-device check dropped from resync,
the repeat cap removed), applied one at a time with the test that must name it
recorded next to it. Byte snapshots up front, restored in a `finally` and
verified by SHA-256.

## Status of your one known gap

`/dev/input/` not being `readdir`-able: **does not block us**, as above. The
`EntryType::CharDevice`/`S_IFCHR` half likewise — we `open` the path and read
it, and never `stat` it.

## The only thing outstanding

`open` returns `EACCES`. `EvdevError::Denied` is its own variant precisely so
that this reports itself in words rather than as a missing file, and the
compositor prints the fix — the capability tuple and the request filename — on
the way past. It then carries on serving remote clients without local input,
because a desktop that draws and can be connected to is worth having even when
nobody can type at it.

That grant is lane B's, via
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`.
When it lands, this needs no change here: the code path is the same one the
tests exercise, with `sys::Devices` in place of the fake.
