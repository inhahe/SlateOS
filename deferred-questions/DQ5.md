## DQ5 — Should one user be able to put a message on another user's terminal (`write`, `wall`, `mesg`), and through what? — deferred 2026-10-01 (lane B)

**In short:** on Linux, `write alice` opens alice's terminal by its path
(`/dev/pts/3`) and prints a message on it, `wall` does it to everyone, and
`mesg n` blocks both by clearing a permission bit on the terminal. SlateOS
cannot work that way: a terminal is reached only through a handle (an
unforgeable token the kernel gives its owner), never by a path anyone may
open -- `SYS_PTY_SLAVE_ID`'s own doc says knowing `/dev/pts/3` "grants
nothing". So these tools need a design, not a port: some service that holds
each session's terminal and delivers a message only if its owner allows it.
`userspace/mesg`, which answered as `mesg`, `write` and `talk`, faked all of
this through files and was deleted (known-issues
`B-MESG-AND-WRITE-UNTIL-PORTED`).

**Why this is not in `open-questions.md`.** Nothing on SlateOS needs it yet:
there is one interactive session at a time, `logind` (which tracks sessions)
is not on the image, and no program wants to announce a shutdown to other
users' terminals. Asking now would be asking which mailbox to build for a
house with one resident.

**The choice, when it arrives:**

| Option | *What changes* |
|---|---|
| A session-message service (logind is the natural owner: it already knows every session and its terminal) that `write`/`wall` ask, and that each session's `mesg` setting gates | *`write alice` and `wall` work, with consent enforced by the service that holds the terminals; ports of util-linux's tools talk to it instead of opening devices* |
| Desktop notifications only | *Messages between users appear as notifications in the graphical session; text-mode `write`/`wall` stay absent* |
| Nothing | *No user-to-user messages; shutdown notices reach only the session that asked* |

**Trigger to promote this into `open-questions.md`:** whichever comes first --
(a) two interactive sessions can run at once (logind on the image, or a
second login on a serial or ssh terminal); (b) a program needs to notify
other sessions -- a shutdown or reboot warning from `powerctl`, a `wall`
from a script.
