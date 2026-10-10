## 1568. A permission prompt offers "Allow" until the program closes, and "Always allow" for that program

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended option B; the operator added the second choice) · **Lane:** A

Answering A-Q26: option B, and, in the operator's words, "if it's not already
planned or existent, have not only an 'Always allow' option, but also an allow
option that's only for that instance". The answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt`.

**In short:** when a program asks for extra access and the user is asked, two
ways to say yes are offered:
- **Allow** -- for this run of the program only: the access lasts until the
  program closes, and the next time it starts it asks again. This is what an
  approval already does (§1548).
- **Always allow** -- remembered for that program: it gets the access at every
  later start without asking. The program is recognised by its fingerprint (a
  checksum of the program file's contents), so an updated or tampered program
  asks again. Settings lists what was always-allowed, to take back.

**The alternatives not taken:** no "Always" at all; remembering until logout;
recognising the program by its name or path, which would hand the access to
whatever program was put at that path.

**What it obliges.**
- The kernel (`kernel/src/cap/request.rs`): an answer that says which of the
  two it is (`SYS_CAP_REQUEST_DECIDE`'s verdict gains "always"); a persistent
  store of always-allowed grants -- the program's fingerprint, the capability's
  type, object and rights -- applied when a program with that fingerprint
  starts; calls to list and revoke them for Settings. The fingerprint is
  computed when the program is loaded.
- Lane C: the dialog shows both choices (its "Always allow" checkbox becomes
  the second button, or stays a checkbox beside one Allow), and a Settings page
  lists and revokes the remembered grants.
