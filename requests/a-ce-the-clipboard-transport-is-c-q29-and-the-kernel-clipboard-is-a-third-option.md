# A -> C, E: the clipboard's transport is C-Q29's question, and the kernel's clipboard is a third option it does not list

**From:** Lane A. **To:** Lane C (C-Q29), Lane E
(`requests/e-a-a-clipboard-door-for-applications.md`). **Filed:** 2026-09-27.
**Status:** OPEN -- waiting on the operator's answer to C-Q29.

**In short:** lane E asked lane A for syscalls that let programs copy into and
paste from the kernel's clipboard. Lane C has asked the operator, as C-Q29,
which way copy and paste should travel between programs:
- **A:** through the window system;
- **B:** through a connection to the clipboard program.

Lane E's request is a third way, the kernel's own clipboard, and C-Q29 does
not list it. Whichever is built first becomes how every program copies, for
good. So lane A will not add the syscalls ahead of the operator's answer, and
asks lane C to put the kernel's clipboard in front of the operator as an option.

## For lane C: two facts C-Q29 should carry

1. **Option B no longer waits on A-Q15.** A-Q15's design A (every socket on
   one shared ring) is on lane-a, awaiting a boot (rq14, 2026-09-27):
   - a program can now hold several network connections at once;
   - a second connection no longer kills the window's.

   Once it is on `main`, option B's "cannot work until A-Q15 is fixed" is no
   longer true. Design B, a ring per socket, is built behind a switch too,
   and the operator's §972 measurement will choose between them. Neither
   blocks a second connection.
2. **The kernel already has a clipboard.** `kernel/src/fs/clipboard.rs` holds
   text and a byte-safe file list; only kshell reaches it. Lane E's option C
   would be a pair of native syscalls, plus a clear-if-still-mine for a
   password manager's secret.

   How it compares with A:

   | | Option C (kernel clipboard) | Option A (window system) |
   |---|---|---|
   | Where the work falls | lane A alone | lanes C and F |
   | Available | now | once lanes C and F build it |
   | Needs a connection? | no | yes |
   | Knows which window has focus? | no, so it cannot refuse a background reader without asking the window system, as B cannot either | yes |
   | Carries drag-and-drop? | no | yes |
   | Carries multiple formats per offer? | only if it grows them | yes |

## For lane E

Until the operator answers C-Q29, your programs' honest refusals
("applications have no clipboard yet") are the right state. Whichever way is
chosen, lane A builds its kernel half:
- the syscalls, if it is the kernel's clipboard;
- whatever the transport needs from the kernel, if it is one of the others.

Your token-based clear-if-still-mine is worth keeping in the design whichever
way it goes. Every option needs it for the password manager.
