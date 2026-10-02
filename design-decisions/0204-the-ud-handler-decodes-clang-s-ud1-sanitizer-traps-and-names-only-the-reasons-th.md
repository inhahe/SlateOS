## §204 — The #UD handler decodes clang's `ud1` sanitizer traps, and names only the reasons that were actually measured

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When a program does something the compiler was told to check for
— an array read past its end, an integer overflow, a function call redirected
somewhere it should not go — the compiler does not print a message. It plants
a deliberately invalid instruction ("trap") at that spot, and the CPU raises a
fault when execution reaches it. The kernel catches that fault. Until now it
could recognise exactly one such instruction shape, and only when the fault
came from the kernel itself; a trap in an ordinary program was reported as an
anonymous "invalid opcode" with no hint of which check failed. This adds a
decoder that reads the *reason* out of the trap instruction's own bytes and
names it, for programs as well as for the kernel.

### The thing that makes this worth a design entry: it is `ud1`, not `ud2`

Everyone's mental model of a compiler trap is `ud2` (bytes `0F 0B`) — that is
what rustc emits, and what `__builtin_trap()` emits. But clang's
`-fsanitize-trap=<check>` emits **`ud1`** (`0F B9`), because `ud1` takes an
operand and clang uses it to carry *which check failed*:

```
67 0f b9 40 02        ud1    2(%eax), %eax      <- reason 2 = cfi_check_fail
^  ^^^^^ ^^ ^^
|  |     |  +-- disp8 = the failing check's ordinal
|  |     +----- ModRM: mod=01 -> reason is in a disp8; mod=00 -> reason is 0
|  +----------- ud1 opcode
+-------------- 0x67 address-size prefix (the operand is 32-bit %eax)
```

The consequence is a trap in the project's most-repeated failure shape: **the
obvious way to check whether a sanitizer is switched on is to scan the binary
for a trap instruction, and the obvious instruction to scan for is `ud2` —
which is never there.** That scan returns "no traps found" on a *fully
instrumented* binary. It produced exactly that false negative here before being
caught. A check that cannot fire is indistinguishable from a check that passes.

### Decision 1 — decode on the ring-3 path too, before dispatch

The old handler decoded only for `rip >= 0xFFFF_8000_0000_0000`, and the
userspace branch returned immediately with no byte inspection at all. That
inverts the priority: a CFI violation *is* a ring-3 fault — protecting user
programs is the entire point of the feature — so the one path that could not
name it was the one that would see it.

- *Alternative considered:* report only when no handler catches the fault, on
  the grounds that a caught fault is not a crash. **Rejected**: a program that
  installs a handler and swallows its own CFI violation is precisely the case
  where silence is most expensive.
- *Cost accepted:* reading user memory inside a fault handler. This is safe
  here for a specific reason worth stating — the branch is reached only under
  `is_userspace_exception`, i.e. the interrupted CS had RPL 3, so the
  interrupted context was ring-3 code and held no kernel lock on this CPU. The
  helper carries a "do not call this from the ring-0 path" note for that
  reason. Bytes are read **one at a time**, stopping at the first failure, so
  an instruction at the end of the last mapped page still decodes as far as it
  goes instead of failing wholesale.
- *Spam:* nothing is printed unless the bytes decode as a deliberate trap, so
  a program probing for CPU features with a bad opcode stays quiet.

### Decision 2 — name only measured ordinals; print the rest as numbers

clang's reason ordinals are assigned by declaration order in its internal
`SanitizerHandler` list, so inserting one check renumbers every check after
it. There is no ABI promise here at all.

- *Alternative:* transcribe clang's whole table from its header. **Rejected.**
  It would be a table of ~25 names of which we had verified none, in a file
  whose entire job is to tell a human what went wrong. A confidently wrong
  cause attached to a real bug is worse than a bare number.
- *Chosen:* **measure them.** Twenty of the twenty-five were established by
  compiling a one-line function per check with a single
  `-fsanitize=X -fsanitize-trap=X` and byte-scanning the output for the
  resulting `ud1`. Those twenty are named. The five that were not reachable
  from C (Objective-C casts, nullability attributes, implicit-conversion)
  deliberately return `None` and print as `reason N (unrecognised)`.
- *Cost accepted:* a toolchain bump can silently renumber the named twenty.
  Mitigated by the self-test below and by an explicit instruction in the doc
  comment to **re-measure rather than trust the comment**.

### Decision 3 — a boot self-test, not a `#[cfg(test)]` module

The kernel binary cannot be built for the host test harness (duplicate
`panic_impl` lang item), so a `#[cfg(test)]` module in `kernel/src/idt.rs`
would never be compiled, let alone run. Writing one would have been the same
mistake this decoder exists to prevent, one level up. `ud_trap_decode_self_test`
therefore runs at boot alongside the other IDT self-tests, asserting against
byte sequences copied verbatim from real clang output — so a toolchain change
that alters the encoding fails loudly at boot instead of quietly mis-naming
every fault thereafter. It also asserts the **negative** cases (SIB and
RIP-relative addressing forms, truncated instructions, multi-byte nops),
because a decoder that answers "sanitizer trap" for arbitrary bytes would be a
net loss.

### What this does *not* do

It does **not** enable CFI. That is deferred by an operator decision (§201,
`deferred-questions.md` → D-Q2) until the first substantial C port. This is the
half that is useful regardless: it names out-of-bounds, shift, divide-by-zero,
null-dereference and sixteen other trap kinds today, on any C we already
compile, and it means the CFI decision — whenever it is taken — lands on a
kernel that can already say what happened.

**Where it lives:** `kernel/src/idt.rs` — `UdTrap`, `decode_ud_trap`,
`sanitizer_trap_name`, `report_ud_trap`, `read_user_insn_bytes`,
`ud_trap_decode_self_test`, and both branches of `handle_invalid_opcode`;
called from `kernel/src/main.rs` with the other IDT self-tests. The measured
flag set and the three false-negative traps found while verifying it are in
`deferred-questions.md` → D-Q2's 2026-08-16 amendment.
