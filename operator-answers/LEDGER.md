# Operator answers -- ledger

One section per answers file, newest first. `check-lane-signals.py` reads the
`sha256` lines to tell a recorded file from a new one (`README.md` here says
how); the rows say where each answer went. **State** is `recorded` when the
answer is written up and nothing from it waits on the operator; anything still
waiting on the operator says so in bold, with the question that carries it.

A lane that processes a new file adds its section, fills in its own lane's
rows, and leaves the other lanes' rows as `relayed <date>` for those lanes to
fill in. Section numbers (§) are `design-decisions/` entries.

## open-questions/answers.2.txt

- **Written:** 2026-10-09 16:15, as `open-questions/answers.2.txt` in the
  integration tree: the operator's reply to lane A's report (`claude-answers.txt`,
  the next section), and five questions of the operator's own -- the same five
  are in the operator's `todo2.txt`.
- **Processed:** 2026-10-09 by lane B: copied; every lane with answers in it
  sent a notice; the operator's five questions answered in the rows below.
- **Copy:** `operator-answers/2026-10-09-open-questions-answers.2.txt`
- **Content:** sha256 `5e94dc49f15ffd10ca0eaa0053a1cd35cf84524c809fbeeee00853e2d252ad6a`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| C-Q34 | C | -- | relayed 2026-10-09 (A) |
| C-Q29 | C | -- | relayed 2026-10-09 (A) |
| F-Q7 | F | -- | relayed 2026-10-09 (A) |
| F-Q8 | F | -- | relayed 2026-10-09 (B) |
| F-Q9 | F | -- | relayed 2026-10-09 (B) |
| F-Q10 | F | -- | relayed 2026-10-09 (A) |
| F-Q6 | F | -- | relayed 2026-10-09 (a note to make: turning on remote desktop also sets up whatever is still missing of a dynamic-DNS name and a Let's Encrypt certificate) |
| "Yes, commit the MIT license." | A | -- | relayed 2026-10-09 (lane A asked; the untracked `LICENSE` is at the integration tree's root) |
| "...reform some files ... has that been done?" | A | §1530 | **Answered here.** Mostly yes, on 2026-10-02 (§1530, lane A's five steps, which you approved): known issues, design decisions and the question queue are one file per entry; closed issues and finished roadmap blocks live apart from open ones (`known-issues-resolved/`, `roadmap-done.md`); and `scripts/docsearch.py` searches all of them through a database (SQLite), by words and by meaning. Not done: `todo.txt`, still one file of 59,315 lines, and `roadmap.md`, still 6,523 lines of open items. `todo2.txt` is your own file, which no session edits. The documents' tooling is lane A's (§973): relayed 2026-10-09 for `todo.txt` |
| "...configuration abilities like those in Process Lasso...?" | B | `open-questions/B-Q26.md` | **Answered, with one question back to you: B-Q26.** Most of it is planned: a priority remembered for each program (your proposal of 2026-09-26); processor time, memory and disk speed kept back for the desktop, so a runaway program cannot make it sluggish (what Process Lasso's ProBalance is for, done ahead of time instead of after); workload profiles; disk priorities; per-program limits. Not planned: remembering which processors a program may use, a cap on one program's processor use, and switching profile while a program runs -- B-Q26 asks whether to add them |
| "...hotkeys ... pgup, pgdn, home, end, ctrl+home, ctrl+end" | C | §1416; `roadmap.md`, the `[C]` item "Page Up, Page Down, Home, End, Ctrl+Home and Ctrl+End wherever ..." | **Answered here: yes, it was recorded** -- in lane C's decision on the shortcuts that are on by default (§1416: "the operator's addition, later the same day"), and as a roadmap item. Relayed 2026-10-09 |
| "...flipping the .net userspace default?", and the GPU order | A, C | §934 | **Answered here.** It is not .NET: `net.userspace` is the switch that chooses which of the system's two network stacks runs -- the one inside the kernel, or the one that runs as an ordinary program, as the design asks. "Flipping" it makes the ordinary program the default. You set the order on 2026-09-12, answering A-Q9 (§934): first fix the program's one known problem (a server answering its clients one at a time), then flip, then delete the kernel's stack. Your `todo2.txt` marks the reply to lane C, with the GPU ordering, as lane A's: relayed 2026-10-09 to both |

## open-questions/claude-answers.txt

- **Written:** 2026-10-09 16:16, as `open-questions/claude-answers.txt` in the
  integration tree. These are not the operator's answers: it is lane A's report
  to the operator -- what was recorded from `answers.txt`, and lane A's account
  of the questions the operator had asked back -- saved beside the questions.
  The checker reads its `B-Q22:`, `F-Q5:` and `F-Q3:` lines as answers. The
  operator's reply to it is `answers.2.txt` (above).
- **Processed:** 2026-10-09 by lane B: copied; each lane it speaks to sent a
  notice.
- **Copy:** `operator-answers/2026-10-09-open-questions-claude-answers.txt`
- **Content:** sha256 `317a8dc611c4aa5d9d3d358e4d87d1b8cfc4c37e0f9827987aee0164d937d1c1`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| B-Q22 | B | §1072 | recorded: lane A's reading of your answer -- close, terminate, force -- is the one §1072 records; its offer of the kernel half is accepted (lane A told 2026-10-09) |
| B-Q23 | B | -- | **still not answered**: neither file answers it, and `open-questions/B-Q23.md` waits. If the first "B-Q24" line of `answers.txt` ("Claude's recommendation") was meant for it, the recommendation there is E, with B as what "desktop" means -- say so and it is recorded |
| F-Q5 | F | -- | relayed 2026-10-09 (lane A's note: the run-time processor check disappears once the whole system is built for x86-64-v3) |
| F-Q3 | F | -- | relayed 2026-10-09 (lane A's note: keep the three choices in A-Q26's stored permissions, not a second store) |
| C-Q33 | C | -- | relayed 2026-10-09 (lane A: by your rule it is C; lane C records C unless you say otherwise) |
| D-Q3, D-Q8 | D | -- | relayed 2026-10-09 (lane A's account: for D-Q3 a fourth option, offered to you through lane D; for D-Q8, a swap-file bug lane A logged and is fixing) |
| E-Q4 | E | -- | relayed 2026-10-09 (lane A: yes, the per-drive age, size and count policy is in `roadmap-detailed.md`) |
| C-Q34, C-Q29, F-Q6 ... F-Q10 | C, F | -- | lane A's analysis for you; your answers to it are in `answers.2.txt` (above) |

## open-questions/answers.txt

- **Written:** 2026-10-09, as `open-questions/answers.txt` in the integration
  tree (not at its root, where the first two were).
- **Processed:** 2026-10-09 by lane A, the same hour: lane A's answers written
  up; every other lane sent its answers and questions by notice.
- **Copy:** `operator-answers/2026-10-09-open-questions-answers.txt`
- **Content:** sha256 `6302a9fc6df72bd6bcc87acf588378e75fb974ac3525522bea8ffaaebac2f63c`
- **Also left in the integration tree that day:** `xor.png` (C-Q28's logo),
  and an untracked `LICENSE` (MIT, dated 2026-09-16) that no question asked
  for -- lane A asked the operator whether it is to be committed.

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| F-Q1 | F | -- | relayed 2026-10-09 (B) |
| C-Q31 | C | the paragraph is in `CLAUDE.md`, added by lane A under the operator's permission in this answer | relayed 2026-10-09 for lane C's decision entry |
| B-Q24 | B | §1070 (A: new passwords are yescrypt, `$y$j9T$`; done 2026-10-09) | recorded |
| A-Q22 | A | §1564 | recorded |
| A-Q23 | A | §1565 | recorded |
| A-Q24 | A | §1566 | recorded |
| A-Q25 | A | §1567 | recorded |
| A-Q26 | A | §1568 (B, and the operator's "Allow" for this run only) | recorded |
| B-Q22 | B | §1072 (your design). Your questions, answered: yes to each -- one `kill`, procps-ng's with SlateOS's options; the system picks the route (SlateOS's message to a program that answers it, the Linux signal to one that does not), and an option forces either; and the third strength is *terminate* -- stop now, ask nothing -- between *close* (may ask "save first?") and *force*. The task manager gets the same three (asked of lane E). Done 2026-10-09: procps-ng's `kill` ported, so `kill PID` is `SIGTERM` everywhere, never the forced end; the message waits on lane A's kernel half, then lane B's library | recorded |
| B-Q25 | B | §1071 (A: a lost output is reported, in `which`, `ed`, `hostname` and `patch`; done 2026-10-09) | recorded |
| C-Q28 | C | -- | relayed 2026-10-09 (`xor.png` is at the integration tree's root) |
| C-Q29 | C | -- | relayed 2026-10-09 (A, with questions) |
| C-Q32 | C | -- | relayed 2026-10-09 (A) |
| C-Q33 | C | -- | relayed 2026-10-09 (C if startup programs depend on each other, else A) |
| C-Q34 | C | -- | relayed 2026-10-09; **questions back to the operator, no option chosen yet** |
| D-Q3 | D | -- | relayed 2026-10-09; **concerns raised, no option chosen yet** |
| D-Q4 | D | -- | relayed 2026-10-09 (B) |
| D-Q5 | D | -- | relayed 2026-10-09 (B) |
| D-Q6 | D | -- | relayed 2026-10-09 (B) |
| D-Q7 | D | -- | relayed 2026-10-09 (Claude's recommendation) |
| D-Q8 | D | -- | relayed 2026-10-09 (B, with a question about swap files) |
| E-Q1 | E | -- | relayed 2026-10-09 (Claude's recommendation) |
| E-Q2 | E | -- | relayed 2026-10-09 (Claude's recommendation, with three changes) |
| E-Q3 | E | -- | relayed 2026-10-09 (Claude's recommendation) |
| E-Q4 | E | -- | relayed 2026-10-09 (Claude's recommendation, with a question) |
| E-Q5 | E | -- | relayed 2026-10-09 (Claude's recommendation) |
| F-Q3 | F | -- | relayed 2026-10-09 (Claude's recommendation, with three choices for a capture) |
| F-Q4 | F | -- | relayed 2026-10-09 (A) |
| F-Q5 | F | -- | relayed 2026-10-09 (A, with a note) |
| F-Q6 | F | -- | relayed 2026-10-09 (B, with additions) |
| F-Q7 | F | -- | relayed 2026-10-09; **questions back to the operator, no option chosen yet** |
| F-Q8 | F | -- | relayed 2026-10-09; **questions back to the operator, no option chosen yet** |
| F-Q9 | F | -- | relayed 2026-10-09; **questions back to the operator, no option chosen yet** |
| F-Q10 | F | -- | relayed 2026-10-09 (A, with a question) |
| B-Q23 | B | -- | **not answered** in this file; the first "B-Q24" line may have been meant for it -- lane A asked the operator |

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
