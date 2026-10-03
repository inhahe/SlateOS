### [A] ROOT CAUSE: `SYS_PROCESS_EXEC` cannot read the caller's ELF, and its own self-test reports OK because a crashed process is a zombie -- 2026-09-21
**Status:** FIXED (stamped 2026-09-25) -- both halves are in: `SYS_PROCESS_EXEC` reads the caller's ELF (the 2026-09-25 boot logs `[exec] Process 131 exec successful`), and `test_exec_process` now also asserts the target's exit code is `Some(0)`, so a crashed exec can no longer pass as a zombie. The ring-3 exit-11/8 failures this entry blamed on it turned out to have their own cause (a missing METADATA right in the rungs; see `A-TWO-RUNGS-COULD-NOT-EXEC-FOR-WANT-OF-METADATA`)

**In short:** the kernel's native "replace this program with another one"
call is broken -- it cannot read the new program's bytes out of the calling
program's memory. The test that exists to prove this feature works has been
reporting success the whole time, because the only thing it checks is that
the process ended, and a process that crashes has also ended.

**The evidence, from the boot of 2026-09-21, in six consecutive lines:**

```
[spawn] Created process 131 ("spawn-test-exec")
[spawn]   Exec test: mapped 136 bytes of target ELF at 0x5000000000
[exec] NATIVE exec FAILED -> -101 (elf_len=136)
[exception] Killing task 96 - General Protection Fault (#GP) at 0x4000000016
[thread] Process 131 has no threads left - now zombie
[spawn]   Exec (replace process image): OK
```

`-101` is `KernelError::InvalidAddress`. The caller mapped 136 bytes at
`0x5000000000` and passed that address; the kernel could not read it. The
caller then fell off the end of its own code into unmapped memory and took
a `#GP`. Four lines later the rung says **OK**.

**Why it says OK.** `test_exec_process` asserts exactly one thing:

```rust
let state = pcb::state(result.pid);
if state != Some(pcb::ProcessState::Zombie) { ... FAIL }
```

A successful exec ends with the new image calling `exit(0)` -> zombie. A
failed exec ends with the caller crashing -> **also zombie**. The assertion
cannot tell the feature working from the feature failing, and for as long
as it has existed it has been reporting the second as the first.

**This is the root cause of `ctest-coreutils-runs` exit 11**, and of
`ctest-python-repl` exit 8. `posix::execve` reads the target ELF into its
own memory and calls `SYS_PROCESS_EXEC` with that buffer. The kernel
returns `InvalidAddress`, `execve` returns -1, the child `_exit(127)`s, and
the fixture prints *"COULD NOT EXEC ... it is not on the image, or is not
executable"* -- a guess, and the wrong branch of it, which is what seven
rounds chased.

**It also explains the vector-vs-list discriminator**, which lane C and I
both treated as evidence about `execl`. fastpy's `os.execv` works because
fastpy is a Linux-ABI binary and goes through `linux_execve`; the C
fixtures are native-ABI and go through `SYS_PROCESS_EXEC`. **The split was
never `execl` versus `execv` -- it was native versus Linux ABI**, and the
two happened to line up exactly with the call form in the sample we had.
That makes the `va_trampoline` lead almost certainly a red herring, and it
was already marked unsupported rather than disproved.

**Found by instrumentation added the same morning** (`391232edb`), which
existed only because the Linux-ABI path had a failure probe and its native
sibling had none. The probe's first run found this in its first minute.

**Two fixes, and the second is not optional:**

| what | why |
|---|---|
| make `SYS_PROCESS_EXEC` read the caller's buffer | the actual defect. Likely the copy-in from user memory before the old address space is torn down |
| make `test_exec_process` assert the exec HAPPENED | asserting `Zombie` is asserting "something ended". The target ELF calls `exit(0)`, so assert the **exit code**, or that no `#GP` was taken, or both |

Without the second, the same fix could regress and the rung would go on
saying OK.
