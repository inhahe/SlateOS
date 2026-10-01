# C -> F -- which keys are held when the desktop starts

**From:** Lane C. **To:** Lane F (`gui/compositor`, `gui/window` -- the input
devices and the control protocol).
**Filed:** 2026-09-27. **Status:** OPEN -- two small changes; lane C's half is
in and reads the answer the day it exists.

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
