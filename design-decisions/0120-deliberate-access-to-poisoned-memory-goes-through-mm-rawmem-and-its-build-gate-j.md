## §120 — Deliberate access to poisoned memory goes through `mm::rawmem`, and its build gate judges accessors rather than reachability

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). The third and last of the
§107 escalation's "make the instrumented kernel actually boot" decisions;
mine to revisit.

**Context — the same hazard from a third root.** §118 and §119 recorded that a
module-level `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]`
cannot exempt a generic `core` function, because the monomorphisation lands in
*this* crate carrying the default (instrumented) attribute. Both were found in
places where instrumentation is fatal for structural reasons: before the shadow
exists (triple fault), and underneath the check itself (unbounded recursion).

The third place is different in kind and was found the expensive way. A whole
class of our code exists to read and write bytes that KASAN has *deliberately*
marked inaccessible — `mm::heap`'s free-magic and redzone checks, `mm::poison`'s
fills and verifies, `mm::quarantine`'s parked slots. For them the access **is**
the detector, so a report on it is not a finding. Every one of those modules
carried the opt-out, and every one of them did its actual byte touching through
`core::ptr::{read_volatile, write_volatile, write_bytes}` — so the exemption was
cosmetic on precisely the operations it existed for. The first instrumented boot
to reach the self-tests died of it after ~2.7 hours, with
`report <- __asan_load1_noabort <- core::ptr::read_volatile::<u8> <-
mm::heap::check_redzone` (`known-issues.md` →
`B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS`).

**Decision 1 — one module of `asm!` accessors, not hand-rolled loops in each
exempt module.** `mm::rawmem` provides `read_u8`, `write_u8` and `fill_u8`
(`rep stosb`), all inline assembly, and every deliberate poisoned-memory touch
in `heap.rs`/`poison.rs`/`quarantine.rs` routes through them.

*For:* LLVM's AddressSanitizer pass does not instrument the memory operands of
inline asm, so the property holds regardless of whether the helper is inlined,
whose function body the access ends up in, or whether that caller is exempt —
it is enforced by the mechanism rather than by everyone remembering a rule.
*Against:* it is more code than `*ptr`, and it moves the safety argument out of
the type system into `// SAFETY:` comments. The considered alternative was to
hand-roll byte loops inside the exempt modules so no `core` generic is called.
Rejected: it works today and breaks silently the first time someone reaches for
a `core::ptr` helper inside an "exempt" module, which is a natural thing to do
and produces no warning. Volatility is preserved by omitting `nomem`/`readonly`,
since the call sites depended on it (a poison fill must not be dead-store
eliminated, a verify read must not be constant-folded).

**Decision 2 — the accessors get a boot self-test, not a `#[cfg(test)]` one.**
`rawmem::self_test` runs from `kernel_main` *before* the poison, KASAN and
quarantine self-tests.

*For:* the kernel binary cannot be built for the host harness at all
(`cargo test -p kernel --bin kernel` fails with a duplicate `panic_impl` lang
item), so this matches the existing `mm::poison::self_test` convention. Ordering
is the substantive part: these three helpers are the foundation the poison,
redzone and quarantine checks all stand on, so if a hand-written `asm!` store
had the wrong operand size or direction, every downstream "OK" would be
meaningless. *Against:* it costs boot time in every build, instrumented or not.
Accepted — it is three buffers and a `rep stosb`.

**Decision 3 — the gate's third walk judges *which* function is instrumented,
not whether it is reachable.** `scripts/kasan-check-preshadow.py` grew a third
root set, but deliberately **not** the violation rule the other two use.

Walks 1 and 2 flag every instrumented function they can reach, and that is
right for them: before the shadow exists, or underneath the check, an
instrumented access is fatal no matter what memory it touches. Applying the
same rule here produced 500-odd "violations" on its first real run, every one
of them an `AtomicUsize::fetch_add` on a stats counter, a `Range::next` on a
loop variable, or a `SpinMutexGuard` deref on the serial port — ordinary live
kernel objects, where instrumentation is correct and even desirable, since it
is how a bug in the memory debugger itself would surface.

