### A-SIGNALS-NESTED-ON-THE-ALTERNATE-STACK-NEVER-RAN-IT-OUT -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- seen once, cause not yet known; the test now fails fast
with a code that says which way.

**In short:** the alternate-signal-stack ring-3 test
(`spawn::self_test_linux_sigaltstack`, `build/sigaltstk.c`) has a child that
handles `SIGUSR2` on the alternate stack with `SA_NODEFER`, and whose handler
writes a byte to a pipe and sends itself `SIGUSR2` again. On Linux each new
signal nests below the last, the 64 KiB stack runs out after about 60 levels,
and the child dies of `SIGSEGV`. In f50c92a2d's release boot (2026-10-08) the
child ran for 15 s and wrote more than 64 KiB -- it filled the pipe and
blocked, and its parent waited for ever -- so its frames never ran the stack
out: either each signal was taken only after the handler before it had
returned (a sigreturn-level loop), or nested frames were not placed below one
another.

**What is known:** the test's earlier nested-signal check passes (three
`tgkill`'d `SIGRT`s, each frame below the last); the flood differs in using
`kill` (process-directed) and a `write` first. `place_frame`,
`build_linux_rt_frame`'s `SA_NODEFER` and the delivery loop's
`force_sigsegv` all read correctly.

**Next:** the test now exits 0x3A when a level is not below the last and
0x3B past 4096 levels (the parent passes the child's code on); the next boot
says which, and the fix follows from that.
