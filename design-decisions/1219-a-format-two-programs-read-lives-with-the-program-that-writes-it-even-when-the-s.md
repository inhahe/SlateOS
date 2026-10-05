## 1219. A format two programs read lives with the program that writes it, even when the second reader is in `gui/`

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous), with lane C -- lane C proposed where it should live and asked for the crate (C-Q19, §1424)

**In short:** the desktop's calendar popup drew event dots from a store of
its own that nothing ever filled, while the calendar program kept the user's
real events in `<config>/calendar/events.txt`. For the popup to show them it
has to read that file -- and the code that reads it lived inside the calendar
program, which nothing can depend on. It is now its own crate,
`apps/calendarstore`: the calendar's events, the file they are kept in and
iCalendar, moved out of `apps/calendar` unchanged. The popup (lane C's) reads
the file through it. That makes `gui/desktop` depend on a crate under `apps/`
-- the first dependency in that direction in the tree.

**Why the crate lives in `apps/`, not `gui/`.** The format is the calendar
program's: it decides what the file holds, changes it, and is the only
writer. A reader elsewhere depending on the writer's crate means a change to
the format is one change, made by its owner, that every reader picks up --
where a copy of the reader in `gui/` would be a second parser of the same file
that nobody remembers to update. Lane C weighed a request to itself against
this and chose this ("I'd rather the format sit with its owner than behind a
request to me").

**The rule it sets:** a shared *data-format* crate lives with the lane that
writes the data, whatever lane reads it. It is not a licence for `gui/` to
depend on applications: `calendarstore` holds no window, no drawing and no
program; it depends only on what `gui/desktop` already does (`guitk`,
`appearance`, `textfmt`, `settingsfile`, `safeio`).

**Alternatives:**
- *Move the format into `gui/`* (next to `settingsfile`) -- the owner of the
  format would then have to file a request for every change to its own file.
- *A copy of the reader in the desktop* -- two parsers of one file.
- *The desktop asks the calendar program* over IPC -- only works while the
  calendar is running, and the popup is exactly what is seen when it is not.
