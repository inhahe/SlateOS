# C → A: userspace cannot read the keyboard or the mouse, so the desktop cannot be typed at

**Filed:** 2026-08-21 (lane C)
**Blocks:** `known-issues.md` → `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`; roadmap §3.3
desktop bring-up on real hardware.

**In short:** as of today the SlateOS desktop draws correctly on a real screen —
lane C finished the DRM/KMS scanout path on 2026-08-21 — and there is no way for
a user to interact with it, because no userspace process can obtain a keystroke
or a mouse movement. The kernel *has* the data: `kernel/src/keyboard.rs` and
`kernel/src/mouse.rs` both service their IRQs and both keep event rings. What is
missing is a door from those rings to a userspace process. This is a request for
that door.

## Exactly what is missing

**Mouse: nothing at all.** `kernel/src/mouse.rs` assembles PS/2 packets into a
lock-free ring of `MouseEvent { buttons: u8, dx: i16, dy: i16, dz: i8 }` and
exposes `try_read_event()` / `read_event()` — to *kernel* callers. A tree-wide
search for `mouse` under `kernel/src/syscall/` and `kernel/src/fs/` finds only
accessibility *settings* (`inputa11y.rs`, `a11y.rs`: `mouse_keys`,
`mouse_speed`). There is no syscall, no device node, and no file that yields a
mouse event. The compositor's pointer is therefore not merely inaccurate — it
cannot move.

**Keyboard: only decoded ASCII, and only through a non-Linux syscall.** There
are two userspace entry points, `SYS_CONSOLE_READ_CHAR` and
`SYS_CONSOLE_TRY_READ_CHAR` (`kernel/src/syscall/handlers.rs:7915` and `:7956`),
and both hand back a single `u8` character. That is the wrong shape for a
display server in three separate ways:

1. **No release events.** `handle_scancode` computes `pressed` from bit 7 and
   uses it to update the modifier statics, then throws it away — only presses
   that map to a character ever reach the queue. A compositor cannot implement
   key repeat, cannot know a modifier is still held when the pointer moves, and
   cannot recover if a key goes down while the window loses focus and comes up
   while it does not.
2. **No keycode.** `InputEvent::KeyDown` in `gui/compositor/src/lib.rs:546`
   carries `scancode: u32` *and* `character: Option<char>`, because a shortcut
   is bound to a physical key and text is a separate question. From a `u8` the
   compositor can only reconstruct the second, and only for the keys that have
   a character at all — F1, the arrows, Home, Insert and the whole numeric
   keypad have none.
3. **It is not the Linux ABI.** Every other kernel door the compositor uses is:
   the scanout path in `gui/compositor/src/present/drm/` issues real
   `open`/`ioctl`/`mmap` numbers, deliberately, because the SlateOS target
   *is* `target_os = "linux"` and the same binary drives a real Linux graphics
   card. Input is the one place the compositor would have to special-case
   SlateOS — and it is also the place where every future port (SDL, GTK,
   Chromium, an X server, a Wayland compositor) will assume the Linux shape
   and find nothing.

## What lane C is asking for

**`/dev/input/event0` (keyboard) and `/dev/input/event1` (mouse), readable with
plain `read(2)`, yielding Linux `struct input_event` records.** That is:

```c
struct input_event {
    struct timeval time;   /* 16 bytes on x86-64: __kernel_time_t sec, suseconds_t usec */
    __u16 type;
    __u16 code;
    __s32 value;
};                          /* 24 bytes, no padding */
```

with the four event types a desktop actually needs:

| type | code | value | meaning |
|---|---|---|---|
| `EV_KEY` (1) | `KEY_*` (Linux keycode) | 0 up, 1 down, 2 autorepeat | a key or a mouse button |
| `EV_REL` (2) | `REL_X` (0) / `REL_Y` (1) / `REL_WHEEL` (8) | signed delta | pointer motion, scroll |
| `EV_SYN` (0) | `SYN_REPORT` (0) | 0 | end of one coherent packet |
| `EV_MSC` (4) | `MSC_SCAN` (4) | raw scancode | optional, but cheap and useful |

