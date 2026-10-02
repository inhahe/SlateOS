## TD-C-THE-POWER-MANAGER-IS-A-DEAD-HALF-OF-A-LIVE-MODULE

**In short:** the desktop's power file contains a whole battery and sleep
manager -- idle timeouts, low-battery and critical thresholds, suspend and
hibernate transitions, a list of things blocking sleep, a log of what happened
and why -- and nothing anywhere creates one. The same file also draws the power
menu you click on, and *that* part is live, which is why no tool has reported
the dead half.

**Date:** 2026-09-15. **Lane:** C.

**The evidence.** `PowerManager` occurs 28 times, every one of them inside
`gui/desktop/src/power.rs`. `gui/desktop/src/lib.rs` uses the same file's
`render_power_menu`, `PowerMenuRow` and `PowerMenuStyle`, so the module has
real callers and is correctly absent from
`scripts/orphan-modules-baseline.txt`.

**Why no gate can see this, and it is not an oversight in any of them.**
`scan-orphan-modules.py` asks whether a *module* has a caller, and this one
does. A module can be half-dead and that scan will always answer for the whole
file. The finding needs a different question -- "which public items of this
module are reached?" -- which nothing here asks and which is a genuinely bigger
scan, since it needs per-item reachability rather than per-file.

**Inside the dead half, a second finding.** The low-battery *warning* was never
written. The file's own module doc advertises "Battery monitoring with
low-battery warnings", `PowerConfig::low_battery_pct` defaults to 20 and is
parsed and re-serialised by the config code, and `low_battery_warned` is
initialised, documented as "whether a low-battery warning has been shown this
discharge cycle", and reset when charging begins. Nothing ever sets it true.

Its sibling shows exactly what is missing. `critical_action_taken` has the same
initialisation and the same reset-on-charging, and at `power.rs:837` it has the
guard-once block that actually does the work:

    if !self.critical_action_taken && self.battery.is_critical(&self.config) {
        self.critical_action_taken = true;
        ...
    }

There is no such block for `low_battery_warned`, and no `is_low` to go with
`is_critical`. So one of two symmetric features was finished and the other left
as bookkeeping only.

**What the fix needs, in order.** The warning is not a `PowerAction` -- the
existing `check_battery_thresholds` returns one of those and a notification is
not one -- so it wants a `take_low_battery_warning()` in the style of
`SettingsState::take_notifications_change`, which the shell polls and turns
into a notification through `gui/notifications`. But that is the *second* step.
The first is giving `PowerManager` a constructor call at all: writing the
warning now would add a feature to a state machine nothing drives, which is the
defect this entry is about rather than a fix for it.

**One note on how this was found**, because the first attempt got it wrong.
`check-fields-written-never-read.py` reported `low_battery_warned` and I wrote
in its baseline that it was "probably a live bug -- either warns every tick or
never warns". Neither happens: nothing constructs the manager, so the
thresholds are never evaluated. I had inferred a severity from the shape of the
code without asking whether anything ran it. The corrected note is in
`scripts/fields-written-never-read-baseline.txt`; the question that was skipped
is the same one that makes every other finding in this file worth acting on.
