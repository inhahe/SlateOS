## 571. One shortcut table, not two: the configurable registry became the live one

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop had *two* lists of keyboard shortcuts. One was live
but hardcoded, so the user could not change it; the other could be edited and
saved to a file, but nothing ever read it — it was 2,431 lines of working code
wired to nothing. They disagreed about what several keys did. The live one has
been deleted and the configurable one is now the only list, which means the
shortcuts a user edits are the shortcuts that fire. Merging the two lists added
six shortcuts that previously existed only on paper (Super+I for settings,
Super+E for the file explorer, Super+L to lock, Ctrl+Alt+Delete for the task
manager, PrintScreen and Shift+Super+S for screenshots) and fixed two that were
listed and unreachable (Ctrl+Super+Left/Right for the previous/next desktop).

### The two tables

| | `DesktopAction::for_chord` (`lib.rs`) | `HotkeyRegistry` (`hotkeys.rs`) |
|---|---|---|
| Reachable from a keypress | yes | **no** |
| User can change it | **no** | yes — text config, load and save |
| Conflict detection | no | yes |
| Reference card renderer | no | yes (`render_settings_panel`) |
| Grab list for the compositor | a hand-written `const` | none |

Two tables meant two answers to "what does Super+E do", and the *live* answer
was the one nobody could edit. The wrong fix — and the one that looks cheapest —
is to consult the registry *and* keep the hardcoded chain as a fallback: that
leaves both lists alive, so they go on disagreeing, and adds a precedence rule
to disagree about as well.

### What was decided

**One table, and it is the configurable one.** `DesktopAction` is deleted.
`HotkeyAction` gained the ten actions only the live table had
(`RestoreOrMinimize`, `ToggleZoneOverlay`, `CycleWindowsBackwards`,
`ToggleOverview`, `PreviousDesktop`, `NextDesktop`, `ToggleStartMenu`,
`ToggleRunDialog`, `ToggleNotifications`, `DismissPopup`), and the shell holds a
`pub hotkeys: HotkeyRegistry` on the same terms as `pub rules` — public so a
settings panel can rebind without going through the shell.

**The defaults are the union of the two sets,** not the live set. The
alternative was to keep only what fires today and treat the orphan's extra
bindings as never-shipped. Rejected: those six were written as defaults, are the
chords every other desktop uses for the same things, and every one of them has
somewhere real to go — `HotkeyOutcome::launches` already carries command lines,
and `launcher::builtin_app_database` already names the programs. Declining to
ship them would have been throwing away working features to avoid changing a
grab list.

The cost is real and worth naming: six more chords are now taken from every
application on the desktop. Ctrl+Alt+Delete and PrintScreen are the ones a
program might plausibly want; both are chords a desktop is conventionally
expected to own, and the user can now unbind them, which under the old
arrangement they could not.

**Matching is on the whole modifier set.** The live table was once a chain of
`if`s that each tested only the modifiers it cared about, so `Super+Left`
(snap) was tested before `Ctrl+Super+Left` (previous desktop) and matched with
Ctrl held too — the virtual-desktop shortcuts had never once fired. A
`BTreeMap` keyed on the exact set makes that class of bug impossible to write.

**Two exceptions, applied on the way in as well as out** (`Hotkey::normalized`):

1. A *dedicated* key — `VolumeUp`, `VolumeDown`, `VolumeMute` — ignores
   modifiers entirely. A volume key that stops working because Shift happened
   to be down is a bug the user cannot see and could not describe.
2. A chord never carries the modifier its own key *is*. Whether the Super bit
   is set on the press of the Super key itself is the keyboard driver's
   business, and both answers are defensible; normalising it away means the
   start menu opens either way.

Normalising on *registration* is the part that matters: without it the registry
would accept a `Ctrl+VolumeUp` binding that no keystroke could ever reach.
Asking for one is now reported as the conflict it is.

**`Custom(String)` is deleted.** It named a string nobody read, so a binding on
it grabbed the chord from every application and then did nothing with it — the
key gone *and* the shortcut dead. A config line naming one is now refused, which
is the honest answer: the shell cannot perform an action it has no name for.

**The grab lists are derived, not hand-listed.** `GLOBAL_CHORDS` was a `const`
kept in step with `for_chord` by two tests, which works only while the table is
fixed at compile time. Now that a user can rebind, the set has to be computed
from what they bound: `HotkeyRegistry::global_chords()` walks the bindings, and
`Hotkey::chords()` expands each one into the exact chords a grab can name —
sixteen for a modifier-agnostic key, two for a key that is itself a modifier,
one otherwise. `conditional_chords()` is the same for `DismissPopup`, which is
claimed only while a popup is open: a key the shell holds unconditionally is a
key no window can ever see, and Escape closes a dialog far more often than it
closes the start menu.

### What is still missing

`BrightnessUp`/`BrightnessDown` are the only actions with nowhere to go. A
laptop's brightness pair sends no scancode — the firmware answers it over ACPI
or vendor WMI — so the two default bindings that used to exist bound Windows
virtual key codes that never arrive. The actions remain nameable so a user can
put them on a chord of their own; they consume the press and do nothing. See
`known-issues.md` → `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.

`render_settings_panel` draws the reference card and is still reachable from no
chord — the card exists, nothing opens it.

**Where it lives.** `gui/desktop/src/hotkeys.rs` — `Hotkey::normalized`,
`Hotkey::chords`, `HotkeyAction`, `HotkeyAction::is_conditional`,
`HotkeyAction::command`, `HotkeyRegistry::global_chords`/`conditional_chords`,
`install_defaults`; `gui/desktop/src/lib.rs` — `DesktopShell::hotkeys`,
`bound_action`, `run_desktop_action`; `gui/desktop/src/session.rs` — the startup
grab loop and `reconcile_escape_grab`.
