## `A-LOCKDEP-SELF-TEST-PANICS-ON-EVERY-RELEASE-BUILD` (lane A, 2026-08-30) — **fixed 2026-08-30 (took three attempts; the first two were incomplete)**

**Status:** FIXED 2026-08-30, on the third attempt, by deleting the mechanism
rather than repairing it. Rounds 1 and 2 are recorded below as written at the
time. Round 1 concluded the *test* was wrong and the code was right — it was the
other way round. Round 2 found a real cause but treated it as *the* cause; it
turned out to be one of three independent optimiser liberties that each erase
exactly one stack frame. Round 3 is the fix that shipped. The earlier rounds are
kept because the reasoning errors are the useful part: each one ended in a
plausible mechanism that fully explained the symptom and was not the whole
story.

**In short:** the kernel would not finish booting when built with optimisations
on. It panicked partway through startup, inside a self-test belonging to the
lock-order checker. The test was checking that the checker correctly records
*which piece of code took a lock* — information that appears in every deadlock
report it prints. In an optimised build it was recording the wrong function: not
the one that took the lock, but the one that called it. So every "taken at" line
in every release-build lock report pointed at innocent code. The self-test was
right to panic; three rounds were needed to fix it, and the fix in the end was
to stop asking the machine where the caller was and ask the *compiler* instead
— it knows, and it cannot be optimised into a different answer.

---

### Round 1 (wrong): "the test is asserting a property of the build"

**What happened.** `lockdep`'s test 9b checks that `caller_ips()` walks exactly
one frame up from `lock_acquire`, landing on the caller. It identified that
caller by symbolising the recorded return address and asserting the name
`contains("lockdep")`:

```rust
lock_acquire(SITE_PROBE, b"site-probe", Acquire::Blocking);
…
if let Some((name, _)) = crate::ksyms::resolve_static(site as u64) {
    assert!(name.contains("lockdep"), "recorded site resolved to {name} …");
}
```

In a debug build the caller is `lockdep::self_test` and the substring matches.
In release, LLVM inlined `self_test` into `kernel_main`, so the recorded address
genuinely *is* inside `kernel_main` — the frame walk was correct and the
assertion fired anyway:

```
!!! KERNEL PANIC !!!
panicked at kernel\src\lockdep.rs:1647:13:
recorded site resolved to kernel_main, which is not the calling self-test
```

**The test was asserting a property of the build, not of the code under test.**
After inlining there is no `self_test` frame to walk to, because there is no
`self_test`. A test whose premise the optimiser is free to delete is a test that
only runs at `-O0`.

**How it stayed hidden — and this is the transferable part.** `scripts/boot-test.sh`
builds debug. `--bench` builds release, and `--bench` had not been run since
before this test was written. So the failing configuration was one flag away and
nothing in between exercised it. **A self-test that reasons about symbol names,
frame layout, or inlining is release-sensitive by construction, and the routine
gate does not cover it.** The other place this class lives is `caller_ips()`
itself, whose own `#[inline(never)]` doc comment says the attribute is
load-bearing — the same hazard, correctly handled one function away.

**The fix.** Move the probe's acquire into a dedicated `#[inline(never)] fn
site_probe_acquire(probe: usize)`, so a frame with a stable symbol exists in
every build, and compare against the name `resolve_static` gives for that
function's *own address* rather than a hardcoded substring:

```rust
let expected = crate::ksyms::resolve_static(site_probe_acquire as usize as u64).map(|(n, _)| n);
assert!(Some(name) == expected, …);
```

Asking the same table for both sides is deliberate: a literal would test the
symboliser's spelling and mangling as much as the frame walk, and neither is
this test's subject. The assertion is now stronger than the substring it
replaced — `contains("lockdep")` would also have accepted a walk that landed on
any other function in this file.

**Not a regression in lockdep.** `caller_ips` was right in both builds
throughout; only the test's way of naming its own caller was wrong.

*(That last paragraph is the error. It was written without re-running `--bench`,
because the change was "obviously" sufficient. It was not.)*

---

### Round 2 (correct): `lock_acquire` was `#[inline]`, so the walk lost a frame

Running `--bench` after round 1 panicked again, and the stronger assertion round
1 had installed is what made the second panic legible:

```
panicked at kernel\src\lockdep.rs:1680:13:
recorded site resolved to kernel_main, not to the function that took the lock
(Some("_ZN6kernel7lockdep18site_probe_acquire…")) — caller_ips() walked to the wrong frame
```

