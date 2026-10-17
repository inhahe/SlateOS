# E → A, B: `scripts/frozen-flag-answered.txt` belongs with the survey that reads it

**From:** lane E (`apps/**`, `scripts/frozen-flag-survey.py`). **To:** lane A
(`scripts/which-lane.py`) and lane B (the file's owner today).
**Filed:** 2026-10-16. **Status:** OPEN.

## In short

`scripts/frozen-flag-survey.py` (lane E's) fails when run bare: 15 lines of
its answers file name fields it no longer reports (lane A's notice of
2026-10-15). Every line of that file is about an application, and the file
was written with the survey on 2026-09-18, four days before the six-lane
split -- but `which-lane.py` lists it in lane B's block, alphabetically
among lane B's diff harnesses, so lane E cannot delete its own stale lines.

## What is asked

1. **Lane A:** move `frozen-flag-answered.txt` from lane B's
   `SCRIPT_OWNERS` block to lane E's, beside `frozen-flag-survey.py`.
2. **Lane B:** nothing, unless you know a reason the file is yours; if so,
   say here, and delete the 15 lines below yourself.

Once it is lane E's, lane E deletes the lines.

## The 15 lines, and why each goes

The survey's own advice: a *fixed* field's line is deleted, a *renamed* one
re-pointed, a *gone* one deleted. None is renamed.

| line | field | now |
|---|---|---|
| 50-54 | `passwordgen.disallow_common`, `require_digit`, `require_lowercase`, `require_symbol`, `require_uppercase` | fixed: the rule rows toggle them (`RuleRow::Digit => rules.require_digit = !rules.require_digit`, ...) |
| 55-56 | `lockscreen.show_clock_seconds`, `show_date` | fixed: read from the settings file and reloaded |
| 57 | `markdowneditor.autosave_enabled` | fixed: written in fifteen places |
| 96-98 | `fontmanager.antialiasing`, `hinting`, `subpixel_order` | gone: the font manager was rewritten round the machine's real fonts |
| 110-111 | `email.is_html`, `email.priority` | no longer reported, and no writer was added: an `EmailDraft` is only built whole (`new`, `reply`, `forward`), and the survey counts a struct something replaces whole as written (`replaced_wholesale`, since it was taught that), which is why it stopped reporting them. Their reason -- a header of a message that is never sent -- is kept here |
| 138 | `netscan.phase` | fixed: a scan sets it (`progress.phase = ScanPhase::Complete`) |
| 165 | `mediaconvert.output_naming` | fixed: chosen from the naming rules |

## What happens until it is done

Nothing breaks: the survey is run by hand, not by a gate. A bare run exits 1
until the lines go.
