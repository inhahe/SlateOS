## 934. Networking moves out of the kernel: fix the head-of-line block, flip, then delete

**Date:** 2026-09-12 · **Decided by:** Operator · **Lane:** A

Answering A-Q9: **C, then D.** Claude laid out four options and did not
recommend one, because the question is risk appetite rather than code.

**In short:** this system does networking two ways -- inside the kernel, and as
an ordinary background daemon. The daemon has feature parity and is exercised
on every boot when switched on, but it is off by default. The operator's answer
is to make it the default *after* fixing its one known rough edge (option C),
and then to delete the in-kernel stack outright (option D).

**The order is the decision.** C before A matters because the rough edge is
real: a listening socket keeps one daemon session and every accepted connection
shares it behind one lock, so a server serves its clients strictly one at a
time and one slow client holds up the rest (`D-NETSOCK-SYNC`). Flipping first
would ship that to everyone. D after C matters because deleting ~40 kernel
files is the step with no way back except a revert, and it should happen when
the replacement is not merely the default but has run as the default.

**Why D at all, rather than keeping the kernel stack as a fallback.** A
fallback that is never selected is not tested, and an untested fallback is a
worse position than none -- it reads as safety while decaying. Deleting makes
the microkernel rule true rather than merely available.

**What each step costs.**

| step | work | reversible |
|---|---|---|
| C: async session layer | rewrite `kernel/src/net/socket.rs`'s session sharing so accepted fds do not serialise | yes |
| flip the default | `net.userspace` on by default | yes, one switch |
| D: delete | remove the in-kernel stack, ~40 files | only by revert |

**Not started at decision time.** Recorded first so the sequencing survives
even if the work is picked up by a later session that did not see this answer.
