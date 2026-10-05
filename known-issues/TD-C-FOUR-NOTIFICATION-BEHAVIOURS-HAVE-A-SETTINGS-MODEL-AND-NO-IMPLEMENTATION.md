## TD-C-FOUR-NOTIFICATION-BEHAVIOURS-HAVE-A-SETTINGS-MODEL-AND-NO-IMPLEMENTATION

**In short:** the desktop can show you a notification and let you dismiss it,
and that is all it can do. It cannot put the banner somewhere else, make it
disappear on its own after a while, group several from one program together,
or forget old ones. A 2,526-line module in the shell described settings for all
four as though they existed. That module was deleted on 2026-09-15 because
nothing anywhere read it; this entry is what it knew, kept so the next person
builds the behaviour rather than a second copy of the settings for it.

**Date:** 2026-09-15. **Lane:** C.

**Narrowed 2026-09-29 -- two of the four behaviours exist now, neither as a
setting.** Notifications pop up and go by themselves (design-decisions §1447,
`gui/desktop/src/toasts.rs`): after 4, 6 or 10 seconds by priority, an urgent
one when closed -- the `AutoDismissDelay` this entry called the one a user
would miss first, with the time fixed by priority rather than chosen. They
appear in one place, the bottom-right corner above the taskbar -- a
`BannerPosition` with one value. And a notification's right-click menu
(§1450) turns its program off, which is `AppNotificationPrefs`' "enabled".
Still not built: `GroupingMode` (the pane groups by time, never by program)
and `HistoryRetention` (the pane keeps a notification until it is dismissed,
and forgets them all when the desktop restarts, so there is as yet no
history old enough to retire). The order below still holds for either:
behaviour first, then the field in `gui/notifsettings`, then the control.

**What was deleted.** `gui/desktop/src/notification_settings.rs` -- an island
under `scripts/orphan-modules-baseline.txt`, referenced by nothing but its own
`pub mod` line. It defined `BannerStyle`, `BannerPosition`, `GroupingMode`,
`NotificationPriority`, `AppNotificationPrefs`, `HistoryRetention`,
`AutoDismissDelay`, `NotificationConfig`, `NotificationHistoryEntry`,
`NotificationSettings` and a tabbed UI over them.

**Why deleting was right rather than porting.** Notifications already have a
live settings model with five consumers -- `gui/notifsettings`, read by
`apps/settings`, `gui/daywindow`, and the shell's own `focus_assist.rs` and
`notif_pane.rs`. It carries quiet hours, per-app rules and importance, and the
Settings app's Notifications page is built from it. So this was a second model
of a thing already modelled, which `design-decisions.md` 815 settles: a screen
you *open* lives in the Settings app and the shell's copy goes.

**The four concepts that had no counterpart, and why they are not lost work.**
`BannerPosition`, `AutoDismissDelay`, `GroupingMode` and `HistoryRetention`
exist in neither model now. Checked against `notif_pane.rs`, the live pane, at
3,809 lines: it has manual dismissal and grouping *by time* (today, yesterday,
this week, older) and no notion of where a banner sits, of a timer that removes
one, of collapsing several from one program, or of a retention policy. So these
were settings for behaviour that does not exist -- which is the thing the Mouse
page and the Startup Apps page each already refuse to ship in so many words:
a control that saves a value to a file, looks as though it worked, and changes
nothing. Building the behaviour is the work; the settings are the easy half and
should follow it.

**What the fix needs**, when someone does it: pick one of the four, implement it
in `notif_pane.rs` first, add the field to `gui/notifsettings` second, and give
it a control in `apps/settings`'s Notifications page third. In that order, so
that at no point does a setting exist that nothing reads. Of the four,
`AutoDismissDelay` is the one a user would miss first -- a notification that
never goes away on its own is the complaint the others are downstream of.
