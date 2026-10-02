## §672 — a free-slot signature is tied to the slot's own address, because a constant is a value the allocator itself leaks

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** The kernel's heap marks every freed block with a fixed magic
number, so that freeing the same block twice can be spotted — the second free
sees the mark already there. That check just accused a piece of perfectly
correct code (`/proc/loadavg`) of freeing memory twice when it had only freed
it once, and the boot test failed for a day because of it. The reason is that
the magic number is a *constant*: the allocator scatters copies of it on the
kernel stack merely by reading and writing it, and a struct with an
uninitialised 8-byte hole picked one up and carried it into the heap, where it
sat at exactly the offset the check looks at. The fix is to stop using one
number for every block and instead use a number derived from the block's own
address, so a stray copy can only ever match the one block it came from.

### The defect

`fs::conformance` walks `/proc`, which runs `procfs::gen_loadavg`, which builds
a `Vec<TaskInfo>` from `sched::task_list()`. On dropping that `Vec` — its first
and only free — `poison_free` reported a double-free, `syshealth` reported
`[FAIL] Heap safety: 2 violation(s)`, and the boot test panicked.

The chain that explains it:

| Fact | Established by |
|---|---|
| The signature was the fixed constant `0xFEEDFACE`, written at bytes 8..12 | `heap.rs` — a single definition, confirmed by grep |
| `poison_free` and `check_poison` therefore place that constant on the kernel stack on **every** allocator call | reading them |
| `TaskInfo` is 176 bytes with `stack_used: Option<usize>` at offset **0**, its payload at bytes **8..16** | DWARF in the kernel ELF, read with `pyelftools` |
| `stack_used` is `None` for the idle task, so those 8 bytes are **never written** | `sched::task_list` |
| The `Vec` was `len=1 cap=1` and the one task at that point in boot is the idle task | the slot's own bytes: `id=0`, `name="idle"`, `name_len=4` |
| The uninitialised payload held `FE ED FA CE FE ED FA CE` when it reached the heap | dumping the slot |

So a live, singly-allocated buffer arrived carrying the allocator's own magic,
at the one offset where the allocator treats it as proof of a previous free.
Nothing in `procfs`, `sched`, or the slab allocator was wrong. `Option<usize>`
leaving its payload uninitialised in the `None` case is entirely legal, and
`memcpy`-ing a struct with uninitialised bytes into a heap buffer is what every
move does.

### The decision

The signature becomes `poison_magic_for(addr) = 0xFEED_FACE_FEED_FACE ^ addr`,
widened from four bytes to eight, occupying bytes 8..16; the poison zone moves
from 12.. to 16...

*What changes:* the expected word is different for every slot, so a copy that
escapes onto the stack can only ever match the slot it came from, and only if
it also lands at exactly the right offset. A collision now requires the value
*and* the address *and* the offset to agree — 64 bits of agreement rather than
32, and no longer reachable by the allocator's own leakage.

### Alternatives considered

- **Zero the uninitialised padding in `TaskInfo`.** Rejected: it fixes one
  struct. Every struct with an uninitialised 8-byte hole at offset 8 that is
  copied into the heap has this bug, and there is no way to enumerate them.
  This is a defect in the *checker*, and fixing it anywhere else leaves the
  checker able to accuse the next struct.
- **Move the signature to a header outside the user-visible bytes.** Correct in
  principle and strictly better, but it changes the slab layout — every size
  class, every offset computation, the red zone, and the KASAN shadow mapping.
  Worth doing on its own merits some day; it is not what this defect requires,
  and doing it here would have made a one-line-cause bug into a slab rewrite.
  Logged as the eventual direction, not as this fix.
- **Widen the constant to 8 bytes without tying it to the address.** Rejected:
  it lowers the odds without removing the mechanism. The value that collided
  was not random garbage — it was the allocator's own constant, placed there by
  the allocator. A wider constant is a wider constant that the allocator still
  leaks.
- **Keep the free-trace instrumentation** (16 recorded frames of the first free,
  written into each freed slot). Removed. It was built to answer "who performed
  the *first* free", and the answer turned out to be "nobody — there was only
  one". Keeping a per-free backtrace capture on the allocator's hot path to
  answer a question that no longer exists is not a trade worth making. The
  16-frame backtrace printed on *genuine* detection stays: it fires only on
  detection, and it is what named the call site in the first place.

### What this cost, and the rule it leaves behind

Roughly a day of boot-test failures, and a chain of six wrong hypotheses —
growth-`realloc` leaving a stale pointer, free-list aliasing across per-CPU
magazines, the `cache.active` guard racing an ISR, CPU migration mid-allocator,
`refill` handing out stale poison, an overridden `realloc` — each plausible,
each disproved. Two rules earned:

- **A defect that vanishes when you add a `serial_println!` has been perturbed,
  not fixed.** Adding a print to `gen_loadavg` made the boot green; it moved the
  stack frame so the temporaries were built over different garbage. The
  discriminating experiment — remove *only* the print — brought it straight
  back, and that is what proved the cause was stack contents.
- **DWARF is the authority on `#[repr(Rust)]` field layout, not the
  declaration order.** Three of the wrong hypotheses came from guessing offsets
  off the struct definition. `pyelftools` against the kernel ELF settled it in
  one command, and should have been the first step rather than the last.
