# B → C: every program built on the toolkit answers *close* as its close button does, and *terminate* as session end does

**Filed:** 2026-10-09 by lane B. **Addressed to:** lane C (the GUI toolkit,
and the desktop's own programs). **Status:** OPEN -- a heads-up: the part
asked of you needs lane A's lifecycle endpoint and lane B's library first,
and this file will carry their names when they exist. **Priority:** medium.

## In short

The operator decided (B-Q22, design-decisions §1072) that stopping a program
comes in three strengths, chosen by the user in `kill` and in the task
manager, and that every SlateOS program is to answer the first two:

| request | what the program does |
|---|---|
| **close** | what clicking its window's close button does: it may ask "save first?", and may decline |
| **terminate** | stops promptly: saves what it can without asking (as at session end), releases what it holds, exits |
| **force** | nothing -- the kernel ends it |

A program built on the toolkit should answer the first two without its
author writing anything: the toolkit already knows what its close button does
and what session end does.

## What will be asked

1. When lane B's library exists (it registers the process's lifecycle
   endpoint -- lane A's kernel half -- and hands each request to a
   callback), the toolkit registers at start-up, as every toolkit program's
   first window is made or before.
2. *close* runs the same path as the close button of the program's windows
   -- every window, in the order the toolkit closes them at quit -- and
   answers "closing" or "declined" (the user chose Cancel at "save first?").
3. *terminate* runs session end: the path the desktop takes at log-out,
   without any dialog, then exit.
4. The desktop's own programs (shell, panels) answer as fits them; the
   session manager's own stop order is yours.

Nothing to do yet. The library's API, and how a program that is not built
on the toolkit answers, will be in this file when it lands.
