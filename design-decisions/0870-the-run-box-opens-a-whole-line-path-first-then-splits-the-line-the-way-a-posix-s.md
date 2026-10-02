## 870. The Run box opens a whole-line path first, then splits the line the way a POSIX shell does

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The Run box (Super+R) used to take whatever was typed as the
name of one program. So `editor notes.txt` asked the system for a program
literally called "editor notes.txt", and typing a folder asked for the folder
to be run; both failed. `design.txt` asks for a Run box "like on windows", and
Windows' takes arguments and opens folders and documents too. It now does the
same: if the whole line is a path that exists, it is opened -- a folder in the
file manager, a document in its program, a program run -- and otherwise the
first word is run with the rest as its arguments. Words are split the way a
POSIX shell splits them: quotes keep spaces together, a backslash escapes.
The code is `run_dialog::split_words` and `DesktopShell::run_request`.

### The question a previous session left open

The code carried a comment declining to split: "inventing [a quoting rule]
silently would make `my program` two words to the shell and one to the
filesystem". That is the real risk, and the two decisions below answer it
rather than avoid it.

| | `/home/u/My Stuff` (a folder) | `editor "a b.txt"` | `editor a b.txt` |
|---|---|---|---|
| **A. Never split** (what it did) | tries to *run* the folder | a program named `editor "a b.txt"` | a program named `editor a b.txt` |
| **B. Always split** | runs `/home/u/My`, argument `Stuff` | `editor`, argument `a b.txt` | `editor`, arguments `a`, `b.txt` |
| **C. Whole line as a path first, then split** -- chosen | opens the folder | `editor`, argument `a b.txt` | `editor`, arguments `a`, `b.txt` |

**Why C.** It is what Windows' Run box does, and it is the only one of the
three under which both a bare path with a space in it and a program with
arguments work. Its one ambiguity -- a line that is both an existing path and
a valid command -- resolves toward the path, and only for an *absolute* path:
`terminal` typed on its own is never taken to mean a file called `terminal` in
whatever directory the shell happens to be in.

### The quoting rule, and why POSIX's

Double quotes group and take `\"` and `\\` as escapes; single quotes group
literally; outside quotes a backslash makes the next character ordinary. The
alternative was Windows' rule (double quotes only, backslash a path
separator). POSIX's is chosen because on this system the backslash is not a
path separator and every other command line -- the shell, `oils`, scripts --
follows POSIX, so a line copied from a terminal means the same thing here. An
unclosed quote is refused with a message rather than guessed at.

### Smaller calls made with it

- **A path chosen with Browse is one word, never split** -- it is a name, not
  a command line.
- **Opening uses the desktop's rules** (`DesktopShell::open_path`, the same
  as a double-click on a desktop icon), so the Run box, the desktop and the
  file manager cannot disagree about what a file opens in.

### How to reverse

A is `run_request` ignoring `words` and launching `whole`; B is it skipping
the path check. The event carries both halves (`RunRequest`), so either
reversal is one function.