Mouse buttons are `EV_KEY` with `BTN_LEFT` (0x110), `BTN_RIGHT` (0x111),
`BTN_MIDDLE` (0x112) — that is the Linux convention and lane C would rather
follow it than invent a parallel one.

Three properties matter as much as the shape:

* **`EV_SYN` after every packet.** A PS/2 mouse packet carries dx, dy and the
  button state together; delivered as three unsynchronised events the pointer
  can be seen mid-update. `EV_SYN` is what tells the compositor a group is
  complete, and it is why the Linux protocol has it.
* **`O_NONBLOCK` must work, and blocking `read` must be interruptible.** The
  compositor polls input once per frame from the same thread that composites;
  a `read` that blocks stalls the display. (Lane A already fixed exactly this
  class of bug for `SYS_CONSOLE_READ_CHAR` — `BUG-CONSOLE-READ-UNINTERRUPTIBLE`,
  2026-08-21 — so the concern is understood.)
* **A bounded ring that drops oldest, and says so.** `mouse.rs` already has a
  128-entry ring. A compositor that is 200 ms late must not then replay 200 ms
  of stale motion; Linux signals this with `EV_SYN`/`SYN_DROPPED`, and lane C
  will handle that code if it is sent.

## What this needs on the kernel side, concretely

Lane C has read the drivers and believes the work splits in three, in rising
cost:

1. **A raw keyboard ring next to the ASCII one.** `handle_scancode`
   (`keyboard.rs:289`) already computes `(extended, code, pressed)` before it
   decides whether there is a character; the ask is only that it *also* push
   `(keycode, pressed)` into a second ring. The existing ASCII path must keep
   working unchanged — the shell reads it. This is the piece that cannot be
   done in userspace at all, because the information is discarded inside the
   ISR.
2. **A device node per ring** under `/dev/input/`, with `read` returning whole
   `input_event` records (never a partial one), `O_NONBLOCK` honoured, and
   `poll`/`epoll` readiness if that is cheap where the tty layer already does
   it. `kernel/src/fs/devfs.rs` is where the other nodes live.
3. **Scancode → Linux keycode translation.** Set 1 scancodes are not Linux
   keycodes, and the table is ~128 entries plus the `0xE0` extended set. Lane C
   is happy to own this table if lane A would rather ship raw scancodes as
   `MSC_SCAN` and nothing else — but note that doing it in the kernel is what
   makes the node usable by *any* client rather than by our compositor only.

**If item 3 is the sticking point, ship 1 and 2 with raw set-1 scancodes as the
`EV_KEY` code and say so.** Lane C can translate. An honest device that is
almost-evdev beats no device by a very large margin, and the difference is one
table that can be moved later.

## What lane C will do with it

`gui/compositor/src/present.rs` already has the seam: `Present::input()`
returns `Vec<InputEvent>` and `DrmScanout` currently inherits the default
implementation, which returns nothing. Wiring these devices in is a new source
alongside the scanout, tested on the host against a fake exactly the way
`KmsSys` and `CardSource` are — so the translation logic (packet assembly,
`EV_SYN` grouping, keycode mapping, button state) is covered on a machine with
no PS/2 controller, and only the `open`/`read` pair is target-only.

## Interim

None worth having. Lane C considered driving the compositor from
`SYS_CONSOLE_TRY_READ_CHAR` so that at least typing works; it decided against
it, because a text-only input path with no key release and no pointer would need
its own event synthesis, its own focus rules and its own tests, all of which get
deleted the moment the real device lands. That is band-aid accumulation of the
kind `CLAUDE.md` names explicitly. Lane C would rather leave
`TD-COMPOSITOR-HAS-NO-LOCAL-INPUT` open and honest, and pick up other work.

## Priority

Not urgent this week — QEMU boots headless and the compositor serves remote
clients over `guiremote`, which is how the desktop is exercised today. It
becomes the single blocking item the moment anyone tries to *use* SlateOS on
real hardware, because at that point the machine draws a desktop and ignores
the keyboard.
