## 1487. "Sleep the display" is a shortcut and a power menu row, and its wake locks the session unless the user says not

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the operator asked for "a key to put the monitor to sleep"
(§1416). Lane F gave the compositor a way to do it (`SleepDisplays`: the
screens go dark, and the next key, click or pointer movement wakes them
without reaching any window). The shell now offers it in two places: a
shortcut action, "Sleep the Display", which is not bound to any key until the
user binds one on the shortcut card, and a row in the start menu's power
menu, "Sleep the display", between Lock and Sleep. When the screens wake, the
session locks -- as if the lock shortcut had been pressed -- unless the user
has switched that off; and, as for every lock, a session whose account has no
password is never locked (§818).

**Where:** `gui/desktop/src/hotkeys.rs` (`HotkeyAction::SleepDisplay`),
`gui/desktop/src/power.rs` (`PowerChoice::SleepDisplay`),
`gui/desktop/src/lib.rs` (`ShellRequest::SleepDisplays`, `power_action`),
`gui/desktop/src/session.rs` (`display_sleep`, `notice_display_wake`),
`gui/desktop/src/idle_lock.rs` (`lock_on_display_wake`), and the built-in
icon theme's `video-display`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The wake locks by default** (`lock.on_display_wake`, `session` group, absent = yes) | never locking; or locking only if an idle delay is set | A display put to sleep on purpose is most often a machine being left, and the person who wakes it need not be the one who left -- macOS's default for a display turned off. The lock is the one every trigger queues, so 818 applies unchanged. | Someone who sleeps the display to watch a film from across the room types a password to come back -- until they switch it off. |
| **Asked once while asleep** | asking again for each press | The compositor answers every sleep at the wake; a second ask in the same sleep would only be a second answer to throw away. | -- |
| **A refusal is a notice** ("The display could not sleep") | nothing | A shortcut that does nothing would look broken. | Untested on the harness, which cannot refuse one request alone: asked of lane F beside its reply. |
| **A row in the power menu, between Lock and Sleep** | the shortcut alone | The roadmap and the request name both; the menu's order runs from the lightest change to switching off, and sleeping the screens is lighter than sleeping the machine. | One more row in the menu. |
| **A new built-in picture, `video-display`** (a monitor with a moon) | reusing `computer` or `system-suspend` | Each row's picture says which it is; `system-suspend` is the machine's sleep, a row away. | A name a few icon themes lack -- for those the built-in set draws it. |
