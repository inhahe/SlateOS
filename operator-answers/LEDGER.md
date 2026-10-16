# Operator answers -- ledger

One section per answers file, newest first. `check-lane-signals.py` reads the
`sha256` lines to tell a recorded file from a new one (`README.md` here says
how); the rows say where each answer went. **State** is `recorded` when the
answer is written up and nothing from it waits on the operator; anything still
waiting on the operator says so in bold, with the question that carries it.

A lane that processes a new file adds its section, fills in its own lane's
rows, and leaves the other lanes' rows as `relayed <date>` for those lanes to
fill in. Section numbers (§) are `design-decisions/` entries.

## chat, 2026-10-16: the logo is `xor2.png`; LithicBackup backs up on the writer's "done"

- **Given:** 2026-10-16, in chat to lane F's session. Neither part answers a
  queued question but C-Q28, which asked for the logo file. The operator
  wrote `.gif`; the two files in the integration tree are PNGs, `xor.png` and
  `xor2.png`.
- **Processed:** 2026-10-16 by lane F: copied; the request written into
  `roadmap-detailed.md` (LithicBackup's entry, §6.13); lanes C and E told by
  notice.
- **Copy:** `operator-answers/2026-10-16-chat-to-lane-f.txt`
- **Content:** sha256 `880dba4f25724cb267f0368a947f1d975f7512db2134a57996a0854f9112fc64`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| C-Q28: the logo is `xor2.png`, not `xor.png` (both at the integration tree's root, untracked) | C | -- | relayed 2026-10-16 |
| LithicBackup's port backs a file up, or makes its new version, when its writer finalizes it, and keeps its quiet-time rules for writers that never do: Linux programs, Windows programs under Wine, and the rest | E | `roadmap-detailed.md`, LithicBackup's entry | recorded by lane F; relayed 2026-10-16 to lane E, whose item it is |

## open-questions/answers.3.txt

- **Written:** 2026-10-11 02:14, as `open-questions/answers.3.txt` in the
  integration tree.
- **Processed:** 2026-10-11 by lane B (copied; lane B's answer recorded; lanes
  C and F sent theirs by notice) and 2026-10-12 by lane F (lane F's answers
  written up; every lane told of F-Q15's), each without seeing the other's.
- **Copy:** `operator-answers/2026-10-11-open-questions-answers.3.txt`
- **Content:** sha256 `f3cee424078cd0ae670605616f54fb1fe4b2bd9515050b51b70a92209fb2c8ea`
  (the checker's key: CRLF read as LF; the raw file's sha256 is
  `d159de1dabe9621db387bade0cd74d9dd4b295d3b6a927b9831160596f427cfc`)

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| B-Q26: A | B | §1074 (a program's remembered settings gain the processors it may use, a cap on its processor use and a profile while it runs; `roadmap-detailed.md` §1.3) | recorded |
| C-Q35: Claude's recommendations | C | -- | relayed 2026-10-11 |
| C-Q36: A | C | -- | relayed 2026-10-11 |
| F-Q11: C, "if that would usually result in more correctness" | F | §1389 (it does: a part a video gives is its encoder's word, a guess is a guess) | recorded |
| F-Q12: B | F | §1390 | recorded |
| F-Q13: C | F | §1391 (the compositor's half lane F's, the prompts' and the taskbar's lane C's) | recorded |
| F-Q14: "I plan on 1, but it's not a sure thing" | F | -- | **waits on the operator: `open-questions/F-Q14.md`, until the new computer is here** |
| F-Q15: A | F | §1392 (every lane told by notice) | recorded |

## chat, 2026-10-10: B-Q23 answered, B-Q24 confirmed, answered questions out of sight

- **Given:** 2026-10-10, in chat to lane F's session, after lane F showed the
  operator which files in `open-questions/` were already answered and why
  they were still there: B-Q23's answer (`answers.txt` had labelled it
  "B-Q24"), B-Q24's confirmed, and a request about how the open questions
  are shown.
- **Processed:** 2026-10-10 by lane F: copied; lane B told by notice; the
  request answered in chat with a proposal.
- **Copy:** `operator-answers/2026-10-10-chat-to-lane-f.txt`
- **Content:** sha256 `27ab410bb9d828ce4214a6ee4369b453fa2f788ec62f5a7d44946b468fd1624c`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| B-Q23: Claude's recommendation -- E, the installer's question decides what is installed, with B as what "desktop" means | B | §1073 | recorded 2026-10-11: the installer's "what is this machine for" answer picks a package group, "desktop" being everything but the six network servers; the image keeps everything until an installer exists (§1053) |
| B-Q24: A -- the second of `answers.txt`'s two "B-Q24" lines; the first was B-Q23's | B | §1070 | recorded (lane B took A on 2026-10-09) |
| Questions already answered should not show among the open ones, though their lanes are rate-limited for days | F | `scripts/questions-for-you.py` | recorded: `questions-for-you/` in the integration tree, only the questions still waiting on you, rebuilt when you run the `update.bat` you keep there -- no scheduled task ("nah, i'll just run update.bat ... when i want it updated", later the same day: `operator-answers/2026-10-10-chat-to-lane-f-2.txt`) |

## chat, 2026-10-10: `dotnet/runtime` instead of Mono; the ports listed in roadmap-detailed.md

- **Given:** 2026-10-10, in chat to lane E's session. It answers lane E's
  report that Mono 6.14 cannot run the operator's two C# programs, both of
  them .NET 8 with a WPF window. It is not the answer to a queued question,
  so there is no file in the integration tree.
- **Processed:** 2026-10-10 by lane E, the same hour:
  - recorded as §1246;
  - the ports the operator asked for by name, listed in
    `roadmap-detailed.md` §6.13;
  - lanes D and B told by notice.
- **Copy:** `operator-answers/2026-10-10-chat-dotnet-runtime-instead-of-mono.txt`
- **Content:** sha256 `88f42c1431aa236b08d6bfaa4ba59e2066d1d7b6da1630c68d4f76ab651d3d64`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| the .NET port is `dotnet/runtime`, not Mono | D, B | §1246 (written by lane E) | relayed 2026-10-10 (D: its Mono item in `roadmap.md` becomes `dotnet/runtime`); B: recorded 2026-10-11 -- the §1050 list in lane B's backlog names `dotnet/runtime` and points to §6.13 |
| the ports asked for by name, listed in `roadmap-detailed.md` | E | `roadmap-detailed.md` §6.13 | recorded |

## chat, 2026-10-10: a consent prompt takes no keystrokes

- **Given:** 2026-10-10, in chat to lane E's session -- a rule, not the answer
  to a queued question, so no file in the integration tree.
- **Processed:** 2026-10-10 by lane E, the same hour: recorded as §1242 with
  the defect it finds (`known-issues/E-the-capability-prompt-is-answered-by-the-keys-the-user-is-typing.md`);
  lanes C and F told by notice and asked in
  `requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md`.
- **Copy:** `operator-answers/2026-10-10-chat-consent-prompts-take-no-keystrokes.txt`
- **Content:** sha256 `192ca7baf831fdf8127577d499703b36ed49eec336b21b5ec2c274458dc53186`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| a consent prompt is answered only on purpose: no key the user was typing answers it | C, F | §1242 (written by lane E) | relayed 2026-10-10 (C: the prompt's keys and clicks; F: a window that opens without the keyboard) |

## open-questions/claude-answers.txt

- **Written:** 2026-10-09 16:16, in the integration tree's `open-questions/`:
  not the operator's answers but lane A's chat reply to the questions asked
  back in `open-questions/answers.txt`, which the operator kept there.
- **Processed:** 2026-10-09 by lane A, and the same day, separately, by lane
  B, whose rows for its own questions are below; the two records met at the
  merge of 2026-10-15. Nothing in it is a decision; what the operator decided
  after reading it is the next file, `open-questions/answers.2.txt` (section
  below).
- **Copy:** `operator-answers/2026-10-09-claude-answers.txt`
- **Content:** sha256 `317a8dc611c4aa5d9d3d358e4d87d1b8cfc4c37e0f9827987aee0164d937d1c1`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| Lane A's answers on D-Q3, D-Q8, C-Q29, C-Q33, C-Q34, E-Q4, F-Q3, F-Q5, F-Q6, F-Q7, F-Q8, F-Q9, F-Q10, B-Q22 | A | relayed to lanes B-F by notice, 2026-10-09 19:40Z | recorded -- a reply, nothing to decide |
| B-Q22 | B | §1072 | recorded: lane A's reading of your answer -- close, terminate, force -- is the one §1072 records; its offer of the kernel half is accepted (lane A told 2026-10-09) |
| B-Q23 | B | §1073 | not answered in either file; answered 2026-10-10 in chat to lane F and recorded 2026-10-11 |

## open-questions/answers.2.txt

- **Written:** 2026-10-09 16:15, in the integration tree's `open-questions/`,
  and given in chat to lane A minutes later, word for word, after lane A
  answered the questions the operator had asked back in
  `open-questions/answers.txt`.
- **Processed:** 2026-10-09 by lane A, the same hour (from the chat; the file
  was found and its copy made byte-exact at the next sync) -- and the same
  day by lane B, whose answer to the Process Lasso question was B-Q26; the
  two records met at the merge of 2026-10-15.
- **Copy:** `operator-answers/2026-10-09-open-questions-answers.2.txt`
- **Content:** sha256 `5e94dc49f15ffd10ca0eaa0053a1cd35cf84524c809fbeeee00853e2d252ad6a`

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| C-Q34 | C | -- | relayed 2026-10-09: A, with the per-element control lane A described (text and shapes scaled; an image carries its own density; a surface can ask for real pixels) |
| C-Q29 | C | -- | relayed 2026-10-09: A, confirmed after lane A's answer on programs with no window (a data-control path; reading from the background takes a permission) |
| F-Q7 | F | -- | relayed 2026-10-09: A, keep the translation |
| F-Q8 | F | -- | relayed 2026-10-09: **B**, translate FFmpeg's H.264 decoder (lane F had recommended A) |
| F-Q9 | F | -- | relayed 2026-10-09: **B**, translate FFmpeg's fixed-point AAC decoder (lane F had recommended A) |
| F-Q10 | F | -- | relayed 2026-10-09: A, Chrome's colour library; Little CMS when printing is built |
| F-Q6 | F, C, D | -- | relayed 2026-10-09: turning remote desktop on offers, in the same steps, a dynamic-DNS name (lane D's `services/dyndns`) and a Let's Encrypt certificate -- whatever is not yet set up |
| The MIT licence | operator | `LICENSE`, lane-a-wip `13032901f` | **committed, held off `main`** (2026-10-15): at the repository root it makes lane C's `gather-notices.py` fail every boot test; lane A restores it, and its owner entry in `which-lane.py`, when lane C's fix is on `main` (`requests/a-c-gather-notices-reads-the-repository-root-as-a-crate.md`) |
| The file reform (`todo2.txt`, `roadmap.md`, `known-issues.md`) | A | §1530 | answered in chat: done for the issues, decisions and questions, and finished roadmap blocks moved to `roadmap-done.md`, with a search database; **not** done for `todo.txt` (59,315 lines) -- lane A's, on its roadmap. `todo2.txt` is the operator's own and was never part of it |
| Process Lasso's controls | B | `open-questions/B-Q26.md` → §1074 | **answered 2026-10-11 (B-Q26: A)**: a program's remembered settings gain the processors it may use, a cap on its processor use and a profile while it runs. Lane A asked the same thing back as A-Q27, which this answer settles (lane A told 2026-10-15) |
| Page Up, Page Down, Home, End, Ctrl+Home, Ctrl+End | C | §1416 (2026-09-27) | recorded and done the same day |
| "Flip the net.userspace default" | A | §934 (A-Q9, 2026-09-12) | answered in chat: networking moves out of the kernel into its own program; you chose to flip after one fix, and it was flipped 2026-09-12; the in-kernel copy's deletion is lane A's remaining step |
| "Which order to do the next few things in" | A, C | §69 (2026-07-18), §264 (2026-08-21) | answered in chat: recorded |

## open-questions/answers.txt

- **Written:** 2026-10-09, as `open-questions/answers.txt` in the integration
  tree (not at its root, where the first two were).
- **Processed:** 2026-10-09 by lane A, the same hour: lane A's answers written
  up; every other lane sent its answers and questions by notice.
- **Copy:** `operator-answers/2026-10-09-open-questions-answers.txt`
- **Content:** sha256 `6302a9fc6df72bd6bcc87acf588378e75fb974ac3525522bea8ffaaebac2f63c`
- **Also left in the integration tree that day:** `xor.png` (taken that day
  for C-Q28's logo, which is `xor2.png`: the operator, 2026-10-16, above),
  and an untracked `LICENSE` (MIT, dated 2026-09-16) that no question asked
  for -- lane A asked the operator whether it is to be committed; the answer
  was yes, and it is (section above).

| Answer | Lane | Recorded in | State |
|---|---|---|---|
| F-Q1 | F | §1370 (HEIC, and HEVC video, a separate install) | recorded |
| C-Q31 | C | §1606; the paragraph is in `CLAUDE.md`, added by lane A under the operator's permission in this answer | recorded 2026-10-12 |
| B-Q24 | B | §1070 (A: new passwords are yescrypt, `$y$j9T$`; done 2026-10-09) | recorded |
| A-Q22 | A | §1564 | recorded |
| A-Q23 | A | §1565 | recorded |
| A-Q24 | A | §1566 | recorded |
| A-Q25 | A | §1567 | recorded |
| A-Q26 | A | §1568 (B, and the operator's "Allow" for this run only) | recorded |
| B-Q22 | B | §1072 (your design). Your questions, answered: yes to each -- one `kill`, procps-ng's with SlateOS's options; the system picks the route (SlateOS's message to a program that answers it, the Linux signal to one that does not), and an option forces either; and the third strength is *terminate* -- stop now, ask nothing -- between *close* (may ask "save first?") and *force*. The task manager gets the same three (asked of lane E). Done 2026-10-09: procps-ng's `kill` ported, so `kill PID` is `SIGTERM` everywhere, never the forced end; the message waits on lane A's kernel half, then lane B's library | recorded |
| B-Q25 | B | §1071 (A: a lost output is reported, in `which`, `ed`, `hostname` and `patch`; done 2026-10-09) | recorded |
| C-Q28 | C | §1604 (the logo is `xor2.png` -- the operator, 2026-10-16 -- now `gui/desktop/assets/xor2.png`; the start button is to draw it) | recorded 2026-10-12, the file corrected 2026-10-16 |
| C-Q29 | C | §1605 (A; both questions answered there, as lane A answered them) | recorded 2026-10-12 |
| C-Q32 | C | §1607 | recorded 2026-10-12 |
| C-Q33 | C | §1608 (C: startup programs do depend on each other and on services) | recorded 2026-10-12 |
| C-Q34 | C | §1609 (A, chosen in chat after lane A's answers; per-element density as the operator asked) | recorded 2026-10-12 |
| D-Q3 | D | -- | relayed 2026-10-09; **concerns raised, no option chosen yet** |
| D-Q4 | D | -- | relayed 2026-10-09 (B) |
| D-Q5 | D | -- | relayed 2026-10-09 (B) |
| D-Q6 | D | -- | relayed 2026-10-09 (B) |
| D-Q7 | D | -- | relayed 2026-10-09 (Claude's recommendation) |
| D-Q8 | D | -- | relayed 2026-10-09 (B, with a question about swap files) |
| E-Q1 | E | §1235 | recorded (whisper.cpp; the port is lane D's, the model package lane B's, the dictation front end lane E's) |
| E-Q2 | E | §1236 | recorded: weather and the dictionary's online lookups off until turned on; the programs whose purpose is the internet (speed test, IRC, torrents, VPN, remote desktop, the scanner's WHOIS) unchanged. The table in §1236 says how "those three things" was read |
| E-Q3 | E | §1237 | recorded (`pkg rollback` exists; a desktop entry for it waits on `requests/e-b-programs-can-ask-the-package-manager-to-roll-back.md`) |
| E-Q4 | E | §1238 | recorded; the question back is answered there: yes, each drive's bin gets an age, a size and a count limit (`roadmap-detailed.md`) |
| E-Q5 | E | §1239 | recorded (one window for the rest waits on `requests/e-cf-a-program-can-ask-to-have-only-one-window.md`) |
| F-Q3 | F | §1371 (the person's action is the permission; scheduled capture asks once, for its run, or always, in A-Q26's store) | recorded |
| F-Q4 | F | §1372 (rav1d's assembly) | recorded |
| F-Q5 | F | §1373; the note in `deferred-questions/F-when-slateos-is-built-for-x86-64-v3-...` | recorded |
| F-Q6 | F | §1374 (PIN pairing; turning it on sets up the name, the port and a certificate; a browser viewer for any screen); lanes C and D asked in `requests/f-cd-turning-on-remote-desktop-sets-up-reaching-the-machine.md` | recorded |
| F-Q7 | F | §1375 (kept), answered "A" in `open-questions/answers.2.txt` after lane A's replies | recorded |
| F-Q8 | F | §1376 (FFmpeg's H.264 decoder, translated), answered "B" in `open-questions/answers.2.txt` | recorded |
| F-Q9 | F | §1377 (FFmpeg's fixed-point AAC decoder, translated), answered "B" in `open-questions/answers.2.txt` | recorded |
| F-Q10 | F | §1378 (Chrome's colour management: skcms, Chrome's HDR handling); its question -- printers -- answered by lane A: Little CMS when printing is built | recorded |
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
| C-Q9 | C | §841 (you had no preference and asked two questions; both are answered there with measurements, and the call was Claude's) | recorded; its line in `open-questions-resolved/lane-c.md` added 2026-10-12 |
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
