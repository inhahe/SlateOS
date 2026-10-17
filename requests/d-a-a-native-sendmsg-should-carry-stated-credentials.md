# D → A: the native `sendmsg` should carry the credentials a sender states (`SCM_CREDENTIALS`)

**Filed:** 2026-10-17 by lane D. **For:** lane A (`SYS_UNIX_SENDMSG`,
`kernel/src/ipc/unix_socket.rs`). **Status:** OPEN. **Priority:** low --
nothing breaks; a privileged sender cannot speak for another.

## In short

A program can attach credentials to a message it sends over a Unix socket
(`SCM_CREDENTIALS`): a pid, a uid and a gid. Linux checks them
(`scm_check_creds`: the sender's own pid, any of its real, effective or
saved ids, or anything with `CAP_SYS_ADMIN`/`CAP_SETUID`/`CAP_SETGID`) and
hands the receiver *what was stated*. Your kernel does this for Linux-ABI
programs (`check_stated_cred`). The native `SYS_UNIX_SENDMSG` takes no
credentials, and the receiver is told the sender's real ones.

So the C library accepts only a statement of the sender's own pid and real
ids -- what the receiver will be told -- and refuses the rest with `EPERM`
(`known-issues/D-POSIX-STATED-CREDENTIALS-ARE-ONLY-THE-REAL-ONES.md`). A
setuid program stating its effective id is refused, and so is a root
service speaking for the client it serves -- what D-Bus brokers and
logging proxies do on Linux.

## Asked

A way for `SYS_UNIX_SENDMSG` to carry the stated credentials: for example
three more words in its record (pid, uid, gid, with a flag saying they are
there), checked by `check_stated_cred`'s rule and recorded for the
receiver in their place. With it, the library passes what the program
stated and takes Linux's rule.