Both sides of that comparison now come from the symbol table, so this is not a
naming quibble: the walk genuinely landed on `kernel_main` when the function
that took the lock was `site_probe_acquire`, which still exists as a real
`#[inline(never)]` frame. Round 1's premise — "there is no `self_test` frame to
walk to, because there is no `self_test`" — had been repaired, and the walk was
*still* one frame high. That is the tell that the walker, not the target, was
wrong.

**The actual cause.** `caller_ips()` is `#[inline(never)]` and reads two frames:
its own, then its parent's — the parent being `lock_acquire`, whose return
address is the acquisition site. That is correct only while `lock_acquire` *has*
a frame. It was declared `#[inline]`. In a release build LLVM took the hint, so
the "parent frame" the walk found was not `lock_acquire`'s but its caller's, and
the return address in it belonged to the caller's **caller**. Every recorded
site was off by exactly one level of call.

`#[inline]` is a hint and `#[inline(never)]` is a guarantee, and the frame walk
was resting the whole of its correctness on the hint.

**This was never confined to the test.** The same run prints the held-lock dump
a few lines above the panic, and it shows the defect directly:

```
[lockdep]   cpu 0 holds 2 lock(s):
[lockdep]     [0] test-A @ 0xdead0001, taken at 0xffffffff812eec9b (kernel_main+0x16b)
[lockdep]     [1] test-B @ 0xdead0002, taken at 0xffffffff812eec9b (kernel_main+0x16b)
```

Two different locks, taken at two different places, reported at one identical
address — the call to `self_test` in `kernel_main`. In a release build every
lockdep report attributed every acquisition to whoever called the function that
took the lock. That is worse than printing nothing: a bare address carries an
implicit claim to be worth reading, and this one sends you to code that never
touched the lock.

**The fix, in two parts, because a frame can be lost two ways.**

1. `lock_acquire` is now `#[inline(never)]`. The invariant the walk depends on
   becomes a guarantee instead of a hope. It costs nothing measurable: there are
   two call sites, both already doing `preempt_disable` + `ensure_registered` +
   `tracking_enabled` on the same path, and `ENABLED` is true from boot onward,
   so the early-out that the inline hint existed to fold away is the branch not
   taken in production.
2. `site_probe_acquire` gained a `core::hint::black_box(probe)` after the
   acquire. `#[inline(never)]` on the *callee* does not stop the *caller's* own
   frame vanishing: a body consisting of one call in tail position can be
   emitted as `jmp` instead of `call`, reusing the frame rather than pushing
   one, which produces the identical off-by-one from the opposite direction.
   The barrier takes the call out of tail position, which is what makes the
   frame mandatory.

