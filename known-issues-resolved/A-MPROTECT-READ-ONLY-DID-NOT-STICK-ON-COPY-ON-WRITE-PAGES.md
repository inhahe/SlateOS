### [A] A-MPROTECT-READ-ONLY-DID-NOT-STICK-ON-COPY-ON-WRITE-PAGES: a page made read-only after a fork could still be written -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found alongside
`A-FORK-MADE-SHARED-MEMORY-COPY-ON-WRITE`.

**In short:** a program can mark a page of its memory read-only. A JIT
compiler does it to code it has finished writing, so a bug cannot overwrite
the code later. On SlateOS that did not work on a page that a `fork` had
left copy-on-write: the next write was treated as "copy the page first",
the copy was made writable, and the write went through.

**Where.** `kernel/src/syscall/linux.rs` `mprotect_core`, which serves both
ABIs (native `SYS_MPROTECT` calls it too). On a read-only request it cleared
`WRITABLE` and kept `COW`. `mm::cow::resolve_cow_fault` treats any
present-and-`COW` page as writable-after-copy and never checks the
protection the program asked for. So `PROT_READ` did not stick until the
page had been copied once. `mm::protect::mprotect` had the opposite fault:
it replaced a leaf's flags wholesale, so a copy-on-write page made writable
would be written in place, into the frame the other process still maps.
Nothing calls it except its own self-test, so that half was not reachable.

**The fix.** `page_table::user_protect_flags` is now the one rule both
mprotects apply to a present leaf. `COW` means "logically writable, copy
first" and nothing else.
- A read-only request clears `COW`.
- A writable request gets `COW` instead of `WRITABLE` when the frame is
  referenced more than once (the test `resolve_cow_fault` itself uses), and
  keeps `COW` on a page that has it.
- A page shared by design gets exactly what was asked.
- Every other bit (the memory type, `SHARED`) is kept.
`mm::protect::mprotect` goes through the same rule
(`page_table::change_user_protection`).

**Tests.** `mm::protect` self-test 6 runs the rule on a real frame:
- a sole owner is made writable in place;
- a `COW` page stays `COW` on RW and loses it on RO;
- a doubly referenced page gets `COW`, not `WRITABLE`;
- a shared page gets exactly the request, and keeps its memory type and
  `SHARED`;
- `PROT_NONE` removes user access.
