### B-NO-CLD-ON-INTERRUPT-ENTRY. Ring 3 could set the direction flag and make every `rep`-string op in the kernel — `memset`/`memcpy` included — run backwards — 2026-08-12 — ✅ FIXED 2026-08-12 (`kernel/src/idt.rs`)

**What.** None of the three `global_asm!` ISR stub macros in `kernel/src/idt.rs`
(`isr_stub_no_error`, `isr_stub_with_error`, `irq_stub`) issued `cld`. There was
no `cld` anywhere in `kernel/src/` at all.

An IDT gate does **not** clear DF. Loading the new RFLAGS clears TF, NT, RF and
VM; DF is explicitly left alone (Intel SDM Vol. 3A §6.12.1). So DF on entry to
every exception and every hardware IRQ is whatever the interrupted context left
in it — and for a ring-3 interrupt, that is whatever userspace chose. `std` is
an unprivileged instruction, one byte, and ordinary glibc string routines emit
it.

**Why it matters.** The SysV AMD64 ABI requires DF = 0 at every function
boundary, and all compiled kernel code silently depends on it. LLVM lowers
`write_bytes` and `copy_nonoverlapping` — and therefore `[T]::fill`,
`slice::copy_from_slice`, `Vec::extend_from_slice`, and every large struct
move — into `memset`/`memcpy` calls whose `rep stosb`/`rep movsb` bodies walk
**backwards** when DF = 1, writing `[p - len, p)` instead of `[p, p + len)`.
`mm::rawmem::fill_u8` uses `rep stosb` directly for the same reason.

So a ring-3 thread that executes `std` and then waits for a timer tick gets the
entire scheduler, the heap allocator and the serial printer to run with every
string operation reversed. Each one scribbles over the memory immediately
*before* its intended destination. That is:

- **a security hole** — a controlled, unprivileged, no-syscall-needed way for
  userspace to corrupt kernel memory adjacent to whatever the preempting code
  path happens to touch. Linux closes it with a `cld` in its interrupt entry
  path (`arch/x86/entry/entry_64.S`) for exactly this reason;
- **a plausible root cause for B-KNULLJUMP**, the rare nondeterministic heap
  corruption. The shape matches unusually well: it needs userspace running (it
  does not reproduce in early boot), the damage lands at an address depending on
  which instruction the interrupt preempted, the corruption is silent and its
  detection is arbitrarily delayed, and the per-boot probability tracks "did any
  thread happen to be inside a DF = 1 window at a tick boundary" — which is the
  right shape for a base rate near 1-in-120 boots.

**The precondition is confirmed, not assumed.** Disassembling the *exact*
`libc.so.6` that `scripts/create-ext4-rootfs.sh` stages into `rootfs.ext4` finds
one `std` in the whole library, and it is in `__memmove_erms`:

```
  ba8e0:  endbr64                     <-- __memmove_erms
  ba8e4:  mov    %rdi,%rax
  ba8ef:  cmp    %rsi,%rdi
  ba8f2:  jb     ba8ff                ; dst < src  -> forward
  ba8f6:  lea    (%rsi,%rcx,1),%rdx
  ba8fd:  jb     ba902                ; dst < src+n -> overlapping, backward
  ba8ff:  rep movsb                   ; forward path
  ba901:  ret
  ba902:  lea    -0x1(%rdi,%rcx,1),%rdi
  ba907:  lea    -0x1(%rsi,%rcx,1),%rsi
  ba90c:  std                         <-- DF = 1 from here …
  ba90d:  rep movsb
  ba90f:  cld                         <-- … to here
  ba910:  ret
```

So any ring-3 `memmove(dst, src, n)` with `src < dst < src + n` — an ordinary
forward-shifting overlapped copy, which is what stdio buffer compaction and
insert-at-front do — runs with **DF = 1 across the whole `rep movsb`**. That
window is not an instruction or two: `rep movsb` is architecturally interruptible
between iterations (RIP stays on the prefix so it can resume), so the window is
*proportional to the copy length*. A large overlapping memmove is a wide open
door for a timer tick.

**And the corruption *class* matches, not just the timing.** B-KNULLJUMP is
specifically a jump through a **null** code pointer (`RIP=0x0`, `error=0x10` —
kernel instruction fetch of a not-present page). That is exactly what a
backwards `memset` manufactures. Most `memset`s in Rust code are *zero* fills —
`vec![0; n]`, `MaybeUninit::zeroed`, `Default`-style struct initialisation, a
cleared buffer — and a zero fill running backwards writes zeros over
`[dst - n, dst)`, i.e. it zeroes whatever object happens to sit immediately
*before* the buffer being cleared. Any function pointer in that region becomes
null, and the next call through it lands at `0x0`.

So the hypothesised chain is fully concrete:

1. ring 3 calls `memmove` with an overlapping forward shift → `std`,
2. a timer tick lands inside the length-proportional `rep movsb` window,
3. the kernel is entered with DF = 1 (no `cld` at the gate),
4. any zeroing `memset` on that path clears the memory *before* its buffer,
5. a callback/vtable/return-address slot in that memory becomes null,
6. the kernel later calls through it → `RIP=0x0`.

Step 6 is B-KNULLJUMP's exact observed signature, and steps 1–3 are now
established fact rather than conjecture.

**Still unproven: that this is what actually happened.** What is established is
that (a) the kernel entered with whatever DF ring 3 had, (b) ring-3 glibc
demonstrably sets DF for interruptible, length-proportional windows, and (c) the
resulting corruption primitive produces precisely the observed failure class.
What is *not* established is that the one caught instance came through this path
rather than another — a use-after-free on a callback pointer, the original
suspicion recorded in `B-KNULLJUMP-SIGNAL`, produces the same signature and
remains possible. The bug above is worth fixing on its own terms regardless. If
B-KNULLJUMP survives this fix, the hypothesis is disproved.

**Note the asymmetry that hid this.** The SYSCALL path was already correct:
`kernel/src/syscall/entry.rs` programs `IA32_FMASK` bit 10, so the CPU clears DF
as part of the transition, and the comment there even says *"Bit 10 = DF
(direction) — ensure forward string ops in kernel."* Whoever wrote that knew the
hazard; only the IDT-gated half was left uncovered. A grep for `DF` or
`direction` finds the handled case and nothing to suggest the unhandled one,
which is why review kept passing over it.

**The fix.** `"cld"` as the *first* instruction of all three stub macros —
first, rather than tucked in just before the `call`, so no later edit can
insert a string operation ahead of it. No `std` is needed on the way out:
`iretq` restores the whole saved RFLAGS, DF included, so the interrupted context
gets its own flag back untouched.

`mm::rawmem::fill_u8` also grew its own `cld` inside the `asm!` block (and
consequently dropped `options(preserves_flags)`), so the helper is correct on
its own terms rather than by trusting a caller-side invariant. Its SAFETY
comment previously asserted that "the SysV ABI guarantees DF = 0 at every
function boundary" — true of compiled code, but not of the machine, and exactly
the assumption this bug violates. That comment has been corrected.

**Regression test.** `idt::df_on_entry_self_test()`, run at boot right after
`mm::rawmem::self_test()`. It sets DF and executes `int3` **in a single `asm!`
block** — they must not be separable, or the compiler could schedule a `memcpy`
into the window and corrupt memory with the very bug under test — and checks a
flag recorded at the top of `handle_breakpoint`, which observes DF as the
handler sees it. A second block confirms `iretq` hands the caller's DF back.
Without the stub `cld`, the first assertion is the only visible failure; the
real symptom is silent corruption somewhere else entirely, which is why a direct
test earns its keep here.