*(Round 2 is correct as far as it goes, and both of its fixes were verified by
disassembly to have worked. It is incomplete: it treats "a frame can be lost two
ways" as an enumeration, and there was a third way, in the walker itself.)*

---

### Round 3 (the fix): a function cannot assume its own prologue has run

Round 2's `--bench` run panicked again, one frame closer than before —
`kernel_main::case` instead of `kernel_main`. At that point the honest move was
to stop proposing mechanisms and read the machine code, which is what found the
third hazard immediately:

```
$ llvm-objdump -d --disassemble-symbols='…caller_ips…' target/x86_64-unknown-none/release/kernel
ffffffff81150310: movq %rbp, %rax      <-- the asm! that reads the frame pointer
ffffffff81150313: testq %rax, %rax     <-- null / alignment sanity checks
…
ffffffff81150322: pushq %rbp           <-- the prologue, AFTER the read
ffffffff81150323: movq %rsp, %rbp
```

LLVM had **shrink-wrapped** `caller_ips`: it sank the `push rbp; mov rsp,rbp`
prologue below the early-exit checks, because on the bail-out paths no frame is
needed. Perfectly legal, and invisible from the source. The `asm!("mov {}, rbp")`
at the top of the function therefore read the *caller's* `rbp`, so "my frame"
was really "my caller's frame" and the whole walk was shifted by one — again.

`-C force-frame-pointers=yes` does not prevent this. It guarantees the frame
pointer is *maintained*, not that it is established before the first instruction
that reads it.

**So there are three separate ways to lose exactly one frame**, and they are not
a list that can be closed:

| # | Liberty | What the walk sees |
|---|---|---|
| 1 | the callee is inlined into its caller | the "parent frame" belongs to the caller |
| 2 | a one-call body is emitted as `jmp` (sibling/tail call) | the frame is reused, not pushed |
| 3 | the prologue is sunk below the read (shrink wrapping) | `rbp` is still the caller's |

Rounds 1 and 2 each closed one of these with an attribute. No arrangement of
attributes closes the third, because it is not about who gets inlined into whom:
**a function cannot assume its own prologue has run.**

**The fix: `#[track_caller]`.** The question the walk was trying to answer —
"which line of code called me?" — is one the compiler already knows and answers
exactly, at compile time, via `Location::caller()`. No optimisation can move it.
`lock_acquire` and `sync::Mutex::{lock, try_lock}` all carry the attribute, so
the location `lockdep` records is transitively the *user's* source line rather
than a line inside `sync.rs`.

What this deleted, and why the deletions are the point:

- `caller_ips()` — ~65 lines of RBP-chain walking, `asm!`, canonical-address and
  alignment checks, and three paragraphs of doc comment justifying attributes on
  neighbouring functions. Gone.
- `CLASS_SITE_UP` and the `via:` line in violation reports. That second array
  existed only because the immediate caller of `lock_acquire` is nearly always
  `Mutex::<T>::lock`, which names the guarded *type* and not the code that took
  the lock; a real inversion in boot batch 31 reported both sides as
  `kernel::sync::Mutex<T>::lock+0x6f`, differing only in the monomorphisation
  hash. `#[track_caller]` sees through that in one step, so the workaround has
  no remaining job.
- `site_probe_acquire`, the `#[inline(never)]` probe helper round 1 added, and
  its `black_box`. With no frame to find, no frame need be forced to exist.

Reports improved in the process: `file:line:col` instead of `symbol+0xoffset`,
and correct at every optimisation level rather than at `-O0` only.

**The one hazard that remains, and how it is gated.** `#[track_caller]` degrades
*silently*. Remove it from `lock_acquire` and every site becomes the
`Location::caller()` line inside `lockdep.rs`; remove it from `Mutex::lock` and
every site becomes the `lock_acquire` call inside `sync.rs`. Both are real code
locations and both look like plausible answers in a report — the second is
precisely the failure this whole mechanism exists to prevent. So test 9b now has
two cases, one per attribute, and each asserts the **exact line** it expects
(captured with `line!() + 1` immediately above the acquire) rather than merely
that something was recorded. An exact line is the only assertion both wrong
answers fail.

**The transferable lesson.** Round 1 explained the failure convincingly and
fixed a real weakness (the substring assertion *was* build-dependent), then drew
the wrong conclusion from it and did not re-run the one command that would have
said so. Round 2 found a genuine cause, fixed it correctly, and assumed it was
the only one. Both were plausible mechanisms that fully accounted for the
symptom — and a plausible mechanism that accounts for the symptom is not the
same as the cause. The thing that ended it was not a better hypothesis but
`llvm-objdump`: **after the second wrong guess, stop reasoning about the
compiler and read what it emitted.** And the real fix was not a third patch to
the walk but the recognition that the walk was answering a question the language
answers exactly — three rounds of increasingly careful frame arithmetic were
three rounds spent reimplementing `#[track_caller]` badly.

The saving grace throughout was that each round made the assertion *stronger*
rather than weaker. A fix that had loosened the test to make the panic go away
would have shipped the wrong attribution silently and permanently.

| | |
|---|---|
| The panic | `kernel/src/lockdep.rs`, test 9b in `self_test` |
| The real defect | `kernel/src/lockdep.rs::caller_ips` — recovering the caller by walking the frame chain at all |
| Round-1 fix (real weakness, wrong conclusion) | `site_probe_acquire` + symbol-table-vs-symbol-table assertion |
| Round-2 fix (necessary, insufficient) | `#[inline(never)]` on `lock_acquire`; `black_box` to defeat the sibling call |
| Round-3 fix (shipped) | `#[track_caller]` on `lock_acquire` and `sync::Mutex::{lock, try_lock}`; `CLASS_SITE` holds a `&'static Location`; `caller_ips`, `CLASS_SITE_UP` and `site_probe_acquire` deleted |
| Blast radius before the fix | every `taken at` address in every release-build lockdep report, not just the self-test |
| How to reproduce (before the fix) | `./scripts/boot-test.sh --bench` — release build, panics ~20s into the boot |
| The gate that missed it | `scripts/boot-test.sh` without `--bench` builds debug |
| The gate now | test 9b asserts the exact recorded `file:line` for both attributes, and is correct in debug and release alike |
| Design record | `design-decisions.md` §642 |
