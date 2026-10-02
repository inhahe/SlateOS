## §226 — A large struct destined for the heap is allocated first and initialised in place, never built by value and then boxed

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When kernel code needs a big chunk of memory on the heap — say a
4 KiB block for saving a CPU's floating-point registers — the obvious-looking
Rust for it, `Box::new(Thing::new())`, does not actually put it on the heap
straight away. It builds the whole thing on the *stack* first, then copies it
to the heap. Kernel stacks are small and fixed (64 KiB here), so a few of those
copies in a row can eat most of one. That is exactly what crashed this kernel:
one 4 KiB field, copied through three function frames, claimed 40 640 bytes of
a 64 KiB stack and intermittently ran a task off the bottom of its own stack.
The decision is a rule: anything big that is going to live on the heap must be
allocated on the heap *first* and filled in there, never assembled on the stack
and moved.

**The threshold:** ~1 KiB. Below that the copy is noise; above it, on a 64 KiB
stack traversed by paths that are already several frames deep, it is not.

### The pattern

Wrong — and it does not look wrong:

```rust
let state = FpuState::new_default();   // 4 KiB built in this frame
let boxed = Box::new(state);           // ...then memcpy'd to the heap
```

Also wrong, and looks even more innocent, because the temporary is unnamed:

```rust
let boxed = Box::new(FpuState::new_default());
```

Right — the value never exists on the stack at all:

```rust
pub fn new_default_boxed() -> Box<Self> {
    let layout = Layout::new::<Self>();
    // SAFETY: non-zero size; an all-zero FpuState is a valid FpuState.
    let ptr = unsafe { alloc_zeroed(layout) }.cast::<Self>();
    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    // SAFETY: freshly allocated, correctly aligned, exclusively ours.
    unsafe { &mut *ptr }.write_defaults();   // patch in place
    // SAFETY: `ptr` came from the global allocator with `Self`'s layout.
    unsafe { Box::from_raw(ptr) }
}
```

`alloc_zeroed` is only correct when **all-zero is a valid value of the type**.
`FpuState` is a plain `[u8; 4096]`, so it is. `KernelFdTable` is
`[Option<FdEntry>; 256]`, and it is *not*: Rust does not promise that `None` is
all-zero bytes for an arbitrary `Option<T>`. There the pattern is
`Box::<Self>::new_uninit()` followed by an explicit `write(None)` per element,
then `assume_init()` — same shape, no illegal assumption.

### Why the alternatives lose

| Option | Why not |
|---|---|
| **Trust the optimiser to elide the copy** | It does, at `opt-level ≥ 1` — and the boot test builds *debug*. The same source measured 40 640 bytes of stack in debug and 8 960 in release. A rule that only holds in release is not a rule; it is a build configuration that happens to hide a bug. |
| **Grow the kernel stacks** | Treats the symptom, costs memory on every task forever, and leaves the copies in place to be re-discovered by the next struct that grows. |
| **Case-by-case, only where a stack overflow is observed** | This bug was intermittent, took ~1 boot in 10, was attributed to the wrong cause for two sessions, and needed a purpose-written disassembly tool to localise. Discovering these one crash at a time is not a strategy. |
| **A lint / `#[deny]`** | There is no such lint. Clippy's `large_stack_frames` is close but fires on the frame, not on the by-value move, and does not run against our target reliably. Would be strictly better if it existed — see *Revisit if*. |

### The cost, stated honestly

The correct form is more code and it contains `unsafe`. Three or four lines of
`SAFETY`-commented allocator work replace one line of `Box::new`. That is a real
loss in readability, and it is why the ~1 KiB threshold exists rather than
applying the rule to everything. The judgement is that for a handful of large,
long-lived structs the trade is clearly worth it, and for small ones it clearly
is not.

### How to check

`python scripts/stack-frames.py --profile debug --top 40` lists every function
by the stack its prologue claims; `--diff BEFORE_ELF AFTER_ELF` shows what a
change moved. Run it after touching a spawn, fork, or context-switch path.

Note the tool had to sum **stack-probe chains** to be useful: a function needing
more than a page emits `sub $0x1000, %rsp; movq $0, (%rsp)` repeatedly rather
than one big `sub`, so reading only the first `sub` reports exactly 4096 for
every large function — hiding precisely the ones worth finding. The first
version of the tool did that and produced a clean bill of health for a kernel
that was overflowing.

**Revisit if** a lint appears that can flag a by-value move above N bytes
(rustc's `-Zprint-type-sizes` plus a build-time check is the plausible
home-grown version), at which point the rule can be enforced mechanically
instead of by convention; or if we ever move to building the boot test at
`opt-level = 1`, which would hide the problem again rather than fix it, and so
should not be done for this reason.
