### TD-OILS-SIGNAL-TABLE-IS-LINUXS-NOT-THE-HOSTS. osh numbers signals the way Linux does, where the reference bash on this host numbers them the way Cygwin does — 2026-08-03 — ⚠️ **KNOWN DEVIATION, NOT A BUG**

**Where:** `userspace/oils/src/interp.rs` — the `SIGNALS` table.

osh is a shell for SlateOS, whose signal numbering is Linux's: `7` is `BUS`,
`10` is `USR1`, `12` is `USR2`, and the table ends at 31 with no realtime
signals (35 specs once `EXIT` and the three pseudo-signals are counted). The
reference bash used by the corpus differ is Git for Windows' msys/Cygwin build,
whose table is the **Cygwin** one: `7` is `EMT`, `10` is `BUS`, and 32 realtime
signals follow, for 68 specs. Neither is wrong; they are simply different
platforms, and osh must match its own.

**Observable difference:** any *numeric* sigspec outside the handful the two
agree on (`0`/`EXIT`, `1`/`HUP`, `2`/`INT`, `9`/`KILL`, `15`/`TERM`) names a
different signal in each — `trap 'x' 10` is `SIGUSR1` here and `SIGBUS` there.
`kill -l` and `trap -l` list different sets entirely, and posix `trap -p` with
no operands prints 35 lines here against 68 there.

**Consequence for the corpus:** a differential case must not print a signal
*number*→*name* mapping beyond the agreed few, must not list the whole signal
table (`kill -l`, `trap -l`, posix `trap -p` with no operands), and must not use
a numeric sigspec except `0`. The affected cases say so in their headers, and
the whole-table listings are covered by lib tests instead.

**What would change this:** running the differ against a Linux bash. Nothing in
osh should change — its table is correct for its target.
