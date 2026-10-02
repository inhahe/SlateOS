## 1024. sshd refuses to start rather than bind wider than it was asked to

**Date:** 2026-09-15
**Decided by:** Claude (autonomous) -- a user-visible policy, so flagged for the
operator to overrule
**Lane:** B

**In short:** If you tell our ssh server to accept connections only from this
machine (`ListenAddress 127.0.0.1`), it cannot do that -- the kernel's network
code can reserve a port but cannot restrict which network card it answers on.
Until today the server accepted connections from everywhere anyway and wrote
"listening on 127.0.0.1:22" in its log, so the one place you would check
confirmed a restriction that was not in force. It now refuses to start and
tells you why. The choice being recorded is refuse-vs-warn.

**The defect.** `SYS_TCP_BIND` takes a port and nothing else:
`sys_tcp_bind` reads `args.arg0 as u16` and calls `net::tcp::bind(ns, port)`.
There is no address anywhere on the path. `ListenAddress` was parsed into
`SshdConfig`, stored, formatted into the startup log, and never passed to
anything. Found by triaging lane C's echoed-settings scanner, which flagged the
field as read-only-into-output; the scanner could not know it was a security
control, which is why the triage had to be by hand.

**The options.**

| | *What changes* |
|---|---|
| **A. Warn, bind anyway** | sshd starts, logs a warning, accepts from everywhere. A machine configured for loopback-only ssh keeps working and is reachable from the network. |
| **B. Refuse to start** (taken) | sshd exits 1 with two lines saying why. A machine configured for loopback-only ssh does not run sshd until someone writes `0.0.0.0`. |
| C. Bind, then drop non-local connections in userspace | sshd enforces the restriction itself by checking the peer address after accept. |

**Why B.** The two failure modes are not symmetric. A's is an administrator who
does not read the warning and is exposed -- silently, indefinitely, with the
log actively reassuring them. B's is an administrator who reads one line and
changes one word. A security control must fail closed, and this one cannot be
provided at all, so the only honest options are "refuse" or "do it in
userspace".

The cost is bounded in a way worth stating: **the default is `0.0.0.0`**, so
nothing that works today stops working. The only configurations refused are the
ones that asked for a restriction we cannot deliver -- exactly the set of people
who would be harmed by proceeding.

**Why not C**, which is the one that actually delivers the feature: it is the
right long-term answer and it is not a substitute for the kernel doing it. A
userspace check happens *after* `accept`, so the connection is established, the
TCP handshake has completed, and the daemon has already spent a slot -- a
remote attacker can still reach and exhaust it. It also puts the boundary in
the process the boundary exists to protect. Worth building when there is a
reason to, but shipping it as though it were the same guarantee would be
another control that looks stronger than it is, which is the defect this entry
is about.

**If it is never revisited:** loopback-only ssh is unavailable rather than
falsely reported, which is the safe direction. The honest fix is a kernel bind
that takes an address; the shape is the same as `keylayout` (1023's neighbour
in this session) -- a capability that exists nowhere rather than a userspace
bug, and the distinction decides which lane can fix it.

**Reversing this** is one predicate, `listen_address_is_honoured`, and its
test. If the operator prefers A, widen it to accept everything and turn the
two `log_error` calls into one `log_warn`.
