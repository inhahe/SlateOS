## 544. Reading the keyboard and mouse: six decisions the kernel's device nodes left to us

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The kernel now offers the keyboard and mouse as files the
compositor can read (`/dev/input/event0` and `event1`), which is what it takes
for someone sitting at the machine to type into it rather than connecting over
the network. Reading those files is not enough on its own — the kernel hands
over a stream of *changes* ("the mouse moved 3 to the left", "the A key went
down") and a desktop needs *state* ("the pointer is at 412, 300", "A is being
held and should be repeating"). This entry records the six judgement calls made
in turning one into the other, all of them mine, none of them forced by the
kernel's design.

**Glossary, used throughout:** *evdev* is the Linux convention for exposing an
input device as a file that emits fixed-size event records. A *scan code* is the
number the physical keyboard hardware sends; a *keycode* is the number Linux
assigns to the key regardless of hardware. `SYN_DROPPED` is the record the
kernel emits to say "you fell behind and I threw events away". `EVIOCGKEY` is a
request that answers "which keys are held down *right now*" — a snapshot, as
opposed to the stream of changes.

The kernel side is lane A's, filed as
`requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`; the
reply naming what we built is
`requests/c-a-the-compositor-now-reads-your-evdev-nodes-and-is-waiting-only-on-the-capability.md`.

### 1. The keycode is the primary route; the raw scan code is only a fallback

Every key event arrives twice over: as a keycode, and as the raw scan code in a
companion `MSC_SCAN` record. Our keymap speaks scan codes, so taking `MSC_SCAN`
is the obvious route and it is wrong.

`MSC_SCAN` only carries a PS/2 set-1 scan code *because these are PS/2 devices*.
Linux puts the **HID usage** there for a USB keyboard instead — a different
number space entirely — so the same code read the same way names a different
key the day a USB stack exists. The keycode has no such ambiguity: it is the
same number whatever the hardware.

So `scancode_for` converts the keycode back to set 1 and consults `MSC_SCAN`
only when that conversion has no answer:

```rust
uapi::set1_for_keycode(keycode).or(scan)
```

The cost is a 40-entry table that must stay the exact inverse of the kernel's
two forward tables. That is a genuine duplication across a lane boundary, and
it is paid for deliberately: `uapi.rs`'s
`the_table_is_the_kernels_table_backwards` transcribes lane A's table and
asserts the round trip, so a row added on one side and not the other fails a
test rather than silently delivering the wrong key.

**Rejected:** *read `MSC_SCAN` and skip the table.* Simpler today, and correct
today, and it fails silently rather than loudly the moment the hardware
assumption changes. A wrong key is not an error anyone can debug from the
symptom.

### 2. Input is paired with the display by a separate adapter, not welded onto it

`Present` is the trait a display implements. The screen and the input devices
are different pieces of hardware with no relationship — but the pointer has to
be clamped to the desktop's size, which only the display knows.

The options were to give `DrmScanout` (the display) an input half, or to build
a `Paired<Screen, Input>` adapter that holds one of each and forwards. The
adapter won. A display that draws and also happens to read the keyboard is two
responsibilities in one type, and it would force every *other* display —
headless, recording, the host window used for development — to grow the same
seam. The adapter is the only place that knows both, which is exactly one
place.

The ordering detail that shaped the constructor: `Server::run_with` calls
`input()` **before** the first `show()`, so a pointer that learned the desktop
size from the first frame would spend that frame clamped to nothing. `Paired`
therefore takes the size at construction and re-sends it only on a change.

### 3. Key repeat is synthesised here, because the kernel refuses to fake it

The kernel returns `ENOSYS` for the repeat-rate requests and never emits a
repeat event. That is the honest answer — it has no timer for it — and it makes
repeat ours.

Two details are not obvious and both are load-bearing:

- **Repeats are capped at 4 per tick.** A frame delayed by a slow composite or
  a breakpoint would otherwise pay out the whole backlog at once, turning a
  held key into a screenful of one letter.
- **Modifiers and latches never repeat.** A repeating Caps Lock would toggle
  itself thirty times a second.

And if a device ever *does* send a repeat — a future USB keyboard, or running
against a real Linux host — we pass it through *and* push our own timer out of
the way, so the two can never both pay out.

### 4. A device that cannot say which keys are held has all of them released

After `SYN_DROPPED` the stream has a hole, so the set of held keys is
reconstructed from an `EVIOCGKEY` snapshot. Reconciling it is deliberately
asymmetric, and the asymmetry is the decision:

| Case | Action | Why |
|---|---|---|
| Held in our books, up in the bitmap | release | the stuck-Shift bug: every letter capital, for ever |
| Up in our books, held in the bitmap | press | a Ctrl held across the drop, otherwise missing from every shortcut |
| Buttons, in the press direction only | **skip** | synthesising a press would deliver a click nobody made |
| The snapshot request fails | release everything | see below |

A device that cannot answer leaves us with no information at all, and the two
possible guesses are not equally bad. **A key wrongly reported up recovers the
instant it is pressed again. A key wrongly reported down never recovers.** So
the failure path releases, and accepts a dropped modifier over a permanently
stuck one.

Resync is per device, so a drop on the mouse does not release keys held on the
keyboard.

### 5. Devices are opened by trying `0..32` blindly rather than by listing the directory

The right way to find input devices is to list `/dev/input/`. Our devfs is flat
and has no directories, so that list is empty — lane A is fixing it, and it is
not blocking.

Rather than wait, we try indices 0 through 31 and keep whatever opens. Every
record is then routed by *its own type*, so nothing depends on which index
turned out to be the keyboard. A keyboard with a built-in trackpoint, a second
mouse, or nodes numbered differently all work with no change.

This is the rare case where the cruder mechanism is also the more robust one:
an enumerating client believes the directory, and a client that routes by
record content believes the device. We will likely switch to enumeration when
it lands, and nothing above it will need to change.

### 6. `EACCES` is reported in words and then survived

Opening a device fails with a permission error until the compositor is granted
an `InputDevice` capability at spawn — which is lane B's, via
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`,
and is not landed. That error looks exactly like a missing-file bug and is not
one, so it gets its own error variant (`EvdevError::Denied`) and the compositor
prints the fix — the capability tuple and the request filename — as it goes
past.

Then it carries on **without local input**, serving remote clients. A desktop
that draws and can be connected to is worth having even when nobody can type at
it, and refusing to start would make an unlanded cross-lane dependency look
like a broken compositor.

### How this is tested at all, given none of it can run here

The four syscalls (`open`, `read`, `ioctl`, `close`) sit behind a trait, and
everything above them — the wire format, the keycode tables, packet assembly,
acceleration, repeat, resync — is driven by a fake device that scripts a real
byte stream. So the whole module is exercised on the Windows development
machine, and the missing capability blocks the hardware run rather than the
test suite. 84 tests — 72 driving the module through the fake, 12 on the wire
format and keycode tables directly.

Because a fake and the code it feeds can agree on the same mistake with nobody
to contradict them, `scripts/reintro-evdev.py` puts 54 plausible one-line bugs
back one at a time and records which test must name each. 46 were named
immediately. The other eight are the interesting ones, and they did not all
mean the same thing:

- **Five were tests passing vacuously**, which is what the sweep is for. A
  missing clamp masked by a second clamp downstream; a divide-by-zero guard
  masked by a saturating clamp further along; an idle-device test that never
  went on to show the device was still being read; and two device-scan tests
  that put their devices at indices 0 and 1, where the search starts, so
  neither could tell "scans every index" from "looks at the first two". None
  of the five would have been found by reading.
- **One was a self-referential invariant.** The only thing asserting the
  per-frame read budget was bounded was a test that read the constant and
  compared it against itself, so changing the constant changed the test with
  it. That is not a weak test but a misplaced one: it moved into a
  `const _: () = assert!(...)`, and the harness learned that a build break
  counts as a catch.
- **One was a fault in the defect**, not in the test — it inserted a resync
  before the drain while leaving the after-drain pass in place, which changes
  nothing, because on entry there is nothing to resync yet.
- **One was a genuine no-op.** Deleting the resync loop's "skip buttons" guard
  is unobservable, because no button code has a set-1 scan code, so the guard
  is reached and redundant. It is recorded as a no-op, and the half that *is*
  checkable — that the keycode table never grows one — is now pinned by a test
  sweeping every code from `BTN_MISC` up.

The distinction is worth keeping, because the sweep's value depends on it. A
harness that reports "eight tests are weak" when five are, one is misplaced,
one is its own bug and one is not a bug at all is a harness whose output gets
skimmed.
