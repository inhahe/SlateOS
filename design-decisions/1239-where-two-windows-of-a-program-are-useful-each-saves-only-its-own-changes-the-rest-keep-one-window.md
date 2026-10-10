## 1239. Where two windows of a program are useful, each saves only its own changes; the rest keep one window

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** E

Answering E-Q5. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("E-Q5: Claude's
recommendation").

**In short:** opening one of these programs twice must not lose anything. For
the programs where two windows side by side are genuinely useful -- notes,
contacts, the kanban board, snippets, the e-book reader's library, reminders,
flashcards and finance -- each window, when it saves, reads the file again and
puts in only the record it changed. The other windows are told the file changed
and show the new state. Two windows editing the *same* record at once is the one
case where the later save wins, and only for that record. Every other program --
a backup program or System Restore opened twice is a mistake, not a way of
working -- keeps a single window: starting it again brings the running one to
the front.

**The alternatives not taken:** one window for every program (loses side-by-side
notes and contacts), and a read-only second window (needs a lock that a crashed
window can leave behind).

**What it obliges.**

- Lane E, program by program, for the eight above: an id per record, saving
  record by record into what is on disk, and a notice when a data file changes,
  as the settings files have. Each program gets tests with two windows.
- Lanes C and F, for single-window programs: a way for a program to find its
  running copy and hand it the request to come to the front
  (`requests/e-cf-a-program-can-ask-to-have-only-one-window.md`).
- `known-issues/`: "[E] Two windows of one program: the last to save throws away
  what the other saved" stays open until both halves are done.
