## TD-C-THE-RETIRED-NOTIFICATION-PROGRAM-STILL-HOLDS-SEVEN-THINGS-THE-SHELL-LACKS

**Date:** 2026-10-16. **Lane:** C.
**Status:** OPEN.

**In short:** the operator chose the desktop shell as the one notification
system (C-Q32, design-decisions 1607), and the separate program
`gui/notifications` is to go -- but only once what it alone can do is in the
shell, as design-decisions 1423 asks of every retirement. Surveyed
2026-10-16, it holds seven things the shell's notifications
(`gui/desktop/src/notif_pane.rs`, `Notification`) do not. Nothing is lost
while it waits: nothing starts the program and no program can send to it.

**Where:** `gui/notifications/src/main.rs` (`Notification`,
`NotificationRequest`, `render_toasts`, `render_center`) against
`gui/desktop/src/notif_pane.rs` and the shell's pop-ups in
`gui/desktop/src/lib.rs`.

### What the shell lacks

| The program's | What it does | The shell today |
|---|---|---|
| `actions: Vec<NotificationAction>` | buttons on a notification, each an id and a label, the chosen one reported back | one `action: Option<String>`, run on a click |
| `progress: Option<u8>` | a bar on the pop-up, for a download or a copy | none |
| `NotificationRequest::Update` | a notification's title, body or progress changed in place, by id | none: a change is a second notification |
| `group_key` | a new notification with a key replaces the one with the same key rather than stacking | none |
| `persistent` | a pop-up that stays until it is dismissed, whatever its priority | only by priority |
| `icon_id` | the notification's own picture | none: the program's name only |
| `category` (message, email, download, update, error, reminder, ...) | what kind it is, for the list and for sounds | none |

The rest -- priorities and their time-outs, Do Not Disturb with a schedule,
per-program settings, the history grouped by program with groups folded,
read and unread, an unread count -- the shell has already, in its own form.

### The proper fix

Give the shell's `Notification` the seven, shaped as the freedesktop
notification specification shapes them -- `actions`, the `value` hint for
progress, `replaces_id` for an update and for a key that replaces,
`resident` for one that stays, `app_icon`/`image-path`, `category` -- since
ported programs send those, and the channel lane F is asked for
(`requests/c-f-a-channel-for-programs-to-send-notifications.md`) carries
them. Then delete `gui/notifications`, its line in the workspace and its
roadmap entry.
