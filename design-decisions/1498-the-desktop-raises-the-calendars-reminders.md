## 1498. The desktop raises the calendar's reminders

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous), under the operator's rule in §1423 (what only the dead copy could do, the survivor takes) &middot; **Lane:** C

**In short:** an event in the calendar program can carry a reminder --
"fifteen minutes before", "a day before" -- and nothing on SlateOS ever
raised one. The calendar program keeps them in its file and says so in its
own code ("Nothing in this program raises a reminder"); the desktop's own
reminder code, which nothing called, went with its event store (§1497). The
desktop raises them now: when one is due, a notification from Calendar pops
up with the event's title, when it is and where, and stays on the screen
until it is closed; pressing it opens the calendar; right-clicking it offers
"Remind me again" in five, fifteen or thirty minutes or an hour. One that
fell due while the computer was off or asleep is raised when the desktop
next looks, if the event has not ended. Each is raised once, across a
restart too.

**Where:** `gui/desktop/src/reminders.rs` (`lead`, `Reminders::take_due`,
`next_due`, `snooze`, `when`, `body`, and the `reminders` data file),
`gui/desktop/src/lib.rs` (`raise_due_reminders`, `reminders_due`,
`next_reminder_in`, `snooze_reminder`, `notify_staying`, the notification
menu's "Remind me again"), `gui/desktop/src/toasts.rs` (`show_until_closed`),
`gui/desktop/src/session.rs` (raised and kept in `finish_batch`, woken for in
`arm_next_frame`, the calendar's file followed in `watch_settings`),
`gui/desktop/src/calendar.rs` (`SETTINGS_GROUP`, `watched_paths`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The desktop raises them** | the calendar program, while it runs; a service of their own | The desktop runs whenever someone is signed in, already reads the calendar, and holds the notifications; a reminder that needs the calendar open misses most of the time. A service of its own would need a way for programs to post notifications, which does not exist yet (C-Q32). | A session without this desktop raises none. |
| **Told of a save by the settings watch, which follows the calendar's file as the `calendar` group** | reading the file every minute; the calendar program telling the desktop | An event added minutes before its reminder must be heard of at once, and an idle desktop registers no wake-up at all (§812): being told costs nothing until something changes. The watch already follows files besides the settings folder (§1483), and the calendar program need not change. | The calendar's data is announced as a settings group, so every program's windows hear "calendar" changed; the calendar program, which has no settings file yet, ignores it. With the watch down a save is seen only when the calendar is looked at -- the popup opened, a widget's minute. |
| **A toast that stays until it is closed, at high priority** | the six or ten seconds other notifications stay; urgent | A reminder exists to be seen by someone who may have looked away for ten seconds. High priority's stripe rather than urgent's red: a dentist's appointment is not an alarm. | Several reminders at once fill the stack, and a newcomer waits for one to be closed, as behind urgent ones. |
| **One missed is raised late if its event is still on, looking back a day** | never raised late; raised for events over too | "Started five minutes ago" is still worth saying; a reminder of a meeting already over is noise. | An event that began more than a day before and is still on -- a conference -- does not have its missed reminder raised. |
| **Each raised once, remembered on disk by event and start** (`<data>/reminders`) | remembered while the desktop runs | A restart, or signing out and in, would raise every reminder of the last day again, closed or not. Keyed by the start, an event moved to another time is reminded of anew, and one added with less notice than its lead is raised at once. | One more file in the data folder. |
| **"Remind me again" on the notification's menu**, in 5, 15 or 30 minutes or an hour | buttons on the toast | The four the deleted code offered, where a notification's actions already are -- a right-click on its toast or its card. Toasts have no buttons. | Two presses and a right-click, where a button would be one press. |
| **An all-day event's reminder counts from its midnight**, as iCalendar has it | a time of day of the user's choosing | What the calendar program writes, and exports, is what is raised. | "A day before" an all-day event comes at midnight. |
| **The calendar's notification rule and focus assist apply**, as to any program | reminders breaking through Do Not Disturb | What the user set for Calendar decides; one who wants its reminders through Do Not Disturb makes it a priority program. | A reminder silenced by Do Not Disturb is only in the pane. |

**What is asked of lane E.** The calendar program's form offers no reminder
because nothing raised one; now something does
(`requests/c-e-the-calendar-can-offer-reminders-the-desktop-raises-them.md`).
