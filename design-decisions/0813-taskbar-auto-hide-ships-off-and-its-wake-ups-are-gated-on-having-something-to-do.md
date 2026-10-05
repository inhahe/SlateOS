## 813. Taskbar auto-hide ships off, and its wake-ups are gated on having something to do

**Date:** 2026-09-06
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The taskbar can now slide out of the way when you are not using
it. It is **off** unless you turn it on, even though the code that implements
it defaults to on -- a taskbar that vanishes is a big change to how a machine
behaves, and someone who did not ask for it would go looking for the setting
they did not knowingly change. And it only asks the desktop to stay awake while
it actually has something to do, so switching it on does not cost the machine
its ability to sit idle.

### Off by default, against the module's own default

`AutoHideConfig::default()` has `enabled: true`. That is a reasonable default
for a component whose entire job is to auto-hide, and a bad one for a desktop.
`ShellSession` therefore constructs it with `enabled: false` and drives it from
`AppearanceSettings::taskbar_autohide`, which is also false by default. Every
desktop this imitates ships auto-hide off; the surprise of a disappearing
taskbar is borne by a user who did not choose it, and the cost of the other
default is one checkbox.

The `false` is asserted, not merely written: a test flips the appearance
default to `true` and fails. Its first version did not -- it never moved the
pointer, so no hide was pending and it passed whichever way the default went,
which is the same "asserting absence where absence happens anyway" mistake as
the input-group test in design-decisions 812's neighbourhood.

### The wake-up gate, which is the part that could have been quietly wrong

`ShellSession::anything_moving` is the single condition that keeps an idle
desktop idle: false there means the loop parks with no bound at all. Auto-hide
has to appear in it, because a pending hide is measured in ticks — without them
the taskbar never hides. But appearing in it *unconditionally* would mean that
turning the setting on ends the unbounded park for ever, on battery, to watch a
taskbar that is already where it is going to stay.

So `AutoHideManager::needs_tick()` answers precisely: sliding and peeking yes,
a pending hide yes, `Hidden` and `Visible`-at-rest no. `Hidden` is a resting
state — what ends it is the pointer entering the trigger zone, which arrives as
an input event and wakes the loop by itself.

`needs_tick` is two mirrored conditions and could drift from the state machine
it describes, so the test does not check its arms: it takes a manager in every
reachable state, and wherever `needs_tick()` says no, asserts that ticking a
million milliseconds later changes nothing observable. That is the property,
and it cannot fall out of step with `tick` the way a hand-written list can.

### The surface moves, not the drawing

The panel is its own compositor surface, so hiding slides the *window*.
Translating the contents inside a stationary window would leave a strip of
transparent panel over the desktop swallowing clicks, and it would still be
there when the bar was fully hidden. `place_panel` is the only function that
adds the taskbar's drawn rectangle to auto-hide's offset; the resize path and
the auto-hide path both call it, because a bar drawn in one place and
hit-tested in another is invisible until somebody clicks.

### A sentinel that made the first millisecond special

Wiring this found a real bug in the module: `mouse_left_at: u64` used `0` to
mean "the pointer has not left", and zero is a real time — the session's clock
starts there. A pointer leaving the taskbar in the first millisecond of a
session was therefore ignored. It is now `Option<u64>`. The window is small and
the fix is small; what is worth recording is that it was found by a test that
pressed before advancing the clock, i.e. by writing the test the lazy way.

**Where it lives:** `gui/appearance/src/lib.rs` (`taskbar_autohide`, stored
under `taskbar:` rather than `effects:` because it is a behaviour);
`gui/desktop/src/taskbar_autohide.rs` (`needs_tick`, `set_config`, the
`Option` sentinel); `ShellSession::{sync_autohide, place_panel,
autohide_pointer, step_frame, anything_moving}`. Ten tests; three mutations
checked -- the default flipped to on, `needs_tick` claiming `Hidden` still
needs ticks, and the wiring removed.
