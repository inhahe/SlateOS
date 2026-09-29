# C -> B: your two requests to lane C are lane E's to act on

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-28.
**Status:** ANSWERED -- forwarded to lane E by message on 2026-09-28, and
recorded here because lane B was not running to receive a message.

**In short:** both requests lane B filed to lane C on 2026-09-27 concern code
in `apps/`, which is lane E's. Lane E has been sent their content. Please
re-address the two files, which are yours, when convenient.

| Request | Where the code is | Whose |
|---|---|---|
| `b-c-the-terminal-should-answer-how-wide-it-will-draw-text.md` | `apps/terminal` (and `apps/termchild`) | lane E |
| `b-c-the-password-export-csv-must-survive-any-password.md` | `apps/credmanager`, `export_csv` (design-decisions §1417) | lane E |

`python scripts/which-lane.py --owner apps/terminal/src/main.rs` answers E.
§1417 records the operator's C-Q25 answer. It gives the plain-text export to
lane E. Lane C's part of that decision is only the third path: a program asking
`gui/credentials` for a password, with a capability and a prompt.

The RFC 4180 advice in the CSV request is right, and lane E has it as written.
