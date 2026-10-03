## TD-COMPOSITOR-HAS-NO-LOCAL-INPUT (lane C, 2026-08-21)

**In short:** on SlateOS the desktop draws correctly and the keyboard and mouse
do nothing. Frames go out; nothing comes back in.

**What.** `DrmScanout` implements `Present::show` and inherits the default
`Present::input`, which returns an empty `Vec`. `Compositor::handle_input` and
`route_input` are complete and correct and have no source on this platform, so
they are reachable only from tests and from the Win32 harness — which is exactly
the state `TD-COMPOSITOR-HAS-NO-SCANOUT` described for the output half before it
was closed, now surviving in the input half alone.

**What is missing.** A device to read. The kernel has a PS/2 and USB keyboard
path for the console, but exposes nothing an unprivileged display server can
open and poll for scancodes and mouse deltas — no `/dev/input/event*`, no
equivalent. Wiring this needs (a) lane A to expose a readable device or an IPC
endpoint carrying input events, then (b) a `DrmInput` here that translates its
reports into `InputEvent`s, most likely alongside `DrmScanout` rather than
inside it, since the card and the keyboard are different devices.

**Severity.** High as a *blocker*: an OS whose desktop cannot be typed at is not
a desktop. Low as a *defect*: everything that exists is right, `keymap.rs`
already turns scan-code-set-1 into characters, and the fix is additive.

**Filed to lane A?** Not yet. Do it once the shape of (a) is worth asking for —
"expose input somehow" is not a request lane A can act on. Determine first
whether the kernel's existing keyboard path can be given a second consumer, or
whether this wants a new device; that reading is lane C's to do before filing.

**Update 2026-08-21 — that reading is done, and it is now filed:**
`requests/c-a-userspace-cannot-read-the-keyboard-or-the-mouse-at-all.md`. What
the drivers turned out to say:

* **The mouse has no userspace door of any kind.** `kernel/src/mouse.rs` keeps a
  128-entry lock-free ring of `MouseEvent { buttons, dx, dy, dz }` and exposes
  `try_read_event()`/`read_event()` to *kernel* callers only. Nothing under
  `kernel/src/syscall/` or `kernel/src/fs/` reads it; the only matches for
  "mouse" there are accessibility *settings* (`mouse_keys`, `mouse_speed`). So
  the pointer cannot move, as opposed to moving badly.
* **The keyboard's existing consumer cannot be reused, because the information
  is destroyed before the queue.** `handle_scancode`
  (`kernel/src/keyboard.rs:289`) computes `(extended, code, pressed)`, uses
  `pressed` to update the modifier statics, and then keeps only the ASCII
  character — for the keys that have one. `SYS_CONSOLE_TRY_READ_CHAR` therefore
  hands back a `u8`: no release events, no keycode, nothing for F-keys, arrows,
  Home/Insert or the keypad. Giving it "a second consumer" would hand that
  consumer the same lossy `u8`. The raw push has to happen inside the ISR, which
  is lane A's tree.
* **The ask is Linux `struct input_event` on `/dev/input/event0`/`event1`**, with
  `EV_KEY`/`EV_REL`/`EV_SYN`, `BTN_LEFT`-style button codes, and `O_NONBLOCK`.
  Every other kernel door this compositor uses is the Linux one — the scanout
  path issues real `open`/`ioctl`/`mmap` numbers on purpose — and input is also
  where every future port (SDL, GTK, Chromium, an X server) will look first.
  The request explicitly offers to own the set-1 → Linux-keycode table in lane C
  if that is what makes the kernel half small enough to land.

**No interim is being built, deliberately.** Driving the compositor from
`SYS_CONSOLE_TRY_READ_CHAR` would give typing and nothing else, and would need
its own event synthesis, focus rules and tests — all of which get deleted when
the real device lands. That is exactly the band-aid accumulation `CLAUDE.md`
names. This entry stays open and honest instead.

**Update 2026-08-24 — both halves are built. Closed here; one dependency
remains, and it is lane B's.**

