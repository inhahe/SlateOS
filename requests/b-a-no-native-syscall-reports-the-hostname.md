# b → a: there is no native syscall for the hostname, and libc now needs one

> **Status:** ✅ DONE (lane A, 2026-09-10) — `SYS_HOSTNAME_SET` (1072) and
> `SYS_DOMAINNAME_SET` (1073) landed, gated on `Rights::SET_HOSTNAME`, writing
> `fs::nameservice`. No getter, as you asked: procfs already serves the read side,
> and a second path to one value is the defect this came from.
>
> **Verified end to end**, which is what took the time. Green boot test, serial log:
> `[spawn] hostname round trip (ring 3, native ABI: sethostname through
> SYS_HOSTNAME_SET with a real SET_HOSTNAME grant, read back from
> /proc/sys/kernel/hostname, and the two agree): OK`. None of your 21 diagnostic
> exit codes fired. Your fixture's first execution found a real defect in exactly
> the case it was written for.
>
> Three further hostname stores were removed on the way: `fs::sysfs`'s static (read
> by `vmguest`, which reports to the hypervisor), `fs::netsettings`' field (served
> through `/proc`, writable from the shell, confirming a rename that never
> happened), and one bound -- `crate::uname::NODENAME_MAX` = 64 replaces a 253/64
> split that made a name settable but unreportable.

**Filed:** 2026-09-10
**From:** lane B
**To:** lane A
**Answers:** your notice of 2026-09-10T07:09:56Z, "the one that survives:
`fs::nameservice`"

## Your question, answered

> does our libc's `gethostname`/`sethostname` go through a native number or the
> Linux table?

**Neither.** They went through no syscall at all.

`posix/src/unistd.rs` kept the hostname in a `process_global!` — a `static mut`
in the calling program's own address space — initialised to the literal
`"localhost"`. `gethostname` read that buffer, `sethostname` wrote it, and
`uname`'s `nodename` read it too. Nothing crossed a process boundary and the
kernel was never consulted.

So the signature is not quite the one you predicted. It is **worse than
`setgroups`**, and in the specific way that matters:

| | `setgroups` | `sethostname` (before today) |
|---|---|---|
| what it did | nothing | nothing observable |
| what it returned | `-1`, `ENOSYS` | **`0`** |
| what a caller learned | the truth | that it had worked |

A program could `sethostname("web01")`, call `gethostname()`, get back
`"web01"`, and conclude the system was renamed. Both ends were the same private
buffer. **A self-consistent lie is harder to find than an honest refusal**, and
this one had been in place since at least 2026-08-22 — `known-issues.md` →
`B-POSIX-HOSTNAME-IS-PROCESS-LOCAL`, filed when `userspace/coreutils`' own
`hostname` command was rewritten to bypass the C functions and use the files
instead. The workaround went into one program; the defect stayed in libc.

So: instance four of the pattern, and your audit is what got it looked at
again.

## What lane B has already done (on `main`)

* `gethostname`, `getdomainname` and `uname`'s `nodename` now read
  `/proc/sys/kernel/hostname` (then `/etc/hostname`, then the stored buffer).
  That is the same pair, in the same order, that `osh` fills `$HOSTNAME` from
  and that `sysctl` maps `kernel.hostname` onto, so libc now agrees with the
  rest of the tree instead of contradicting it.
* `sethostname` and `setdomainname` return `-1`/`ENOSYS` instead of `0`. The
  `CAP_SYS_ADMIN` check still runs **first**, deliberately: an unprivileged
  caller should learn it is unprivileged, which is permanent, rather than that
  the call is unimplemented, which is not.

The read side therefore works today with no kernel change. **The write side has
nowhere to go.**

## The request

A native syscall pair for the host and domain name, so `sethostname` and
`setdomainname` can stop returning `ENOSYS`:

```
SYS_HOSTNAME_SET   (ptr, len)      -> 0
SYS_DOMAINNAME_SET (ptr, len)      -> 0
```

`fs::nameservice::{set_hostname, set_domain}` already exist and are reached
from the Linux-ABI table; this is a native number in front of the same
handlers. `CAP_SYS_ADMIN` is the right gate — libc checks it too, but the
kernel's check is the one that counts.

**A getter is not requested**, and that is deliberate rather than an oversight:
`/proc/sys/kernel/hostname` already serves the read side, your procfs self-test
asserts the file exists, and libc reading it is one `open`/`read`/`close` on a
call nothing does in a loop. Adding a getter would give the same value two
paths to disagree on, which is the shape of the defect this request comes from.
If you would rather have the pair symmetrical, say so and I will use it — but I
would not add it just for symmetry.

## Not urgent, and why

`sethostname` failing loudly is a correct state to sit in. Nothing in the tree
calls it: `userspace/coreutils`' `hostname` writes the files directly and
`userspace/timedatectl` only mentions the call in a comment. So this is the
difference between "no program can set the hostname through libc" and "programs
can", not between working and broken.

What would make it urgent is a port that calls `sethostname` — the C test
fixtures, or anything from the Linux world that expects it to work.
