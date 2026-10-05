# C → E — a setting for how long notifications are kept

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

The notification pane now keeps its notifications when the desktop
restarts, so after a logout or a reboot the pane still shows what arrived
before it (`design-decisions/1468-…`). It keeps them for a week, then forgets
them. How long is the user's choice -- from none at all to a year -- but so
far the only way to choose is to edit `notifications.yaml` by hand. The
Settings application's Notifications page is where that choice belongs.

## What is asked

A row on the Notifications page -- a new section, say **History**, after
**Quiet hours** -- that sets `notifsettings::NotifSettings::history.days`:

- **The value:** `HistoryRetention { days }`, `0..=HistoryRetention::MAX_DAYS`
  (365). `HistoryRetention::default()` is 7. Saved as every other field on
  the page is, through the page's `NotifFile` (`history:` / `days:` in
  `notifications.yaml`); `NotifSettings::write_into` already writes it, and
  the desktop already obeys it.
- **The control:** a dropdown of a few lengths reads better than a number to
  type -- for example *Don't keep*, *1 day*, *1 week*, *1 month*, *3 months*,
  *1 year* (0, 1, 7, 30, 90, 365). A value written by hand that is none of
  them should still show, as itself ("12 days"), rather than snapping to a
  neighbour.
- **A line saying what it does**, as Quiet hours has one: for example
  "Notifications are kept for this long after they arrive, also across
  restarts. *Don't keep* also clears the ones kept so far."

## What happens on the desktop's side, already

- A lower value -- or *Don't keep* -- is honoured as soon as the file is
  saved: the desktop rewrites its history at once, keeping only what the new
  value allows (nothing, for *Don't keep*). Nothing more for the page to do.
- The notifications currently showing in the pane are not removed by a lower
  value until the desktop restarts or they are dismissed; only what is kept
  on disk changes at once. Say so in the line above if that seems worth it.

## If it is never done

Nothing breaks: every user keeps a week, which is the default; a user who
wants none kept has to edit a file to say so.