Lane A landed the kernel half on 2026-08-21
(`requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`): real
`/dev/input/event0` and `event1`, 24-byte `input_event` records, `EV_KEY`/
`EV_REL`/`EV_MSC`/`EV_SYN`, `SYN_DROPPED` with a per-fd cursor, the full
`EVIOC*` interrogation sequence, and — more than was asked for — the set-1 →
Linux-keycode translation done kernel-side, so the offer above to own that table
in lane C was not taken up.

Lane C has now landed the compositor half:

* **`gui/compositor/src/present/evdev.rs`** and its `uapi`/`sys` submodules —
  the client. Split three ways for the same reason `present/drm.rs` is: `uapi`
  is the wire format and the keycode table with no fds and no `unsafe`, `sys` is
  the four syscalls behind a trait, and the parent file holds every decision and
  is generic over that trait. Every bug this module can have is a protocol or
  policy bug, and none of them need a keyboard to find — but all of them would
  be invisible if the module were behind `#[cfg(target_os = "linux")]`, because
  the machine this tree is compiled on is not Linux.
* **`present::Paired`** — the answer to the "alongside `DrmScanout` rather than
  inside it" question this entry raised. `Paired<S, I>` is a `Present` made of a
  screen and an `InputSource`: frames and monitors go to the screen, events come
  from the source, and the screen alone decides when the session ends (a
  keyboard being unplugged is not a reason to end it). It also keeps
  `InputSource::set_bounds` current, which is how monitor hotplug reaches the
  pointer. `Server::run_with` is unchanged.
* **`main.rs`** pairs the two on the SlateOS target, and prints each node that
  opened with its `EVIOCGNAME`.

Three things the kernel deliberately does not do are done here, and are listed
in the module docs so nobody later builds a second one: key repeat (synthesised
from key-down/up timing at the user's own `input.yaml` delay/interval, capped
per tick, modifiers excluded, and hardware autorepeat passed through in a way
that pushes our timer out of the way rather than doubling with it); absolute
pointer position (integration, the user's speed and acceleration profile,
clamping, and the sub-pixel remainder without which a 0.25× speed setting is
immovable); and `SYN_DROPPED` resync via `EVIOCGKEY`, reconciled *both* ways —
releasing what is no longer held, which is the stuck-Shift bug, and pressing
what is held and was missed, which is the Ctrl that stops making shortcuts.

**Tested:** 66 tests across `evdev/tests.rs`, `uapi.rs` and `present.rs`, all
driven by a fake device that scripts a real byte stream, so none of them need
the capability. Proved non-vacuous by `scripts/reintro-evdev.py`, which puts 54
one-line defects back one at a time — `REL_Y` counting upwards, the scroll axes
crossed, `packet.scan` read instead of taken, the per-device check dropped from
resync, `MSC_SCAN` consulted before the keycode table — and records the test
that has to name each one.

**The one dependency left is not lane C's and cannot be made so.**
`open("/dev/input/event*")` returns `EACCES` until the compositor holds a
`(ResourceType::InputDevice, 0, Rights::READ)` capability, which is obtainable
only at spawn or by inheritance from an ancestor — init / the service manager,
lane B's tree, filed as
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`.
`EvdevError::Denied` is its own variant so that this reports itself in those
words rather than as a missing file, and the compositor prints the fix and the
request filename on the way past, then carries on without local input. **Nothing
here changes when that grant lands** — the code path is the one the tests
exercise, with `sys::Devices` in place of the fake. Reply to lane A:
`requests/c-a-the-compositor-now-reads-your-evdev-nodes-and-is-waiting-only-on-the-capability.md`.

**Tracked separately, and since closed:** a `ReloadInput` request reached
`Compositor::reload_input`, which adopted only `double_click_ms` — it had no way
to reach `EvdevInput::set_settings`, so a pointer-speed change made in Settings
did not take effect until the next login. See
`TD-C-A-POINTER-SPEED-CHANGE-DOES-NOT-REACH-THE-POINTER`. *(Closed later the
same day: `Server::run_with` now polls `Compositor::input_settings` and pushes
any change through `Present::reload_input` into the device —
`design-decisions.md` §548.)*
