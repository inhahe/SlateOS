## 768. One libc per process: stateful `posix` is reachable only through the C ABI, never as a Rust dependency

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** Our C library, `posix`, gets compiled two different ways. Built as
`libc.a` it contains real system calls, and that is the copy every SlateOS
program links against. But it can *also* be listed as an ordinary Rust
dependency — and when it is, it compiles a second time with every system call
replaced by a stub that just answers "not implemented". A program that does both
ends up carrying two libraries that disagree, with nothing warning it. That is
not hypothetical: it is why `ssh` and `sshd` could not generate their keys and
so could not connect at all. The decision is where to draw the line: the parts
of `posix` that are pure arithmetic stay usable as a Rust dependency, and
everything that talks to the kernel or keeps state is reachable only through the
linked C library.

### What forced the choice

`toolchain/x86_64-slateos.json` sets `"os": "linux"`. Every syscall in `posix`
is gated `#[cfg(target_os = "none")]`, which is the *staticlib* build. So the
rlib a program pulls in takes the host arm, where `syscallN` returns
`HOST_ENOSYS` (`-38`). `posix::random::fill` reads that as "no kernel present",
falls back to a userspace pool seeded from RDRAND, and the QEMU guest CPU
(`qemu64,+smep,+smap,+umip`) has no RDRAND — so it returned `EIO` while its error
text blamed "the kernel CSPRNG". Full measured chain in `known-issues.md`
→ `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`.

### The options

**A. Widen the gates so the rlib issues real syscalls.** Change ~1,845
`#[cfg(target_os = "none")]` to `#[cfg(any(target_os = "none", target_vendor =
"slateos"))]`.

*What changes:* `posix::random::fill` works from a program, and so does every
other `posix::` API, with no call-site changes anywhere.

*Rejected.* This makes the duplication *real* rather than removing it. The rlib
copy has its own `errno` cell, its own fd table, and its own per-thread block —
`perthread::current()`'s host arm is a `thread_local!`, a different TLS slot from
the linked libc's. Two live libcs, each believing it owns the process, produce a
file closed through one and read through the other, an `errno` set in one and
read from the other. Today's failure is loud and uniform (`-ENOSYS` on
everything). Option A trades it for silent state divergence, which is strictly
harder to find. **A stub that fails every call is a bad state; two libcs that
each half-work is a worse one.**

**B. Forbid the rlib entirely** — `compile_error!` in `posix` for any target
that is not `none` or a host test triple.

*What changes:* `apps/lockscreen` and ten other crates stop building until they
are rewritten to call `crypt` through the C ABI.

*Rejected as too blunt.* Measured: of the 11 rlib dependents, the only `posix::`
modules any of them name are `ed25519` and `crypt::*`, both pure arithmetic over
caller-owned buffers. A second copy of those computes the same answer — there is
no state for the copies to disagree about. Option B red-lights another lane's
build (`apps/lockscreen` is lane C) over a use that is provably harmless, which
is how a gate gets bypassed rather than obeyed.

**C (chosen). Split by nature, not by crate.** Pure computation over
caller-owned buffers stays available as a Rust dependency. Anything that touches
the kernel, global state, or per-thread state is reachable from a program only
through the C ABI symbols in `libc.a`.

*What changes:* the five entropy call sites in `ssh`/`sshd` move to
`randrange::fill_secret`, which goes through the linked libc's `getrandom`
symbol; `ssh-keygen`'s hand-written third copy of that same extern is deleted;
`ed25519`/`crypt` users are untouched.

### Why the line is where it is

The distinguishing property is not "does it call the kernel" but **does it have
anything to disagree about**. `ed25519::sign` reads its input and writes its
output through pointers the caller owns; two copies of it are two copies of a
function, which is a size cost and nothing else. `random::fill` consults a
process-global cache (`HW_RNG`) and a per-thread pool; two copies are two
*states*, and the program has no way to say which one is real.

That gives a rule that can be applied to a new module by reading it, rather than
by remembering a list: **if it has state, it goes through the C ABI.**

### The cost of the choice, stated plainly

Option C is the only one of the three that needs *ongoing* enforcement. A and B
are one-time edits that are thereafter self-maintaining; C is a rule that the
next author has to know. That cost is real and is why the entry above proposes a
two-part gate — an allowlist of pure modules, *plus* a check that the
allowlisted modules have not since grown a `syscall` or `perthread` reference.
Without the second part the allowlist is a claim made once and never rechecked,
and it will rot the way every unchecked claim in this tree has.

**Not recorded as settled policy for the operator to revisit:** this is a
`Claude (autonomous)` call about internal structure with no user-visible
behaviour attached. If the enforcement half proves unmaintainable, option B
applied narrowly (a `compile_error!` in `posix/src/syscall.rs`'s stub arm, gated
on `target_vendor = "slateos"`) is the fallback, at the price of failing a whole
crate rather than the offending call.
