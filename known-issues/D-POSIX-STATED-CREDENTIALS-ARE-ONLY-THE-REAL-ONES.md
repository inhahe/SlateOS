## D-POSIX-STATED-CREDENTIALS-ARE-ONLY-THE-REAL-ONES — a native `sendmsg` may state only its own pid and real ids in `SCM_CREDENTIALS`, where Linux takes the effective and saved ids, and anything from root (lane D, 2026-10-17) — OPEN
**Status:** OPEN -- waiting on lane A (`requests/d-a-a-native-sendmsg-should-carry-stated-credentials.md`).

**In short:** a program can attach its identity -- process id, user id,
group id -- to a message it sends over a Unix socket (`SCM_CREDENTIALS`),
and the receiver reads it. Linux checks what is stated and passes it on:
any of the sender's real, effective or saved ids, and anything at all from
root, which is how a privileged proxy speaks for the client it serves. The
native send here carries no stated credentials: the kernel tells the
receiver the sender's real ones, whatever was stated. So the C library
accepts only what the receiver will be told -- the sender's own pid and
real ids -- and refuses the rest with `EPERM`. A setuid program stating
its effective id, or a root proxy stating its client's, is refused where
Linux would accept it.

Accepting them instead would be worse: the send would succeed and the
receiver would read other credentials than were stated, a root proxy's
own for its client's (Opus's review of lane D's round 205's fixes).

**Where:** `posix/src/unix_socket.rs`, `may_state`, called by
`check_control` for each `SCM_CREDENTIALS` message.

**The proper fix:** `SYS_UNIX_SENDMSG` takes the stated credentials, checks
them as the kernel's Linux ABI already does (`check_stated_cred`), and
records them for the receiver; then `may_state` takes Linux's rule.
