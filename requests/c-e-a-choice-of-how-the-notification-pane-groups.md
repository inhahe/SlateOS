# C → E — a choice of how the notification pane groups its notifications

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-07. **Status:** OPEN.

## In short

The notification pane now groups each program's notifications under its
name, the program heard from last at the top, its newest three shown and
"Show N more" for the rest -- as Windows' pane does
(`design-decisions/1495-…`). The old arrangement, by when they came (Today,
Yesterday, This Week, Older), is still there for a user who prefers it, but
the only way to choose it is to edit `notifications.yaml` by hand. The
Settings application's Notifications page is where that choice belongs.

## What is asked

A row on the Notifications page -- beside the history row asked for in
`requests/c-e-a-setting-for-how-long-notifications-are-kept.md`, say -- that
sets `notifsettings::NotifSettings::grouping`:

- **The value:** `notifsettings::Grouping`, `ByProgram` (the default) or
  `ByTime`; `Grouping::ALL` lists them in the order to offer them, and
  `Grouping::label` names each for a person ("By program", "By time").
  Saved as every other field on the page is, through the page's
  `NotifFile` (`pane:` / `grouping:` in `notifications.yaml`, `program` or
  `time`); `NotifSettings::write_into` already writes it.
- **The control:** a two-way choice -- a dropdown or two radio buttons --
  titled, for example, **Group notifications**.
- **A line saying what it does**, as the page's other rows have one: for
  example "By program shows each program's newest three, with the rest a
  click away."

## What happens on the desktop's side, already

- The desktop reads the file when it changes and regroups the pane at
  once, open or not (`DesktopShell::poll_notification_rules`).
- A spelling it does not know -- a later desktop's grouping -- is read as
  the default, so the page may offer only these two.
