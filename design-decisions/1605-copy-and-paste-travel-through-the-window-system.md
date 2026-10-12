## 1605. Copy and paste between programs travel through the window system

**Date:** 2026-10-12 &middot; **Decided by:** Operator (Claude recommended A; the operator chose A, asked two questions back, and confirmed A after lane A's answers) &middot; **Lane:** C, with F

**In short:** something copied in one program can be pasted in another only
if the clipboard is somewhere both can reach. The operator chose the window
system -- the compositor, which every program with a window is already
connected to -- over a second connection to the clipboard program or the
kernel's own clipboard (`open-questions.md` C-Q29). It is how Wayland and
macOS work, and the same channel carries dragging between programs. Text
already travels this way (§1488); the decision is that it stays the way.

**The answers, verbatim.** In `operator-answers/2026-10-09-open-questions-answers.txt`:

> C-Q29: A, though I'm concerned that this way a headless program wouldn't be
> able to ask what's in the clipboard or write to it? By the way, we have
> planned for our clipboard program to remember past clips, kinda like Ditto,
> right?

And after lane A's answers to both questions, in chat (`operator-answers/2026-10-09-chat-to-lane-a.txt`):

> C-Q29: A

**The two questions, as lane A answered them and the operator confirmed:**

| Question | Answer |
|---|---|
| Can a program with no window read or write the clipboard? | Yes: what it needs is the connection to the window system, not a window -- Wayland's "data control" extension is the precedent (clipboard managers, `wl-copy`/`wl-paste`). Writing is open to every program; reading without a focused window takes a permission, which keeps background programs from snooping on what the user copies. |
| Does the clipboard program remember past clips? | Yes, it is planned and partly built: `design.txt`'s "clipboard with history view and select", `roadmap-detailed.md`'s clipboard history, and `gui/clipboard`, which already keeps one. |

**What it obliges:** the window system carries every format a copy offers
(text, formatted text, pictures, files) and drags between programs (lane F);
a windowless program's reading and writing over the same connection, reading
behind a permission (lane F's protocol, lane C's toolkit client); the
clipboard program keeping the history from what the window system carries
(lane C). The private copies in programs and the kernel's clipboard go as
each is replaced (`known-issues/TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO.md`).
