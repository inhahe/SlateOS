## §260 — A user-pointer copy from kernel context checks the mapping, because the permission bypass never meant "this address exists"

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When kernel code (rather than a user program) hands the
kernel's "copy from a user pointer" helpers an address, those helpers used to
skip *all* checking and copy from whatever number they were given. A wrong
number therefore did not produce an error — it killed the machine outright,
with a page fault the kernel cannot recover from. The decision is to keep the
permission waiver (kernel code legitimately passes kernel addresses, which the
user-address rule would reject) but to still confirm the address is actually
mapped before dereferencing it, so a bad pointer comes back as an error the
caller can report.

### The shape of the hole

`mm::user::validate_user_read` / `_write` / `_ptr` open with:

```rust
if is_kernel_context() {
    return Ok(());
}
```

where "kernel context" means the running task owns no process. The waiver
itself is right and has to stay: a bare kernel task's buffers live in the
kernel half of the address space, which `validate_user_range` rejects on
principle, and dozens of fidelity self-tests depend on a bogus pointer getting
*past validation* so that a later gate in the handler can be observed.

What was wrong is that `copy_from_user` and `copy_to_user` treat a successful
validation as a licence to `rep movs` through the raw address. So in kernel
context every user-pointer API degenerated into an unchecked dereference. The
only thing standing between that and a halted kernel was a per-handler
convention: each handler happened to return early on `caller_pid() == None`
*before* reaching its copy. Nothing enforced it, nothing tested it, and it is
not a property a reviewer can check locally — it is a property of every
handler at once.

It broke the first time something ran at a place no handler owns. A stat-family
diagnostic added to `dispatch_linux`'s tail — which by construction runs after
*every* handler, including ones that returned before their own gate — re-read
the path pointer to name the file in its log line. The fidelity self-tests pass
`0x1000` as a "path" to observe an `EINVAL` gate, so the hook read address
`0x1000` from a kernel task and the boot died at 15 seconds with
`Page Fault (#PF) … address=0x1000, error=0x0`.

### The alternatives

**Guard the diagnostic and move on.** One line, and it *is* also correct on its
own terms (a kernel task has no path to name), so it was done as well. But as
the whole fix it treats a landmine as a stumble. The next thing to run outside a
handler — a tracepoint, an audit hook, an accounting pass, an io_uring
submission replayed from a kernel worker — steps on the same mine, and the
symptom is a dead machine at boot rather than a failed test naming the caller.
This is precisely the band-aid pattern `CLAUDE.md` says to stop and redesign
instead of repeating.

**Make the validators themselves strict in kernel context** — waive only the
user-half rule, keep the mapping check. This was written and then withdrawn: it
changes what a great many self-tests observe. `access(0x1000, 0)` in kernel
context is *supposed* to reach `ENOENT` by way of the bypass; under a strict
validator it stops at `EFAULT`, and the same is true across a wide spread of
gate-order tests. Those tests are not wrong — they are testing gate ordering,
and the bypass is the mechanism that lets them see past the first gate — so
breaking all of them to fix a dereference is the wrong trade.

**Check at the dereference** — chosen. The validators keep answering exactly
what they answered before, and `copy_from_user`, `copy_to_user` and
`user_atomic_u32` ask the *other* question, the one the bypass silently
dropped: is this range mapped at all. `validate_kernel_range` walks the current
address space a page at a time and additionally requires writability when the
caller intends to write. `page_table::translate` resolves 1 GiB and 2 MiB
leaves as well as 4 KiB ones, so a kernel pointer into the HHDM or the kernel
image passes normally. The cost is a page-table walk per page on a path only
kernel tasks take; no user syscall reaches it.

No CoW break and no demand-paging fault-in on that path: both are properties of
a *process's* address space, and in kernel context there is no process. A
kernel-half page that is not present will not become present by being asked.

### What this does not fix

Kernel-context code that builds a slice over a user address by hand rather than
going through the copy helpers is still unprotected. A sweep of `kernel/src`
found no such site in the syscall or IPC layers — the copy primitives are the
only routes — but the property holds by audit, not by construction. If one is
ever added, the same fault returns.

### Where it is pinned

`mm::user::self_test` test 7: a kernel-context `copy_from_user` from `0x1000`
must be `InvalidAddress`; likewise `copy_to_user` to `0x1000` and to the
address of the test function itself (mapped, `.text`, not writable); and a copy
of a mapped kernel range must still succeed and be byte-exact, so the check
cannot pass by rejecting everything. The test runs in kernel context by
construction — the boot self-test task owns no process — so a regression
re-panics the kernel inside a named test rather than a thousand lines later.
