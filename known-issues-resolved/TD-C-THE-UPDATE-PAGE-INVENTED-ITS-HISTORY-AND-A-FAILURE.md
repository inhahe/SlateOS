## TD-C-THE-UPDATE-PAGE-INVENTED-ITS-HISTORY-AND-A-FAILURE -- FIXED 2026-09-15

**In short:** the Settings app's System Updates page told the user their
machine was up to date, listed four updates it claimed to have installed, and
said one of them had failed. None of it was real. There is no updater on this
system, nothing had been checked, and the four entries were written into the
source as constants. Fixed by making the page say what is true: this system
cannot update itself yet.

**Date:** 2026-09-15. **Lane:** C. **Found by:** working through
`design-decisions.md` 815's list of settings screens that exist twice -- this
was the live copy, not the dead one.

**What it did.** Four separate claims, in `apps/settings/src/main.rs`:

1. **"Check for Updates"** ran `self.checking_for_updates =
   !self.checking_for_updates` and nothing else. It flipped the label to
   "Checking..." and checked nothing; pressing it again flipped it back.
2. **"Your device is up to date"**, in green, drawn unconditionally whenever a
   check was not "running" -- an assurance about the user's machine from code
   that had never looked at it.
3. **Four update-history entries** hard-coded at construction, with
   Windows-style KB numbers (`KB5032100`), May 2026 dates, and one described as
   "Cumulative update for .NET runtime" on a system that has no .NET.
4. **Automatic updates, active hours, and two deferral sliders** -- settings
   stored in fields nothing else read.

**The entry that decided it.** One of the four was `UpdateStatus::Failed`: a
"Driver update for GPU" that had failed. A fabricated success is a lie about
nothing. A fabricated *failure* sends someone looking for a problem on their
own machine that never happened, with no way to discover it was never real.
That is a worse defect than the button, and it is the one that made this
urgent rather than untidy.

**Why it was not wired up instead.** An update needs a source. There are two in
the tree -- `userspace/pkg` and `kernel/src/fs/updatemgr.rs` -- and both are in
other lanes with no service between them and a GUI application. The version
string had the same problem: `os_version` was the constant
`"Slate OS 1.0.0 Build 2600"`, and 2600 is Windows XP's build number. Nothing
in this tree reports an OS version to a userspace program.

**The fix.** The page now states that the system cannot update itself, shows
"Last checked: Never" and "Available updates: Unknown", and says plainly that
no component reports a version yet. The dead state went with it: `UpdateStatus`,
`UpdateEntry`, `update_history`, `os_version`, `checking_for_updates`,
`auto_update_enabled`, `active_hours_start`/`_end`,
`defer_feature_days`/`_quality_days`, `ButtonId::CheckForUpdates`,
`ToggleId::AutoUpdate` and two `SliderId` variants -- 179 lines deleted against
54 added. Same treatment as the Sound page received on 2026-09-14, for the
reason the Mouse page states: a control that writes a value nothing reads is
worse than an absent control, because the absent one does not claim the setting
took effect.

**One thing worth knowing for the next page.** `cargo build` was clean with the
page rewritten and every field still present -- the fields are `pub` on a `pub`
struct and some are read by tests, so nothing warned. `cargo test` was what
caught the leftover (`SliderId::FIXED` is `[Self; 6]` behind `#[cfg(test)]` and
had to become `[Self; 4]`). A build passing is not evidence that removed code
left nothing behind, and `scripts/check-fields-written-never-read.py` exists
for exactly the residue a build cannot see.
