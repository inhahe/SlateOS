## 1018. The C-ABI route to the libc is a crate, not a convention

**Lane:** B
**Date:** 2026-09-10
**Decided by:** Claude (autonomous)

**In short:** Our C library gets compiled twice, and only one of the two copies
actually works. §768 decided that programs must reach the working one, and said
so as a rule for people to follow. In the six days that followed, the lane that
wrote the rule broke it three times, and the third time it stopped the whole
project's `main` branch from building. This entry replaces the rule with a
small library that only offers the correct route, so following it is no longer
something anyone has to remember.

### What happened

§768 ends with a sentence that turned out to be a specification, not a summary:

> The correct route and the incorrect one are indistinguishable at the call
> site, so the call site must not be where the choice is made.

It was written after three crates independently drew entropy through
`posix::random::fill` -- the `posix` crate reached as a *Rust dependency*,
which compiles a second libc with every syscall stubbed to `-ENOSYS`. The
remedy chosen there was a single function, `randrange::fill_secret`, rather
than three corrected call sites. That remedy was correct and it held: no
crate has drawn entropy the wrong way since.

What did not hold was the general rule. Between 2026-09-04 and 2026-09-10,
`userspace/swapon` (`swapon`, `swapoff`, and `get_errno` twice),
`userspace/powerctl` (`sync`) and `userspace/dhcpcd` (`sethostname`,
`get_errno`) all reached `posix` as a Rust dependency. All three were written
by the lane that made the §768 decision. Two of them were written *after* lane
A had already reported the first.

`scripts/check-one-libc-per-process.py` caught it, which is the gate working --
but it caught it on `main`, where it runs before the build and so blocked all
three lanes from boot-testing, including the `SET_HOSTNAME` grant lane B was
waiting on.

### The observation that decided the shape of the fix

**Both halves of the defect were wrong in the same direction, so they
cancelled.** `swapon` called the rlib's stubbed `swapon`, which set `errno` in
the *rlib's* cell -- and then read it back with `posix::errno::get_errno()`,
which reads that same rlib cell. Self-consistent by accident. The program
printed a plausible message and nothing looked wrong.

That means a *partial* fix would have been worse than the bug. Correcting only
the call -- routing `swapon` through `libc.a` while leaving `get_errno` -- would
have had the linked library set `errno` in its own cell and the program read a
different, untouched one, reporting `errno 0` for a real failure. This is the
project's recurring compensating-defect shape, and it is the reason the fix had
to name both halves at once. Lane A's report did name both, which is why it was
actionable.

### The options

**A. Fix the four call sites.** Add `unsafe extern "C"` blocks to the three
crates, as `randrange::fill_from_kernel` does.

*What changes:* the gate goes green today.

*Rejected.* This is what §768 already asked for in prose, and it has now failed
three times in a week under the author of the prose. A fourth restatement of a
rule that has been broken by everyone who has read it -- which is one person --
predicts a fourth violation, not compliance.

**B. Forbid the `posix` rlib outright.** Make §768's Option B (rejected there)
the rule now that the violations are concrete.

*Rejected for the same reason it was rejected in §768, which has not changed:*
fourteen crates still list `posix`, and every one of them uses only `crypt` or
`ed25519` -- pure arithmetic over caller-owned buffers, where a second copy
computes the same answer and there is no state to disagree about. Four of those
crates are other lanes'. A gate that red-lights another lane's build over a
provably harmless use is a gate that gets bypassed.

**C (chosen). A crate that offers only the correct route.** `libcall`, at the
workspace root, wrapping `sync`, `swapon`, `swapoff`, `sethostname` and
`setdomainname`, each with a `#[cfg(unix)]` arm declaring the C symbol and a
host arm that declines with `ENOSYS`.

*What changes:* the three crates drop `posix` from `[dependencies]` entirely.
There is no longer a wrong route reachable from them, so the choice §768 says
must not be made at the call site is not made there.

### Two details that are decisions rather than mechanics

**`errno` is returned, never fetched.** Every fallible call gives back
`Result<(), i32>` carrying the `errno` read inside the same function,
immediately after the failure, through `__errno_location`. The alternative --
exposing a `libcall::errno()` -- would have preserved the exact hazard in a new
namespace. With the value returned, fetching it has nowhere left to live, so
the second half of the defect is unreachable rather than merely discouraged.

**The constants are restated, not re-exported.** `EPERM`, `ENOSYS`,
`SWAP_FLAG_PRIO_MASK` and the rest are declared in `libcall`. Re-exporting
`posix`'s would put `posix` back in `[dependencies]`, which is the line the
crate exists to delete -- and lane A confirmed that a `pub const` cannot
instantiate a second libc, so naming them was never the *bug*; needing the
dependency to name them is.

This is the weaker half of the decision and it is worth saying so: restating a
value is how one truth becomes two sources, which is a shape this lane spent
the same week fixing in three other places (`sysfs`'s private hostname,
`uname` versus `/proc`, the process-local name buffer). What makes it
acceptable here is that the drift is *tested against*, not merely unlikely:
`constants_agree_with_posix` asserts every value against `posix`'s through a
**dev**-dependency, which exists in the test binary and in no program. If a
third source of these values ever appears, this decision should be revisited
rather than extended.
