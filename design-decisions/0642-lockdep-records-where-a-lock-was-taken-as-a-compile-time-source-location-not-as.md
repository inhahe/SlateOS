## 642. lockdep records *where a lock was taken* as a compile-time source location, not as a return address recovered from the stack

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane A
**Lane:** A

**In short:** when the kernel detects that two locks are being taken in
opposite orders — the classic way to deadlock — it prints a report, and the
most useful line in that report is *which piece of code took each lock*. We
used to work that out by reading the CPU's stack at run time: walk back one
call frame, take the address stored there, look it up in the symbol table.
That was wrong in every optimised build, in three separate ways, and each way
shifted the answer by exactly one call — so the report confidently named a
function that had never touched the lock. It is now a `file:line:column`
supplied by the compiler, which knows the answer for certain and cannot be
optimised into a different one.

**Decision.** `lockdep::lock_acquire` is `#[track_caller]` and stores
`Location::caller()`. `sync::Mutex::lock` and `sync::Mutex::try_lock` carry the
attribute too, so the location propagates through them to the code that actually
took the lock. `CLASS_SITE` holds `AtomicPtr<Location<'static>>` instead of a
`usize` instruction pointer. The frame walk (`caller_ips`), its companion array
`CLASS_SITE_UP`, the `via:` line in violation reports, and the `#[inline(never)]`
test probe that existed to give the walk a frame to find are all deleted.

### Why the stack walk could not be repaired

Three independent optimiser liberties each erase exactly one frame: the callee
may be **inlined** into its caller; a one-call body may be emitted as `jmp`
rather than `call` (**sibling call**), reusing the frame instead of pushing one;
and the walker's own `push rbp; mov rsp,rbp` prologue may be **shrink-wrapped**
below the instruction that reads `rbp`, so it reads its caller's frame pointer.
The first two were closed with attributes, one per round of diagnosis. The third
cannot be: it is not about who is inlined into whom, and
`-C force-frame-pointers=yes` does not help, because it guarantees the frame
pointer is *maintained*, not that it is established before the first instruction
that reads it. **A function cannot assume its own prologue has run.** The full
history, including the disassembly that settled it, is
`A-LOCKDEP-SELF-TEST-PANICS-ON-EVERY-RELEASE-BUILD` in `known-issues.md`.

### The alternatives, and why not

| Option | Why not |
|---|---|
| More `#[inline(never)]` / `#[no_sanitize]`-style attributes on the walker | Closes hazards 1 and 2, which is what rounds 1 and 2 did. Nothing source-level closes hazard 3. |
| Read the return address from `[rsp]` on entry via a naked shim | Correct in principle, and it is a naked-function ABI dance in `asm!` per call site, for an answer worse than the one the compiler will hand over for free. |
| Keep the walk, add a compiler barrier before the `rbp` read | A barrier orders memory, not the prologue. There is no source-level construct that forces a prologue to precede an `asm!` block. |
| A frame-pointer-free unwinder (`.eh_frame`/`.pdata`) | Genuinely correct, and vastly more machinery than the problem needs: a table parser and its own correctness story, in the kernel, to answer a question the front end already knows. Worth it for *panic* backtraces, where the frames are not the caller's own; not worth it for "who called me". |
| Pass the site explicitly as a `&'static Location` parameter | This is `#[track_caller]`, spelled by hand at every call site. |

### The costs, stated plainly

- **`#[track_caller]` is contagious.** It has to be on every function between the
  user's code and `lock_acquire`, or the chain stops and the location becomes
  that function's own line. Today that is `Mutex::lock` and `Mutex::try_lock`;
  any future wrapper that takes a lock on someone's behalf must carry it too.
- **It costs an implicit argument.** A `#[track_caller]` function receives a
  hidden `&Location` pointer, and calls to it are marginally larger. On
  `Mutex::lock` this is a pointer to a static passed in a register on a path that
  already does `preempt_disable` + `ensure_registered` + an atomic — not
  measurable against that.
- **It degrades silently, which is the real risk.** Drop the attribute and every
  lock in the kernel is reported as taken at one line of `sync.rs` — a real
  location, plausible in a report, and wrong for every lock. That is precisely
  the failure the site recording exists to prevent, so it is gated rather than
  merely commented: `lockdep`'s test 9b has one case per attribute and each
  asserts the **exact** expected line (captured with `line!() + 1` immediately
  above the acquire). An exact line is the only assertion that both silent
  degradations fail.

### What got better besides correctness

Reports now read `kernel/src/fs/vfs.rs:812:19` instead of
`kernel::sync::Mutex<T>::lock+0x6f`. That is not cosmetic: the old form was the
reason `CLASS_SITE_UP` existed at all. A real AB/BA inversion in boot batch 31
reported *both* sides as `kernel::sync::Mutex<T>::lock+0x6f`, differing only in
the monomorphisation hash — it said two different types were locked and refused
to say by whom, so a second array recorded the caller's caller as a workaround.
`#[track_caller]` reaches the same answer in one step, and reaches it correctly,
so the workaround and its extra report line are gone.

### If this turns out wrong

`CLASS_SITE`'s type and three attributes. The information stored is strictly
richer than a return address (a `Location` names file, line *and* column, where a
symbol+offset names neither line nor column), so nothing that consumed the old
form has lost anything. Reverting would mean reintroducing `caller_ips` from
git history — and it would reintroduce the bug, since none of the three hazards
has gone away.
