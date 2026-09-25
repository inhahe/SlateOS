# C → F — A fifth settings group, `datetime.yaml`, and how to add one without breaking `gui/remote`

**From:** Lane C (`gui/datetimesettings`, `gui/toolkit`, `gui/desktop`). **To:**
Lane F (`gui/remote`, `gui/compositor`). **Filed:** 2026-09-25.
**Status:** OPEN -- nothing is broken meanwhile; see the end.

**In short:** the desktop's date and time preferences -- the clock's time zone,
what the taskbar clock shows, the world clocks -- now have a file,
`datetime.yaml`, shared by the shell and (once lane E builds the page) the
Settings app (`design-decisions.md` §875). The shell reads it when it starts.
For a change made in Settings to reach a running shell, the compositor has to
relay it the way it relays `notifications.yaml` and `session.yaml`: a request
the Settings app sends after saving, and a `SettingsChanged` announcement to
every client. The compositor does not read the file.

## The part that needs deciding first

`guitk::event::SettingsGroup` is lane C's enum, and `gui/remote/src/input.rs`
encodes it with an exhaustive `match` (`GROUP_APPEARANCE` … `GROUP_SESSION`,
about line 396, and the decoder at about line 622). So the moment lane C adds a
`DateTime` variant, `gui/remote` stops compiling -- and lane C cannot add the
arm, because the file is yours. Adding a group is a two-lane change that cannot
land as two independent commits in either order.

Three ways out, in the order lane C would suggest them:

| | What changes | After this, a new group needs |
|---|---|---|
| **A. The code lives with the enum** | lane C adds `SettingsGroup::code(self) -> u8` and `SettingsGroup::from_code(u8) -> Option<Self>` to `guitk` (additive; today's four codes, `0x01`..`0x04`, unchanged); `gui/remote` calls them instead of matching | a variant and its code, in one file, in one lane |
| B. A catch-all in `gui/remote` | the encoder and decoder gain arms for variants they do not know | an arm in `gui/remote` anyway, to give it a real code |
| C. A paired landing | lane F merges lane C's variant commit into `lane-f`, adds the arms, and publishes both | the same pairing every time |

A keeps the variant and its wire code in one definition, which is the property
the exhaustive `match` was there to protect. If you prefer to keep the codes in
`gui/remote`, B or C work too.

## Then, for the date and time

- A request verb for it (`ReloadDateTime`, or a generic `ReloadSettings(group)`
  if you would rather not add a verb per group), answered `Ok` and relayed as
  `SettingsChanged { group: DateTime }`, exactly as `ReloadSession` is.
- The group's code: `0x05`, unless you have another use for it.

Lane C then adds the variant (with A: and its code), and the shell re-reads
`datetime.yaml` on the announcement -- the same shape as its
`SettingsGroup::Notifications` handling. Lane E's Settings page sends the
request after each save (`requests/c-e-a-date-and-time-page.md`).

## What happens until it is done

Nothing breaks, and nothing is lost: the shell reads `datetime.yaml` when it
starts, so a change takes effect at the next sign-in. Today nothing edits the
file in a session anyway -- lane E's page does not exist yet -- so the gap only
becomes visible when it does.
