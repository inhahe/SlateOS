## A-IO-URING-UNKNOWN-OPCODE-IS-STILL-AMBIGUOUS (lane A)

**Status:** ✅ FIXED 2026-09-02 in `d9e224706` — see the Resolution at the end
of this entry. The body below is left as written on 2026-08-31, because the
"why it was deferred" reasoning is the part worth keeping.

`execute_sqe` in `kernel/src/ipc/io_ring.rs` (~line 851) ends its opcode match
with

```rust
_ => KernelError::NotSupported.code() as i64,
```

so an SQE naming an opcode this kernel has never heard of completes with `-2` —
the same CQE result a *registered* opcode's handler gives when it ran and could
not do the thing. That is exactly the ambiguity design-decisions.md §656 removed
from the syscall dispatch table, wearing a different hat: a caller probing
whether this kernel supports, say, `IO_OP_FH_PWRITE` cannot tell "no such
opcode, fall back to the synchronous route" from "the opcode exists and this
file refused", and the two demand opposite responses.

**Why it was not swept into §656.** The syscall change had two callers asking
for it, a promise outstanding to lane B, and a boot self-test to pin it. This
has none of those, and it is a different return contract with different
callers — the CQE `res` field, not a syscall return value. Changing it in the
same commit would have been a behavioural change nobody requested, decided by
analogy rather than on its own evidence, and buried inside a commit about
something else.

**Proper fix.** Return `KernelError::NoSuchSyscall` from that arm (the variant
is about an unregistered *entry point*, and an io_uring opcode is one, though
the name reads as syscall-specific and may deserve widening if this lands), and
add an assertion to `io_ring`'s self-test in the shape of
`test_dispatch_unimplemented`: an unknown opcode gives -10, -10 is not -2, and
the Linux-ABI mapping is unchanged.

**The blast-radius question is already answered — 2026-08-31, lane A.** The fix
step above used to open with "check first whether any caller in `userspace/` or
`posix/` probes CQE `res == -2`; if one does, it needs the same two-line
treatment lane B is getting for the syscall half." That check has now been run,
so whoever picks this up does not have to re-derive it:

**No caller anywhere in the tree reads a CQE `res` and compares it to `-2`.**
The only consumers of io_uring results outside the kernel are
`posix/src/linux_io_uring.rs`, `posix/src/aio.rs` and
`posix/src/linux_aio_abi.rs`, and the sole `ENOSYS`-shaped reasoning in them
(`linux_io_uring.rs` lines 578, 1372, 1542) is about **`io_uring_setup` itself**
returning `-1`/`ENOSYS` because no real ring exists yet — it is not a probe of a
completion result. So this is a kernel-local change: one match arm plus the
self-test assertion, **no `requests/` file and no cross-lane coordination**.

That has a cost consequence worth stating plainly: the reason this stayed
deferred was never the risk, it was the absence of evidence that anyone needs
the distinction. Re-verify cheaply before acting — `rg -n 'cqe.*res|\.res\b'
posix/ userspace/ services/` and look for a comparison against a negative
literal — because the answer above is a fact about the tree on a date, not a
property of the design.

**Trigger:** a caller that needs to feature-probe io_uring opcodes, or the next
time someone writes a fallback path around a CQE result.

> **Resolution — 2026-09-02, lane A, commit `d9e224706`.**
>
> The fallthrough arm now returns `KernelError::NoSuchSyscall` (-10), exactly as
> the "Proper fix" above specified, and the widening the entry flagged as
> conditional ("may deserve widening if this lands") was done: `error.rs`'s doc
> for `NoSuchSyscall` claimed syscall dispatch was its only source, which this
> change falsifies. It now names both callers and states what the variant
> actually means — *unregistered entry point* — while keeping the name, since
> renaming would touch every use for no gain in meaning. Rationale is
> design-decisions.md §676.
>
> **The trigger never fired, and it was still right to act.** This entry was
> gated on "a caller that needs to feature-probe io_uring opcodes", and none
> appeared. It was fixed anyway because the blast-radius check that the entry
> itself demanded be re-run — it warns the no-caller finding is "a fact about
> the tree on a date, not a property of the design" — came back clean a second
> time on 2026-09-02 across `posix/`, `userspace/`, `services/` and `init/`.
> A zero-caller result cuts both ways: it is why the change is not urgent, and
> it is also why it is free. Doing it while nothing depends on the old code is
> strictly cheaper than doing it after the first fallback path has been written
> around the ambiguity — at which point the fix acquires a caller to migrate.
> Deferring is the correct default for a behavioural change nobody asked for;
> it stops being correct once the change has become purely subtractive.
>
> **The self-test asserts both halves.** `test_unknown_opcode_is_distinguishable_from_a_refusal`
> checks that the CQE result is -10 *and* that it is not -2. The second
> assertion looks redundant against the first and is not: an assertion on the
> new value alone would still pass if someone later collapsed the two codes
> back together at the `KernelError` level, which is the regression this entry
> exists to prevent. It also asserts the SQE was *processed* (one completion
> posted, not silently dropped) — a ring that dropped an unknown opcode would
> hang a caller waiting on the completion, a worse failure than the ambiguity.
>
> The sentinel opcode is `0xFF`, not `last_assigned + 1`. The highest assigned
> opcode is 17, and a test that picked 18 would begin failing the day an
> eighteenth opcode is legitimately added — turning a real feature into a
> spurious self-test failure. `0xFF` is the last number the `u8` opcode field
> will ever assign.
>
> The Linux ABI is unchanged: both codes map to `ENOSYS`, so the distinction is
> visible only on the native ABI, where the raw code is what the caller sees.
