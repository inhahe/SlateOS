## 1567. A debugger takes over a running program when the user says yes, or when the program agreed in advance

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q25 with Claude's recommendation: option A, with option B
alongside. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("A-Q25: Claude's
recommendation").

**In short:** a debugger can already debug any program it starts itself. To
take over a program that is already running (`gdb -p 1234`, `strace -p`, a
crash window's "debug this program") it needs a permission token over that one
program (the `DEBUG` right), because being the same user is not enough here
(§24). Two things give it that token: the user, asked at the moment ("Allow
gdb to inspect firefox?"), or the program itself, having named in advance who
may debug it (Linux's `prctl(PR_SET_PTRACER)`, which crash handlers use).

**The alternatives not taken:** a debugger may take over only what it started
itself, directly or not (Linux's common Yama policy); administrators may take
over anything; the same user may take over anything -- ruled out by §24, since
one compromised program could then read every other one's memory. The
administrator rule could be added later.

**What it obliges.**
- `PTRACE_ATTACH` and `PTRACE_SEIZE` (and the native `SYS_PTRACE`'s attach)
  succeed when the caller holds `(Process, <pid>, DEBUG)`; without it they ask
  for it through the capability request broker (`SYS_CAP_REQUEST_FOR`, §1548),
  whose handler is the desktop's permission dialog (lane C), and proceed if the
  user allows. With no handler -- a text console, no desktop -- they are refused
  as today, until a text-mode prompt exists.
- `prctl(PR_SET_PTRACER, pid)` (and `PR_SET_PTRACER_ANY`) records whom a
  program lets attach to it with no prompt; cleared at `exec`, as on Linux.
- Everything else a debugger needs is built (§1547): an attached
  program is handled as a started one is.
