### [D] B-D-ADJTIMEX-WAS-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/sys_timex.rs` -- `adjtimex`, `ntp_adjtime`,
`clock_adjtime`, and `adjtime` (new).

**In short:** `adjtimex` is how time daemons -- chrony, ntpd,
systemd-timesyncd -- steer the system clock, and `adjtime` is the older call
many programs still use to nudge it. Ours refused `adjtime`'s form of the
call outright, so there was no `adjtime` at all, and it judged the rest in a
different order from Linux, so an unprivileged daemon could be told its
values were wrong when Linux would have said it lacked the right to change
the clock.

**What was wrong, against Linux 6.6 (`timekeeping_validate_timex`,
`__do_adjtimex`) and glibc 2.39:**

| | was | now |
|---|---|---|
| `ADJ_OFFSET_SINGLESHOT`, `ADJ_OFFSET_SS_READ` -- `adjtime`'s modes | `EINVAL`, as unknown mode bits | Linux's: a one-time slew kept and the pending one returned; reading needs no capability |
| any other unknown mode bit | `EINVAL` | accepted: Linux has no such check |
| a change without `CAP_SYS_TIME` beside a bad value | the value's `EINVAL` | `EPERM`, which Linux asks first |
| `ADJ_FREQUENCY` that overflows when scaled | accepted | `EINVAL` |
| `time` in the reply | 0 | the current time, in microseconds or nanoseconds by `STA_NANO` |
| `adjtime(3)` | missing | glibc's: `ADJ_OFFSET_SINGLESHOT` over `clock_adjtime`, `EINVAL` past its ±2145 s, the pending slew split as glibc splits it |

**Still not Linux's:** the slew is only kept and reported -- the clock here
has no slew, so the time does not move by it -- and the discipline state is
this process's, not the system's.
