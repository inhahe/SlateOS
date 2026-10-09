# Operator answers -- ledger

One section per answers file, newest first. `check-lane-signals.py` reads the
`sha256` lines to tell a recorded file from a new one (`README.md` here says
how); the rows say where each answer went. **State** is `recorded` when the
answer is written up and nothing from it waits on the operator; anything still
waiting on the operator says so in bold, with the question that carries it.

A lane that processes a new file adds its section, fills in its own lane's
rows, and leaves the other lanes' rows as `relayed <date>` for those lanes to
fill in. Section numbers (§) are `design-decisions/` entries.

## open-questions-answers.2.txt

- **Written:** 2026-09-27, at the root of the integration tree.
- **Processed:** 2026-09-27 -- given to lane F's session, which relayed each
  answer verbatim to its lane; every lane recorded its own that day. This
  section was written 2026-10-09 by lane A, from the records each lane made.
- **Copy:** `operator-answers/2026-09-27-open-questions-answers.2.txt`
- **Content:** sha256 `cdc88ec66e88246fc5dde8f85c3acc7fd6535e9984e48dc53a1b97b8c7f8e4de`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| F-Q2 | F | §1332 (VP9: hardware codecs where the machine has them, a multithreaded CPU codec everywhere); §1339, §1340 (the codec itself) | recorded |
| F-Q1 | F | §1333 (AVIF: yes) | AVIF recorded; **HEIC waits on you**: `open-questions/F-Q1.md` answers your question about letting users replace the library, and asks A, B or C |
| A-Q14 | A | §971 | recorded |
| A-Q15 | A | §972 (both designs behind one switch, measured against each other; the measurement on real hardware noted) | recorded |
| C-Q26 | C | §1418 | recorded |
| C-Q25 | C | §1417 | recorded |
| C-Q24 | C | §1416 (the defaults; Ctrl+X, the zone overlay and the shortcut card explained); §1419 (Alt+Tab); §1420 (the redo tree, on the roadmap) | recorded |
| B-Q8 | B | §1042 (GNU's width table; your question about glyphs that disagree with it is answered there); §1224 (the terminal's width query) | recorded |
| C-Q11 | C | §974 (fast checks at push time); §979 (a check whose inputs have not changed reuses its last pass); §1534 (a way into a running guest) | recorded |
| C-Q15 | C | §1421 | recorded |
| C-Q16 | C | §1422 | recorded |
| C-Q17 | C | §1423 | recorded |
| C-Q18 | C | §1334 | recorded |
| C-Q20 | C | §1425 (one list of programs, nothing lost on the way); §1528 | recorded; **your `CLAUDE.md` suggestion waits on you as C-Q31** (`open-questions/C-Q31.md`: the tool is built, may `CLAUDE.md` make it a rule?) |
| C-Q19 | C | §1424 | recorded |
| C-Q21 | C | §1426 | recorded |
| C-Q22 | C | §1427 | recorded |
| C-Q23 | C | §1428 | recorded |
| B-Q9 | B | §1043 (genuine Oils becomes the default shell) | recorded |
| B-Q12 | B | §1046; your warning about the password export's CSV went to its owner as `requests/b-c-the-password-export-csv-must-survive-any-password.md` (the export quotes every field since 2026-10-01) | recorded |
| B-Q11 | B | §1045 | recorded |
| B-Q10 | B | §1044 | recorded |
| A-Q13 | A | §974; §973 | recorded |
| A-Q11 | A | §973 (every file has one owner; a gate refuses a file with none) | recorded |
| B-Q13 | B | §1047 | recorded |
| B-Q14 | B | §1048 | recorded |
| B-Q16 | B | option 1: §1005 and §1006 were already written | recorded |
| B-Q17 | B | §1049 | recorded |
| B-Q18 | B | §1050 (the ports you added are on the roadmap: Mono, Xonsh, Nushell on SlateOS, your WinDirStat fork, a debugger, your backup program; Chromium and YSH were there already) | recorded |
| B-Q19 | B | §1051 | **applied 2026-10-09** by lane A, at your instruction in its session: the rule is in the `CLAUDE.md` of all three accounts |
| B-Q20 | B | §1052 | recorded |
| B-Q21 | B | §1053 | recorded; **what SlateOS is for waits on you as B-Q23** (`open-questions/B-Q23.md`, with the catalogue of programs you asked for) |
| A-Q16 | A | §975; the conversions done on lane-a-wip 2026-10-09 | recorded |
| A-Q17 | A | §976 | recorded |
| A-Q18 | A | §977 | recorded |
| A-Q21 | A | §978 | recorded |

## open-questions-answers.txt

- **Written:** 2026-09-07, at the root of the integration tree.
- **Processed:** by 2026-09-12 (found then by accident, in `git status` output
  during an unrelated merge -- `open-questions/README.md`). This section was
  written 2026-10-09 by lane A, from the records each lane made.
- **Copy:** `operator-answers/2026-09-07-open-questions-answers.txt`
- **Content:** sha256 `18f6dc6cf257ea44ddfda50091f2431a1127887c6f17f1a45e02e4ac83eec8ac`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| Q46 | A | `open-questions-resolved/lane-a.md` | recorded |
| Q47 | A | `open-questions-resolved/lane-a.md` | recorded |
| Q56 | A | `open-questions-resolved/lane-a.md` | recorded |
| B-Q7 | B | §359; §1005 | recorded |
| B-Q8 | B | you asked for an explanation; it was rewritten and answered again on 2026-09-27 (above) | recorded |
| Q57 | A | `open-questions-resolved/lane-a.md` | recorded |
| C-Q6 | C | §815 | recorded |
| C-Q7 | C | §816 | recorded |
| C-Q8 | C | §817 | recorded |
| C-Q9 | C | §841 (you had no preference and asked two questions; both are answered there with measurements, and the call was Claude's) | recorded; its line in `open-questions-resolved/lane-c.md` is missing -- lane C's to add |
| A-Q1 | A | `open-questions-resolved/lane-a.md` | recorded |
| A-Q2 | A | `open-questions-resolved/lane-a.md` | recorded |
| A-Q3 | A | §914 | recorded |
| An account with no password | C | `open-questions-resolved/lane-c.md` | recorded |
| A-Q4 | A | `open-questions-resolved/lane-a.md` | recorded |
| A-Q5 | A | §919; your grep's features: §1008 | recorded |
| Which cipher, and who owns it? | C | §819 | recorded |
| A-Q6 | A | `open-questions-resolved/lane-a.md` | recorded |
| 2,288 of the 2,756 commands | B | `open-questions-resolved/lane-b.md` | recorded |
| C-Q10 | C | §826 | recorded |
| A-Q7 | A | `open-questions-resolved/lane-a.md` | recorded |
| The test machine cannot produce random numbers | B | `open-questions-resolved/lane-b.md`; the bell curve: §1047 | recorded |
