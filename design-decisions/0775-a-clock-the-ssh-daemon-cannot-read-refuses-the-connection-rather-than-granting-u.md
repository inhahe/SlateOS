## 775. A clock the SSH daemon cannot read refuses the connection rather than granting unlimited login time

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** the SSH server gives someone who has not logged in yet a limited
number of seconds to finish logging in. It measures that by reading the clock
twice and subtracting. If the clock cannot be read, there is no elapsed time to
compute, and the server has to decide what to do. It now **hangs up**. The
alternative was to carry on as though no time had passed, which would have
quietly switched the limit off — and switched it off specifically for people who
have not proved who they are, who are the only people it applies to.

**What forced the choice.** The clock reader reported the number `0` when the
read failed, which is the same thing it reports for "the machine booted this
instant". Subtracting a real start time from that underflows: a crash in a debug
build, and in a release build a huge elapsed time that disconnects every client
on arrival. Both are reachable by anyone who can open a socket. Fixing the
underflow meant making the failure representable — `Option<u64>` instead of a
magic `0` — and once the failure is representable, something has to decide what
it means. See `known-issues.md`
`TD-B-SSHD-UNDERFLOWED-THE-LOGIN-GRACE-TIMER-WHEN-THE-CLOCK-FAILED`.

| Option | *What changes:* | Why not |
|---|---|---|
| **Fail closed** (chosen) | A peer that connects while the clock syscall is failing is disconnected with "monotonic clock unavailable" instead of being let in. | A transient clock failure refuses logins that would otherwise have succeeded. |
| Fail open — treat an unreadable clock as "no time elapsed" | Nothing visible, until the clock breaks; then unauthenticated peers may hold connections open indefinitely. | Disables the bound for exactly the population it exists to bound. A limit that stops applying when a subsystem fails is not a limit, and nothing would report that it had stopped. |
| Fall back to a wall-clock reading | The timer keeps working off a clock that can be stepped. | A wall clock can jump backwards, which reintroduces the underflow this entry exists to remove, and can be moved by anyone who can set the time. |

**Why fail-closed is consistent here rather than merely cautious.** The daemon
already makes this exact call one level up: when the host key file exists but
cannot be parsed, it stops instead of generating a substitute, because serving
under a substitute identity is indistinguishable — from the client's side — from
the attack that host-key checking exists to detect. A grace timer that silently
stops bounding unauthenticated connections is the same kind of failure: the
security property is gone and the logs look normal.

**Against it, honestly:** availability. A kernel whose `SYS_CLOCK_MONOTONIC` is
briefly failing now refuses SSH logins, which is the worst moment to lose
remote access to a machine. That is a real cost and it is why this is written
down rather than assumed. It is accepted because the failure is loud — the peer
is told "monotonic clock unavailable", so the cause is in front of whoever is
diagnosing it — whereas fail-open is silent by construction. A loud refusal can
be investigated; a limit that quietly stopped applying cannot be noticed at all.

**Revisit if** the monotonic clock turns out to fail transiently in normal
operation rather than only when something is badly wrong. That would make the
availability cost real rather than theoretical, and the answer would then be to
make the timer tolerate gaps (e.g. remember the last good reading) rather than
to fail open.
