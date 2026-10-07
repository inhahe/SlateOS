## 1450. A notification's right-click menu turns its program off, from the pane or its pop-up

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** Right-click a notification -- its card in the list behind the
bell, or the pop-up while it is showing -- and a small menu offers "Turn off
notifications from Mail" and "Notification settings". Turning a program off
is the same switch the Settings app has for it: what that program sends
afterwards is still kept in the list, silently, so the same menu on one of
its cards offers "Turn on notifications from Mail". "Notification settings"
opens the Settings app on its Notifications page.

**What `design.txt` asks.** "A notification pane like on Windows -- option
for any notification to not show notifications from that application
again." The shell had the switch (the pane's per-program list, and the
Settings page, both writing a program's rule as `Silent`), but not on the
notification: the user had to go and find the program in a list.

**How it behaves** (`DesktopShell::open_notification_menu`):

| | |
|---|---|
| Rows | "Turn off notifications from *program*" -- or "Turn on ..." for one already off -- and "Notification settings" |
| Turning off | the program's rule becomes `Silent`, saved to `notifications.yaml`; its pop-ups go at once; its cards stay in the list |
| Turning on | the rule goes back to `Normal`, as the Settings switch does |
| Notification settings | `settings --page notifications`; the list closes, since its dimming would cover the window that opens |
| From the list | the menu opens over it and the list stays open |
| From a pop-up | the menu opens beside the pop-ups, ending where their surface begins, and holds them while it is up |
| Closing it | a press outside it, Escape, a row chosen, another menu opening -- and the press that closes it does nothing else |

**Beside the pop-ups, not on them.** The pop-ups have a surface of their
own above the menus' (§1447: "a toast pops up over an open menu"), so a
menu opened under the pointer on a pop-up would be drawn under the pop-up it
was opened from. It opens with its right edge where the pop-ups' surface
begins, at the pointer's height.

**Held while it is up.** The pointer holds the pop-ups (§1447); moving from a
pop-up to its menu leaves the stack, and the pop-up the menu is about would
time out while the menu is read. So the menu holds them too, and every way
it closes lets them go -- one function closes it, and a test walks each way.

| Alternative | For | Against |
|---|---|---|
| **The rule's `Silent`** (chosen) | one switch, the one Settings and the pane already write; turned back on from the same menu | a silenced program's notifications still fill the list |
| Drop a silenced program's notifications entirely | the list stays clean | nothing to turn it back on from but Settings; a program that matters can vanish without trace |
| Draw the menu on the pop-ups' surface | opens at the pointer | that surface would have to grow to hold it, and a press outside it would reach the window under it rather than close it |
| Put the pop-ups' surface below the menus' | opens at the pointer | reverses §1447: an open menu would hide a notification arriving |
| Only in the list | simplest | the moment a program annoys is the moment its pop-up is showing |

**Revisit if** programs come to register kinds of notification
(`design.txt`: "maybe have applications register different notifications so
that the user can modify or disable them individually") -- the menu would
then offer turning off *this kind* as well as the program.