So `walk_poison` judges exactly two things: a root's own body (an `__asan_*`
call there means the module's opt-out is not in force) and the raw-pointer
accessors it reaches, which is the §118/§119 hazard itself. 500 hits became 2,
and both of those turned out to be real information about the walk rather than
about the kernel (see below).

Getting the accessor list right needed one non-obvious addition. `core::ptr` and
`core::intrinsics` look like they cover the ground, but `core::intrinsics` emits
only `rotate_left`/`rotate_right` in this binary — pure register operations —
because `write_bytes` and `copy_nonoverlapping` are lowered by LLVM straight to
`memset`/`memcpy` calls rather than to callable monomorphisations. A bulk fill
aimed at poisoned memory therefore appears in the call graph as a plain
`call memset`, which is the single most likely way to write this bug and which
both patterns would have missed. The three `mem*` builtins are matched by exact
name.

One consequence is worth stating rather than leaving to be rediscovered: after
the `rawmem` conversion, *zero* accessors of any kind are reachable from the
poison roots, because the byte touching is all inline `asm!`, which emits no
call. The accessor branch therefore judges an empty set today and passes because
there is nothing to judge — the intended state, but indistinguishable by exit
code from a check that has quietly stopped working. The gate's OK line reports
the accessor count next to the reachable count so the two cannot be confused.
The real work every run is done by the root-body branch (all thirteen roots
verified uninstrumented); the accessor branch is a tripwire for reintroduction.

*For:* a gate that reports 500 non-problems does not get 500 exemptions, it gets
rubber-stamped as noise, and the one real signal goes with it. Precision here is
not tidiness, it is whether the check survives contact with a reader.
*Against:* it can miss a poisoned-memory access made some way other than through
a raw-pointer accessor, and it can over-report a `read_volatile` aimed at MMIO.
The over-report direction is cheap (a `rawmem` call or a documented exception);
the under-report direction is bounded by the fact that a plain `*ptr` in an
exempt module *is* covered by the module attribute — the generics are the entire
residual gap, and they are what is checked. `core::slice` is deliberately out of
scope: slice helpers are used on ordinary memory throughout these modules, so
including them would reintroduce the false positives above, and poisoned memory
is reached here through raw pointers regardless.

**Decision 4 — a cut that matches nothing is a hard failure.** The walks stop
at cold reporting machinery (the serial printer, the panic path, the KASAN
reporter), which is reached only *after* a violation has been found and is far
too much code to keep clean. Those cut lists are now validated against the
binary, exactly as the root lists already were.

*For:* this is not hypothetical. `'6serial6_print'` was one underscore short of
the real symbol — v0 escapes the leading `_` of `_print`, encoding it as
`6__print` — so the cut matched nothing, and a cut that matches nothing does not
stop the walk. It ran on through the printer into the APIC MMIO accessors and
reported `read_volatile` there. Both of the two surviving "violations" above had
that single cause. A dead root fails safe (the walk covers less than you think
and says so); a dead cut fails *unsafe*, by burying the real signal in noise
from code the check never meant to cover. *Against:* it couples the script even
harder to symbol mangling. Accepted for the same reason §118 accepted it: a
stale list becomes a hard failure instead of a silent pass.

**Where it lives.** `kernel/src/mm/rawmem.rs` (new), the converted call sites in
`kernel/src/mm/{heap,poison,quarantine}.rs`, `kernel/src/main.rs` (the self-test
call, ahead of `mm::poison::self_test`), `scripts/kasan-check-preshadow.py`
(`POISON_ROOT_SUBSTRINGS`, `POISON_ACCESSOR_SUBSTRINGS`, `walk_poison`).

**How to reverse.** Inert in the ordinary build: the `asm!` accessors compile to
the same loads and stores a `read_volatile` would, and the gate exits 2 ("not an
instrumented build") when handed a kernel with no `__asan` symbols.
