### [A] A freshly spawned process enters ring 3 with undefined registers; `rdx` holds 0x1B, which is the seven-round `exec` bug -- 2026-09-21
**Status:** ROOT CAUSE CONFIRMED by disassembly (fix written, gate written). Supersedes the fragmentation/address-validation theories below.

**In short:** when the kernel starts a new program it jumps into it without
clearing the CPU's scratch registers, so the program begins with leftover
kernel values in them. One of those leftovers, 27, was then read by the
kernel as the address of a program's argument list, which is why starting a
program via `exec` had been failing with a meaningless address error for
seven rounds of investigation.

**The proof is a disassembly of the exact binary that produced the failure**
(`target/x86_64-unknown-none/release/kernel`, built 12:25:42, whose 12:31
serial log holds the `-101`). The end of `userspace_entry_trampoline`:

```
  movl  $0x1b,  %edx        ; rdx = 0x1B  = USER_DS selector
  movl  $0x202, %esi        ; rsi = 0x202 = RFLAGS
  movl  $0x23,  %edi        ; rdi = 0x23  = USER_CS selector
  pushq %rdx / %rax / %rsi / %rdi / %rcx    ; the five IRETQ words
  iretq                     ; <-- nothing cleared, nothing restored
```

**The chain, now with no inferred link left in it:**

| step | fact |
|---|---|
| 1 | the trampoline loads `0x1B` into `edx` to push as SS, and never clears it |
| 2 | so a fresh process's first instruction runs with `rdx = 0x1B` |
| 3 | `test_exec_process`'s stub sets `rax`, `rdi`, `rsi` -- **not** `rdx` |
| 4 | `sys_process_exec_with_frame_inner`: `argv_len = if arg2 == 0 { 0 } else { arg3 }`; `arg2` **is** `rdx` = `0x1B`, nonzero |
| 5 | so it calls `read_user_vec(0x1B, arg3, ARGV_MAX)` -- address **27** |
| 6 | 27 is in the unmapped first page -> `InvalidAddress` -> **-101** |

Everything observed now has a cause. `elf_len=136` was always right, the
range check on `arg0` always passed, `0x50_0000_0000` was always far below
`USER_SPACE_END = 2^47`, and the mapping was always present -- because **the
ELF was never the problem**. The failing read was of `argv`, an argument the
error code never mentions and the caller never set.

**A claim of mine from earlier today, retracted.** I wrote that the leak
handed ring 3 *"a kernel heap pointer in `rdi`"*, reasoning that `info_raw`
arrives in `rdi` per the SysV ABI and that nothing overwrites it. The
disassembly shows `rdi` **is** overwritten -- with `0x23`. The class was
right and the register was wrong, and I had asserted the register.

The leak is real but it is somewhere else. At the `iretq`:

| register | value at ring-3 entry | severity |
|---|---|---|
| `rbp` | **a kernel stack address** -- `pushq %rbp; movq %rsp, %rbp` with no `leave` before the `iretq` | the actual leak: defeats kernel-stack address randomisation |
| `rdx`, `rsi`, `rdi` | `0x1B`, `0x202`, `0x23` | harmless as data, but see the chain above -- `rdx` is what broke `exec` |
| `rax`, `rcx` | the process's own `rsp` and entry point | harmless, it knows both |
| `rbx`, `r8`-`r15` | untouched by this function: whatever the scheduler left | unaudited, and unauditable without fixing it |

So the security finding stands and its specifics changed. That is the third
time today that reading the artefact beat reasoning from a convention, and
the second time I published the reasoning first.

**The FPU surface is already clean, which is the strongest evidence this
was an oversight rather than a decision.** I checked the obvious sibling
leak -- x87/SSE state, where `xmm` registers are 128 bits wide and
optimised `memcpy` runs through them, so kernel bytes could ride out in
them. It is handled: every task-construction site in `sched/task.rs`
(962, 1048, 1202) assigns `FpuState::new_default_boxed()`, and
`sched/fpu.rs` exists largely to make that allocation cheap. So the
kernel already takes deliberate care to hand a new task clean floating-
point state, and handed it dirty general-purpose registers beside it.

**A third consequence, latent rather than live.** System V x86-64 says
`rdx` at process entry holds a function pointer to register with
`atexit`, or zero. `0x1B` is neither. It does no harm *today* only
because our `__libc_start_main` names that parameter `_rtld_fini` and
never calls it -- verified: the identifier appears in the signature and
nowhere in the body. A **conforming** runtime, such as a real glibc or
musl binary ported in later, calls `(*rtld_fini)()` on exit and would
jump to address 27. So the safety of this is currently resting on a
parameter staying unused, which is not a property anyone is maintaining
on purpose.

That also settles the VALUE, not just the need: zero is not merely *a*
defined setting for `rdx`, it is the one the ABI specifies for "no
function to register".

**The fix, and why zero is not a matter of taste.** All four ring-3 entries
define this boundary as their semantics demand: `fork.rs` and
`thread_clone.rs` **restore** the saved set, because a child inherits;
`sys_process_exec_with_frame_inner` **zeroes** `arg0..arg5`, `rbx`, `rbp`,
`r12..r15`, because a new image inherits nothing. Fresh spawn is the fourth
new-image case and the only one defining nothing. So zeroing is what the
sibling path with identical semantics already does 60 lines away -- I had
reached for Linux's `start_thread` as the precedent and needn't have.

**Standing gate:** `scripts/check-ring3-entry-regs.py` requires every
`iretq`/`sysretq` reaching ring 3 to define all six syscall-argument
registers. Measured before writing it: the six correct sites define 6 of 6,
`spawn.rs` defined 0 of 6, and nothing sits in between, so the rule needs no
threshold. Run against the tree it named `spawn.rs:2954` and nothing else.
