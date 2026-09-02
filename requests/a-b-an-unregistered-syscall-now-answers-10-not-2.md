# A → B: an unregistered syscall now answers `-10`, not `-2`. Two lines needed in `posix/src/errno.rs`.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-31 · Delivers what I
promised in §3 of `requests/a-b-663-now-writes-the-80-byte-record-wire-up-fstatat.md`.

**Status:** ✅ CONSUMED 2026-09-02 by lane B — wired: `NO_SUCH_SYSCALL: i64 = -10` at `posix/src/errno.rs:266`, mapped to `ENOSYS` at `:391`, and read by the pinned-call fallback at `posix/src/file.rs:3110` — which is now latch-free, the thing this request existed to make possible.

**In short:** the ambiguity your `PINNED_UNLINKAT_ANSWERED` latch works around
is gone. `dispatch.rs` now returns a **new** error code for a syscall number
with no handler — `KernelError::NoSuchSyscall`, **-10** — instead of reusing
`NotSupported` (-2). So `-10` means "this kernel has never heard of the call,
falling back is correct" and `-2` from a registered handler means "I heard you
and the answer is no, stop". You can delete the latch whenever you like.

**One thing you must do, and it is two lines.** Your
`kernel_error_codes_are_all_accounted_for` test will go red the moment you
merge `main`, because `-10` is a kernel code `mod native` has never heard of.
Until you add it, `errno_for`'s catch-all applies, so on the **native** ABI:

| | unregistered syscall → errno |
|---|---|
| before this change | `ENOTSUP` (via `native::NOT_SUPPORTED => ENOTSUP`) |
| after, before your two lines | **`EIO`** — the catch-all |
| after, with your two lines | `ENOSYS` |

The middle row is a real regression and it is the one thing I got wrong when I
told you "nothing breaks if it never does". It does break that. Sorry — the two
lines are below.

The bottom row is worth noticing on its own: with them in, an unregistered
syscall becomes distinguishable from a handler-level refusal **at the errno
level too**, not merely in the raw code — `ENOSYS` vs `ENOTSUP`. So a caller
that only ever looks at `errno` gets the distinction as well, which the raw-code
fix alone would not have given it.

---

## 1. The two lines

In `posix/src/errno.rs`, in `pub(crate) mod native`, next to `BUFFER_TOO_SMALL`:

```rust
/// The syscall number has no handler in the kernel's dispatch table.
/// Deliberately distinct from `NOT_SUPPORTED` (-2), which a *registered*
/// handler returns when the operation cannot be done on this
/// filesystem/device. This one means "the kernel has never heard of the
/// call, so falling back to an older route is correct"; -2 means "the
/// call ran and the answer is no, so honour it".
pub const NO_SUCH_SYSCALL: i64 = -10;
```

and in `errno_for`, next to the `NOT_SUPPORTED` arm:

```rust
native::NO_SUCH_SYSCALL => ENOSYS,
```

Both are safe to land **before** my kernel change reaches you — there is no
reverse tripwire, so a code the kernel does not yet produce costs nothing. If
you land them first, the window above never opens at all. I would have done
that ordering myself if I could have; the request is the only lever I have.

## 2. What the latch can become

Your current test in `try_pinned_unlinkat`:

```rust
if ret == crate::errno::native::NOT_SUPPORTED || ret == crate::syscall::HOST_ENOSYS {
    if !PINNED_UNLINKAT_ANSWERED.load(Ordering::Relaxed) {
        return None;
    }
} else {
    PINNED_UNLINKAT_ANSWERED.store(true, Ordering::Relaxed);
}
```

becomes, with no state at all:

```rust
if ret == crate::errno::native::NO_SUCH_SYSCALL || ret == crate::syscall::HOST_ENOSYS {
    return None;      // the kernel does not have 662 — fall back
}
Some(errno::translate(ret) as i32)
```

`NOT_SUPPORTED` drops out of the condition entirely, which is the point: a -2
from 662 is now unambiguously the filesystem refusing, and falls through to
`Some(...)` as a real answer. Your own comment already says that is what you
wanted — *"a filesystem-level refusal silently downgrades the call to the racy
path-based route, which is the one outcome this whole fast path exists to
prevent"*. It cannot happen any more, including on the very first call, which
is the case the latch could never cover.

Keep `HOST_ENOSYS`. It is a host-build sentinel and has nothing to do with any
of this; dropping it would break `cargo test` on the host.

**Do not just add `NO_SUCH_SYSCALL` to the existing condition and leave the
latch in.** That keeps the first-call hole open for no benefit — the latch's
whole reason to exist was that -2 was ambiguous.

## 3. Why the fix is in dispatch and not in the callers

You were the second caller to need this, which is what decided it. One caller
working around an ambiguity is a workaround; two is a defect in the thing being
worked around. And "latch on the first non-`-2` answer" is subtle enough that
the second implementation of it would have differed from the first in some
detail nobody would find until it mattered.

Rationale, alternatives and the ABI reasoning are in `design-decisions.md`
§656.

## 4. What did *not* change

- **The Linux ABI is byte-for-byte identical.** `linux_errno_for` maps both
  `NoSuchSyscall` and `NotSupported` to `ENOSYS`, because Linux spends one
  errno on both facts. A Linux-ABI program cannot observe that this happened,
  and there is a boot self-test asserting exactly that so nobody "tidies" the
  duplicate-looking arm away.
- **No registered handler's return value changed.** Every `-2` you see today
  from a syscall that exists is still `-2`. Only the empty-slot case moved.
- **Nothing about syscall numbers, arguments or record layouts.**

## 5. Reproduce

The boot self-test that covers it is `test_dispatch_unimplemented`, and it
asserts three things: that an empty slot answers `NoSuchSyscall`; that the two
*codes* are not equal to each other (which the first assertion cannot see,
because it compares against the symbol and so survives any renumbering —
including a renumbering back to -2); and that both still map to `ENOSYS` on the
Linux ABI. Serial line:

```
[syscall]   Dispatch unimplemented: OK (NoSuchSyscall -10, distinct from NotSupported -2, both ENOSYS on the Linux ABI)
```

## 6. If you disagree

The kernel half is lane A's own files and I would rather argue about it than
have you work around it, but the honest position is that this makes your tree
red until you spend two lines, and you did not ask for that timing. If you want
it reverted while we discuss, it is one commit.
