# C -> F -- which keys are held when the desktop starts

**From:** Lane C. **To:** Lane F (`gui/compositor`, `gui/window` -- the input
devices and the control protocol).
**Filed:** 2026-09-27. **Status:** ✅ **DONE 2026-10-03 by lane F** -- both
changes; lane C's half is to call it (reply at the end).

**In short:** the operator decided (C-Q22, `design-decisions.md` §1427) that an
account set to sign in by itself does so with no pause, and that **holding
Shift while the machine starts** shows the login screen instead, to choose a
different account. The sign-in is built (`gui/desktop/src/autologin.rs`). The
key cannot work yet, for two reasons that are both in the compositor: it does
not know about a key that was already down when it opened the keyboard, and a
client cannot ask it which keys are down.

## What is asked

1. **Read the held keys when a device opens.** `EvdevInput::from_source` builds
   every `Stream` with `needs_resync: false` and starts from `Keys::default()`,
   so a key held since power-on is invisible until it is released and pressed
   again. `Stream::resync` already does exactly the reconciliation needed --
   `EVIOCGKEY`, then press what the device says is down -- it is only ever run
   after a `SYN_DROPPED`. Running it once at open (synchronously, so the state
   is right before the first client connects, rather than on the first poll)
   fixes this. It is also a bug on its own: a Shift held while the compositor
   starts is missing from the modifier state, so the first letters typed come
   out lower-case until Shift is pressed again.

2. **A control request that answers with the modifiers held now** -- say
   `GetHeldModifiers`, answered with the `Modifiers` the keymap already tracks
   (`Compositor::modifiers`) -- and an `EventLoop` method for it in
   `gui/window`. The shell asks once, as it starts, before deciding whether to
   sign in by itself. An event would not do: the answer is needed before the
   first frame, and a client that connects with a key already down receives no
   event for it.

A Shift held on any keyboard counts: "held" is across every device, as `Keys`
already is.

## What lane C does once it exists

`ShellSession::start` asks for the held modifiers and hands them to
`StartConditions` (`gui/desktop/src/autologin.rs`), which today is handed
`Modifiers::NONE` in `StartConditions::of_this_start`, with a comment pointing
here. The decision already reads Shift from what it is handed, and is tested
with Shift held, so nothing else changes.

## Also for lane A

A key held from power-on has to be *reported as down* by the kernel's
`EVIOCGKEY` by the time the compositor asks. PS/2 keyboards repeat a held key's
make code, so the driver sees it within the repeat delay; a USB keyboard's first
report carries the state. Lane C has asked lane A to confirm that half
(`requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`).

## If this is never done

An account set to sign in by itself always does, and the way to another account
is to log out afterwards -- which the operator named as sufficient ("The user
can always just use the menu to logout and login/switch accounts after it
autologs in, so it's not that crucial"). The Shift bug in item 1 remains either
way.

## Reply from lane F -- 2026-10-03

Both done.

**1. Keys held when a device opens.** `EvdevInput::from_source` now reads each
device's key state (`EVIOCGKEY`) as it opens it, through the same `resync` a
`SYN_DROPPED` runs. The keys found are handed over as presses by the first
poll, ahead of anything read since. The server polls input before it answers
any client, so the compositor's modifier state includes them before the shell
can ask. While they wait, the source's wake deadline is "now", so a loop that
waited before polling would not sit on them.
- Not repeated: a key held since power-on was never pressed at a moment this
  process saw, so no repeat is timed from it.
- Buttons are left alone, as after a drop: a held button is not a click.
- A device that cannot say what it holds still opens, with nothing held.
- This also fixes the Shift bug in your item 1 (first letters lower-case).
- Tests: seven in `gui/compositor/src/present/evdev/tests.rs`, "Keys already
  held when a device opens".

**2. `GetHeldModifiers`.** Control version 20 (tag `0x2B`), answered with the
new `ResponseBody::Modifiers` (response tag `0x06`, one byte in the
key-event encoding, so an undefined bit is refused).
- Client side: `oswindow::EventLoop::held_modifiers() -> Result<Modifiers, _>`.
- The answer is `Compositor::modifiers()`, which counts physically held keys,
  sides collapsed. A sticky-keys latch is not reported.
- It is a **shell request**: it goes through `require_shell`, like the window
  list and the key grabs. A program free to ask could poll the keyboard and
  learn when the user holds Shift in another program's password field, the
  key-state query X11 is remembered for. Today `require_shell` refuses
  nobody, so the shell gets its answer. The day it checks, an application
  asking gets `ClientError::Refused`, and `held_modifiers` treats that as an
  error, never as "nothing held".
- `oswindow::testing::TestDesktop` answers it from a new `held: Modifiers`
  field (default none), so the shell's start can be tested with Shift held.

**For `ShellSession::start`:** `events.held_modifiers()?` where
`StartConditions::of_this_start` now uses `Modifiers::NONE`. One thing to
decide on your side: a compositor that refuses is an error, and I would treat
it as "not held" for the sign-in decision rather than failing the shell's
start, since the operator called the key a convenience (§1427).

The kernel half, a key held from power-on being reported by `EVIOCGKEY`, is
lane A's, as you said.
