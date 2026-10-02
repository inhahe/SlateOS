## 564. Do Not Disturb hides the interruption, never the record — and the two switches that spell it are views of one mode

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop has a "Do Not Disturb" switch. Until now it moved
when you pressed it and changed absolutely nothing — notifications kept
arriving exactly as before. Turning it into a real control needed two answers.
First: when something is silenced, is it *thrown away* or merely *not
announced*? We keep it: it goes into the notification list as normal, it just
does not light up the little count on the tray bell. Second: the panel offers
two switches, "Do Not Disturb" and "Focus Mode", but underneath there is a
single setting with four positions (off / only important things / only alarms /
nothing at all). Two switches cannot spell four positions, so they are now
*read-outs* of that one setting rather than two settings of their own, and
turning either on turns the other off.

### The state of things before

`gui/desktop/src/focus_assist.rs` is 1,466 lines: four modes, per-app priority
overrides, automatic rules keyed on time of day and on the machine being in a
game or a presentation, a settings page and a tray pill. Nothing anywhere in
the tree constructed a `FocusAssistManager`. It was the second-largest orphan
island in the shell after `osd.rs` — see known-issues.md
`TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`.

`gui/desktop/src/notif_pane.rs` meanwhile had `QuickSetting::DoNotDisturb` and
`QuickSetting::FocusMode` as two plain `bool`s in its own private state, a
`QuickSettingToggled` event reporting each press, and — since nothing drained
`NotificationPane::events` — nobody listening. The switches were decoration.

### Decision 1: suppression governs attention, not the record

A notification that arrives while focus assist is silencing its app is stored
in the history exactly like any other, and marked
`Notification::silent`. `NotificationPane::attention_count` — the number the
tray bell's badge shows — skips the silent ones; `unread_count`, which is a
plain statement about the history, does not.

*Alternative considered: drop it.* Simpler by a field, and it makes "Total
Silence" literally true. It is wrong because Do Not Disturb is a request not to
be **interrupted**, not a request to be **lied to** about what happened while
you were not looking, and a user who turns it off afterwards has no other way
to ask for the missed hour back. Windows and macOS both keep the record for
this reason. Dropping would also silently defeat the summary
`FocusAssistManager::show_summary` promises, which can only say "you missed 4
things" if the 4 things exist.

*Alternative considered: store it but mark it `read: true`.* No new field, and
`unread_count` — which the bell already used — would then be the badge number
with no second counter. Rejected because `read` means *the user has seen this*,
and under Total Silence they demonstrably have not. Setting it would make the
pane open on a list with nothing highlighted, so the missed hour would be in
the history and invisible in it, which is the drop with extra steps.

The cost of the field is small and was measured rather than assumed: six sites
in the whole tree construct a `notif_pane::Notification`.

### Decision 2: the two switches are views of one mode

`DesktopShell::sync_quick_settings` is the only place the mapping is written:

| mode | Do Not Disturb | Focus Mode |
|---|---|---|
| `Off` | off | off |
| `PriorityOnly` | off | **on** |
| `AlarmsOnly` | off | **on** |
| `TotalSilence` | **on** | off |

A press is applied to the manager and then the manager is pushed back onto
*both* switches, so the pair can never disagree with what is actually happening
to arriving notifications. Turning either on replaces the mode outright, which
makes them mutually exclusive; turning either off means "stop suppressing",
including when the mode in force was the other switch's.

*Alternative considered: let the two booleans stay independent and derive the
mode from the pair.* That is the arrangement the pane already had, and it needs
a rule for "both on" — which has to collapse to one mode, so the write-back
then turns one of the switches off, and the user watches a switch they just
pressed flip itself back. A control that undoes your press is worse than one
that does nothing, because it looks like a bug rather than an absence.

**The one thing the pair cannot spell** is `AlarmsOnly` versus `PriorityOnly`:
both read "Focus Mode on". That is not a *lie* — focus assist is engaged and it
is not total silence, so no switch shows a state the system is not in — it is
merely coarse, and pressing it turns off whichever of the two was in force
rather than swapping one for the other. The distinction lives on
`focus_assist`'s own settings page, and in the meantime the tray glyph carries
it: the mode's icon is a bell, a struck bell, an alarm clock or a no-entry
sign, so the four modes are four different pictures in the tray.

### The tray glyph is the mode's, and the tint is not

`render_taskbar` draws `focus.effective_mode().icon()` in the bell slot, so one
24-pixel square says both "here is your history" and "here is why it has been
quiet" without the tray growing a second item — which would shuffle every
window button sideways, per §563.

The glyph is *not* tinted with `focus_assist`'s severity hues (blue / yellow /
red for the three engaged modes). Those are chosen and contrast-checked against
that module's own pill fill; on the taskbar the only pair this theme guarantees
legible is accent-on-background, pinned by
`the_taskbar_accent_contrasts_with_the_bar`. Reusing a colour across a
background it was never measured on is exactly the bug `focus_assist`'s own
module doc opens by describing. The changed glyph carries the mode; the accent
keeps its one existing meaning of "something is waiting".

### A consequence: the drain became mandatory, and moved to the outside

Acting on `QuickSettingToggled` at all meant the shell finally had to call
`NotificationPane::drain_events`, which nothing ever had — so the buffer grew
one entry per click for the life of the session.

The drain is deliberately placed *wrapping* `handle_mouse` and `handle_hotkey`
rather than on the branch that forwards an event to the pane. Every path out of
those functions can have touched the pane — the bell toggles it, a press
elsewhere on the bar dismisses it, `dismiss_popups` closes it with everything
else — and each reports a `Closed`. Draining only the forwarding branch bounds
the buffer for clicks *inside* the pane and leaks one per click that closed it,
which is the more common of the two; that is not a hypothetical, it is what the
first draft did and what `the_panes_event_buffer_does_not_grow` caught.

The wrapper is also what gives `Notification::action` its first reader in the
tree: a click on a card that names a program returns `ShellAction::Launch`
rather than being merely consumed.

**Still unwired, and recorded rather than faked:** `evaluate_auto_rules` needs
a wall clock and a "the foreground window went fullscreen" signal, neither of
which reaches the shell yet, so `effective_mode()` is today exactly the manual
mode. Wi-Fi, Bluetooth and Night Light are still decoration for the same kind
of reason — the first two are other daemons' state and the third is the
compositor's gamma ramp, over IPC the shell does not hold. Both are in
known-issues.md.

**Where it lives.** `gui/desktop/src/lib.rs` — `DesktopShell::focus`, `notify`,
`sync_quick_settings`, `apply_focus_toggle`, `apply_pane_events`, and the
wrappers around `handle_mouse` / `handle_hotkey`.
`gui/desktop/src/notif_pane.rs` — `Notification::silent`, `attention_count`,
`set_quick_setting`.
