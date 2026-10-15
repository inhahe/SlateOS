## 1569. A program registers for close and terminate requests, and the kernel brings each as one message

**Date:** 2026-10-09 · **Lane:** A · **Decided by:** Claude (operator-approved scope) -- the operator decided the three requests and their meaning (§1072, answering B-Q22); the mechanism below is lane A's call.

**In short:** a program that can be asked to close or to stop says so once,
and from then on every such request reaches it as a message on a channel of
its own. Whoever may send it a signal may ask. A program that never said so is
asked the old way, with `SIGTERM`. The kernel carries the request and nothing
else: no answer comes back, and the asker learns what happened from whether
the program exits.

**What was built** (`kernel/src/proc/lifecycle.rs`; calls 1172-1174; lane B's
`requests/a-b-the-lifecycle-endpoint.md`): `SYS_LIFECYCLE_REGISTER` returns a
channel end; `SYS_LIFECYCLE_REQUEST(pid, close | terminate)` sends a 24-byte
message on it under the signal permission rule; `SYS_LIFECYCLE_QUERY(pid)`
says whether `pid` answers.

### The choices, and the alternatives

| Choice | Taken | Not taken, and why |
|---|---|---|
| How a program gets its endpoint | it registers | **A handle at start**, as Fuchsia's `PA_LIFECYCLE`: programs start many ways here (the shell, the desktop, services, `posix_spawn`), and only the program knows whether it answers; a start-time handle every program got would make every program look as if it answered |
| What crosses | one message per request; nothing back | **A reply** ("declined", "done"): the asker can see the outcome in the exit already, a reply needs a channel back to an asker that may have gone, and a program that hangs would never send one -- the asker needs its timeout either way |
| Across `exec` | the endpoint ends | **Kept:** the new image may not read it, so requests would be swallowed and `kill` would wait out its timeout before forcing |
| Across `fork` | the child has none | **Inherited:** two processes sharing one endpoint, each request reaching one of them, at random |
| Who may ask | whoever may signal the target | **A capability of its own:** a request is gentler than a signal the asker may already send, so a stricter rule would protect nothing |
| A full queue | `ChannelFull` to the asker | **Dropping the oldest:** the asker would think a request delivered that was lost |
| A program with no endpoint | `NotSupported` | **Sending `SIGTERM` in the kernel:** the fallback is policy (`kill`'s, the task manager's), and a native program may not want a signal at all |

### What it costs

One more registry in the kernel, and two hooks (exec, exit). A Linux-ABI
program cannot register -- it is asked with signals, as on Linux -- unless a
SlateOS-aware one calls the native numbers through a shim; nothing needs that
today.
