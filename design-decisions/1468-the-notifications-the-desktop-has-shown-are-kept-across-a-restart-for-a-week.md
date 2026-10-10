## 1468. The notifications the desktop has shown are kept across a restart, for a week unless the user says otherwise

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** Until now every notification in the notification pane was
forgotten whenever the desktop restarted: after a logout or a reboot the
pane was empty, so "what did I miss?" could not be answered across one. Now
the desktop writes the pane's notifications to a file whenever they change,
and puts them back when it starts -- read or unread as they were left, and
without popping any of them up again. A notification older than a week is
forgotten. How long is a setting, `history.days` in `notifications.yaml`,
from 0 to 365 days; 0 keeps nothing at all, for a user who would rather what
a notification said were never written down.

**Where:** `gui/desktop/src/notif_history.rs` (the file and what is kept),
`gui/desktop/src/notif_pane.rs` (`NotificationPane::revision`, `restore`),
`gui/desktop/src/lib.rs` (`load_notification_history`,
`notification_history_dirty`, `save_notification_history`),
`gui/desktop/src/session.rs` (read at start, after the rules; written from
the pump), `gui/notifsettings/src/lib.rs` (`HistoryRetention`),
`gui/settingsfile/src/lib.rs` (`data_dir`, `load_data`, `store_data`); tests
in `gui/desktop/src/notif_history_tests.rs` and `gui/desktop/src/session/tests.rs`.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **In the user's data directory** -- `notifications` in `$XDG_DATA_HOME/slateos`, else `~/.local/share/slateos` | beside `notifications.yaml`, in the configuration directory | It is a record of what happened, not something the user chose: someone who copies their configuration to another machine, or resets it, should neither carry nor lose their notifications with it. Installed themes already lived there; `settingsfile` now finds that directory for both, and writes there as atomically as it writes a setting. | Two directories to know about rather than one. |
| **A line of text a notification**, eight tab-separated fields, the four text fields percent-encoded by §426's rule, under a first line naming the format and its version | YAML, as settings are; JSON lines, as logs are | Nobody edits this file, so what makes settings YAML -- comments and hand edits kept -- buys nothing, and a document format turns one damaged byte into a refused file where a line format loses one notification. The encoding is the one the tree already has for "any bytes, on one line" (`pathcodec`), so a tab or a line break in a title comes back as it went, and the desktop takes on no JSON parser for one file. | A third text format beside YAML and JSON lines, which `jq` cannot read. Still text, and the version line lets a later format replace it without being misread. |
| **Written whenever the notifications change** -- one arriving, read, dismissed, cleared -- at the session's next pump | written only when the desktop exits | A crash or a power cut loses nothing, and most desktops never exit cleanly. A counter on the pane (`revision`) says when there is anything new, so an idle desktop writes nothing. | A small write per change, a few kilobytes at most (50 notifications). |
| **A week by default; 0 to 365 days** | until dismissed, however long | A notification is news: a month-old one is clutter, and what notifications carry -- a message's first line, a calendar entry -- is often private, so it should not accumulate on disk unasked. A week covers a weekend away. | A notification left for later disappears after a week unless the user raises the setting. |
| **Put back without popping up, read or unread as left** | shown again as new | They were shown already; popping them up at every login would be noise, and the unread mark already says what is new. | -- |
| **Under fresh ids** | the ids they had | Ids are the pane's own and its counter starts again with the desktop; nothing outside the desktop holds one across a restart. | -- |
| **A failed write is said once**, as the start menu's and the Run box's are (`report_save`), and tried again at the next change | said at every attempt; tried at every pump | A full disk does not become a notice a frame. | Until the next change, the file is older than the pane. |

**A change of retention is honoured at once.** Lowering it -- or setting it
to nought -- rewrites the file as soon as the Settings application saves the
new number, not at the next notification: a user who has just said "keep
nothing" should not find their notifications still on disk until something
else happens.

**What is not done here.** The number has no control in the Settings
application yet; it is read from `notifications.yaml` and can be set there by
hand. The control is lane E's, asked for in
`requests/c-e-a-setting-for-how-long-notifications-are-kept.md`.
