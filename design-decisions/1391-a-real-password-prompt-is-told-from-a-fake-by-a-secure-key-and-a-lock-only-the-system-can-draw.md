## 1391. A real password prompt is told from a fake by a secure key before typing, and a lock only the system can draw

**Date:** 2026-10-11
**Lane:** F
**Decided by:** Operator (F-Q13, `operator-answers/2026-10-11-open-questions-answers.3.txt`:
"C"; Claude's recommendation)

**In short:** any program can draw a window that looks exactly like the
password manager's prompt and collect the master password. SlateOS gets both
defences:

- **A secure key** -- the system's own Ctrl+Alt+Del. A real password prompt
  takes no typing until the user presses a key combination that only the
  compositor ever receives; no program can see it or fake it. Pressed while
  a fake is on screen, it says that no system prompt is waiting. This is the
  protection.
- **A lock on the taskbar**, which no program can cover or draw into, with
  the true name of the program asking, while a real prompt has the keyboard.
  This is what teaches the habit: "real prompts show this, and want this
  key". A program in full screen gives way to a real prompt, so the lock is
  never hidden.

**Whose each part is.** The compositor (lane F): the key that only it
receives, the grab that sends what follows only to the real prompt, telling
a real prompt's window from others, and full screen giving way. The prompts
(`gui/credentialsd`, `gui/desktop/src/security_dialog.rs`) and the taskbar's
lock (lane C): asking the compositor for the secure input, and showing the
lock with the asking program's name. Both wait on the compositor knowing
which program owns a window, which needs the C library's channel calls
(`requests/f-d-the-c-library-s-slateos-channel-calls.md`); until then the
compositor cannot vouch for the password manager either.

**Not chosen, and why:** the lock alone (A) protects only people who notice
it is missing; the dimmed secure desktop (D) can be imitated, and can be
added later on top of this.

**Related:** `known-issues/C-THE-PASSWORD-PROMPT-CAN-BE-IMITATED-BY-ANY-WINDOW.md`;
`requests/c-f-let-the-user-tell-the-systems-own-prompts-from-a-programs.md`.
