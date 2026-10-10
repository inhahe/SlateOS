## 875. The desktop's clock settings get a file, and its zone is the machine's unless the user chooses one

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The desktop clock's time zone, what the taskbar clock shows, and
the world clocks in the calendar were settings with nowhere to live: the model
sat inside the shell with no file behind it, and the only panel that could
change it was never on screen. So every desktop showed New York time -- a
default someone typed in -- and nothing could change it. They now have a file,
`datetime.yaml`, and a model shared by the shell (which obeys it) and the
Settings app (which will edit it). With nothing chosen, the clock shows the
machine's own zone, read exactly as `date` reads it.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where does the model live? | a crate of its own, `gui/datetimesettings`, like `notifsettings` and `inputsettings` | stay in `gui/desktop` | two processes use it; the Settings app must not link the shell to save a time zone. |
| The default zone? | the machine's (`TZ`, else `/etc/localtime`, else UTC) | New York, as before; UTC | New York was right for one zone and silently wrong for every other. The machine's zone is what `date` and every C program show, so the taskbar agrees with the terminal. On a machine with no `/etc/localtime` -- every SlateOS install today, since no tzdata ships and the installer does not write one -- that is UTC, which is at least the time the clock says it is showing. |
| How is "the machine's zone" read? | `tzrules::tz_source`, a new, additive function stating glibc's order once; the libc and `osh` are asked to use it too | a fourth private copy of the order | the order is subtle (empty `TZ` is UTC, unset is `/etc/localtime`, a rule is tried before a file, `..` is refused), and a copy that drifts is a clock that disagrees with `date`. **Corrected 2026-10-01 (§1458):** that was not glibc's order -- glibc tries the file first, and reads an empty `TZ` as the name `Universal`; `tzrules::tz_plan` states it now. |
| A zone the user chooses? | one of a built-in table of 21, each with its POSIX rule | any IANA name | the table's zones need no tzdata; a name the table does not know would need a file that is not installed. A hand-edited unknown name is kept, and read as the machine's zone -- not an invented offset. |
| How are world clocks stored? | a map keyed by label, in display order | a list | `yamldoc` reads sequences of scalars only, and a label is what a person edits; two clocks with one label could not be told apart on screen, so the model refuses the second. A reorder rewrites the block, since a map can only be put in order by writing it in order; otherwise each entry is updated in place and keeps its comments. |
| The NTP settings and "set the zone automatically"? | not in the file; they stay in the unreachable panel's own state | persist them | nothing obeys them: `userspace/ntpd` chooses its own polling, as the protocol requires, and nothing can detect a zone. A stored setting nothing obeys is the §856 failure. |
| When is the file read? | when the shell starts | also on change | a change reaching a running shell needs a fifth relayed settings group, and adding one breaks `gui/remote`'s exhaustive `match` -- lane F's file. The relay is requested (`requests/c-f-a-settings-group-for-the-date-and-time.md`); nothing edits the file in a session until lane E's page exists anyway. |

### Found and fixed along the way

The `desktop` binary started its session with `ShellSession::start`, which by
its own documentation does not read the appearance settings -- and nothing
else did. So from the day the binary was written (2026-09-13) a real desktop
started in the default theme, with no wallpaper and no widgets, and took up
the user's settings only when something happened to send `ReloadAppearance`.
`ShellSession::start_for_user` is `start` plus that read and a repaint; the
binary calls it, and a session test walks through it. Clock settings load
through the same call. `known-issues.md`
`TD-C-THE-DESKTOP-STARTED-WITHOUT-THE-USERS-APPEARANCE` has the details.

### Not done here

- The Settings page: `requests/c-e-a-date-and-time-page.md`.
- The relay: `requests/c-f-a-settings-group-for-the-date-and-time.md`.
- The libc and `osh` reading `TZ` through `tz_source`: `requests/c-bd-read-tz-through-tzrules-tz-source.md`.
- Writing `/etc/localtime` at install time, which is what would make the
  default more than UTC, stands where §311 left it: tzdata is not shipped yet.
