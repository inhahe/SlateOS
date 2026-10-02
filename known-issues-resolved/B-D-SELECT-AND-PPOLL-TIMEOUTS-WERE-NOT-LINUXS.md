### [D] B-D-SELECT-AND-PPOLL-TIMEOUTS-WERE-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/poll.rs` -- `select`, `pselect`, `ppoll`.

**In short:** `select`, `pselect` and `ppoll` wait for input on several
descriptors at once, up to a timeout. Ours judged the timeout after
everything else, never refused a bad one -- a negative `ppoll` timeout
waited a millisecond instead of failing -- and `select` never told the
caller how much of the timeout was left, which Linux does and event loops
written for Linux use to keep a deadline.

**What was wrong, against glibc 2.39 and Linux 6.6 (fs/select.c):**

| | was | Linux, and now |
|---|---|---|
| `select`'s negative seconds or microseconds | treated as zero | `EINVAL`, before `nfds` is looked at (glibc's own check) |
| `select`'s microseconds past a second | added in as nanoseconds | carried into the seconds; read as a 32-bit `int`, as glibc reads them |
| `select`'s `*timeout` on return | untouched | what is left of the wait, on every return -- the kernel's update, which glibc hands back; a zero timeout stays zero |
| `pselect` / `ppoll` with a `timespec` outside `timespec64_valid` | converted without a check: a negative `ppoll` timeout waited 1 ms | `EINVAL`, before `nfds` and `fds` |
| `ppoll` past `i32::MAX` milliseconds | cut to about 24 days | the whole wait |

`pselect` and `ppoll` still do not write their timeout back: glibc passes
the kernel a copy, and POSIX makes it `const`.
