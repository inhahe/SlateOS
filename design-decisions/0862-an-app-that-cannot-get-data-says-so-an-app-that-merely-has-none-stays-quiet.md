## 862. An app that *cannot* get data says so; an app that merely *has* none stays quiet

**Date:** 2026-09-18
**Lane:** C
**Decided by:** Claude (autonomous) — writing down a rule that thirteen apps
already follow, after nearly filing one of them as broken for following it.

**In short:** Several of our apps can't do their job yet, because the thing
they depend on — a network, a disk, a hardware list — isn't built. Those apps
put a sentence on screen saying so, instead of showing an empty list. The
difference matters because an empty list is not silent: a weather app with no
alerts looks like "no storms coming", and a partition tool with no disks looks
like "this computer has no drives". Both are claims about the world that
nothing actually checked. The rule is that an app which *cannot* look must say
it cannot; an app which looked and found nothing may simply show nothing.

**The rule.** At the top of the render path, before drawing any panel:

- **If the app has no route to its data at all** — no transport, no device, no
  permission — draw a short statement of that inability *instead of* the
  normal view, and return. `apps/weather` does this in `render_cannot_fetch`;
  `apps/partmanager` in its `disks.is_empty()` branch.
- **If the app can obtain data and currently has none**, draw the ordinary
  empty view. `apps/reminders`, `apps/calendar` and `apps/filesearch` are
  correct to stay quiet: you can add a reminder, add an event and run a search,
  so "empty" honestly means "you haven't yet".

  **Correction, 2026-09-18 (same day): `apps/notes` was named here and does
  not belong.** It cannot create a note -- `create_note`,
  `update_note_title` and `update_note_content` all have no production caller,
  and there is no import -- so its "No notes yet." falls in the *first* group
  and should say it cannot. The premise was checked against its `TextEntry`,
  which does handle typing, but only for search terms, tags and notebook
  names. **The check this rule needs is not "does the app accept text" but
  "can the user produce the thing the app is a list of"**, which is a question
  about one specific object and cannot be answered by looking for an input
  box. See `known-issues.md` -> `TD-C-NOTES-CANNOT-MAKE-A-NOTE`.

**Deny the inference, not just the data.** The best of these lines do not stop
at "no data"; they refuse the conclusion the blank space invites:

| App | The line |
|---|---|
| `weather` | "It cannot deliver severe-weather alerts either. **Silence here is not an all-clear.**" |
| `partmanager` | "No disks can be listed … **This is not a finding that you have none.**" |
| `devicemanager` | "This program cannot see the machine's hardware." |
| `netmanager`, `email`, `podcast`, `rssreader`, `torrent`, `videoplayer`, `mediaconvert`, `startupmanager`, `vpnmanager`, `renamer` | the same shape |

**Alternatives considered.**

- *Show sample data instead.* This is what the apps originally did —
  `weather` opened on invented forecasts for New York, London and Tokyo;
  `partmanager` listed a Samsung 970 EVO Plus down to a serial number. It was
  removed deliberately: invented data about the user's own machine or sky is a
  confident lie, and it is worse than emptiness precisely because it is
  legible.
- *Show an empty view everywhere and let the user infer.* Rejected above: the
  inference is wrong and sometimes dangerous.
- *Hide the app until its dependency exists.* Rejected — the layouts are real
  and tested, and hiding them loses the only signal that the work is waiting
  on a prerequisite rather than undone.

**Why this is written down now.** Thirteen apps had converged on it
independently and nothing made it binding on the fourteenth. It also cost me
a false finding: I reported `weather` as drawing a dashboard over an empty
model, having checked its constructor, its fields and its callers but never
its render path — where the early return lives. **A model check tells you what
an app has; only the draw tells you what an app claims**, and the two differ
exactly when the app is handling emptiness well. The check to run when asking
"can this app show anything" is therefore on the render entry point, not the
struct. See `known-issues.md` →
`TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY` (withdrawn).
