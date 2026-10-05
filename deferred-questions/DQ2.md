## DQ2 (was D-Q2) — Install `clang` + `lld` and turn on LLVM CFI for C code?

*(Was `open-questions.md` A-Q1's **option B**, raised 2026-08-14 by Lane A.
Answered by the operator 2026-08-15 with **"not yet"** — a deferral, not a
refusal — and moved here the same day per `design-decisions.md` §313.)*

**Trigger:** the first substantial C port enters the build — ext4, Mesa, or
anything else that makes CFI govern a meaningful amount of compilation. Promote
this entry back into `open-questions.md` **at the start of that port, not
after**: retrofitting CFI onto a landed port means re-linking and re-testing it,
whereas building it that way costs one decision. Today the only C we compile is
`scripts/create-ext4-rootfs.sh`, with gcc.

**In short:** `clang` and `lld` are an alternative C compiler and linker. We do
not have them installed. Having them would let us switch on **CFI**
(Control-Flow Integrity — a compiler feature that stops an attacker redirecting
a function call to code of their choosing). The install is small and standard;
the question is whether it is worth changing how we build C at all right now.

**Why the answer was "not yet".** The payoff is currently near zero. Our rule is
that C is used *only* for porting existing C code, and nothing substantial is
ported yet — so "CFI as the default for C/C++" would today govern one shell
script's worth of compilation. Two costs against that:

- The one piece of C we build (`scripts/create-ext4-rootfs.sh`) uses **gcc**,
  and that script is **Lane B's tree**. Making CFI a default would reach across
  a lane boundary to change another lane's build for no present benefit.
- CFI wants **LTO** (whole-program optimization at link time), which changes
  build times and link behaviour for everything it touches.

**Nothing is blocked.** No roadmap item other than "Enable LLVM CFI as default
for C/C++ compilation" depends on it, and that item is the question itself. The
current behaviour — gcc, no CFI — is safe and stays.

**What will need deciding when it comes back** (not just install/don't):

- Whether CFI is the *default* for all C, or opt-in per ported component.
- Whether the LTO requirement is acceptable for the ported tree's build times.
- Whether `scripts/create-ext4-rootfs.sh` moves to clang too, or stays on gcc
  and is exempted — a Lane B decision that needs a `requests/` entry either way.

**Where it bites:** `.cargo/config.toml` (C flags), `scripts/create-ext4-rootfs.sh`
(Lane B), and `roadmap.md`'s Lane A backlog item "Enable LLVM CFI as default for
C/C++ compilation". Related: `design-decisions.md` §313 (this file's rules) and
`open-questions.md` → A-Q1, whose option **A** (GNAT/SPARK, answered *install
it, with the prover*) was recorded by Lane A as `design-decisions.md` §201.

---

### Amendment, 2026-08-16 (Lane A): two premises above are now false, and the feasibility is no longer a guess

**The answer does not change** — there is still no substantial C port, so the
payoff is still near zero and "not yet" still stands. What changes is the
**cost** side, which the entry above overstates in two specific ways. Recording
this now, because whoever promotes DQ2 will otherwise re-derive it.

**1. "We do not have them installed" is no longer true — and never needed to be.**
`zig cc` **is** clang, and `zig`'s linker **is** `ld.lld`. Zig has been a
required build dependency since the C fixtures landed
(`toolchain._find_zig_cc()`, called by every `services/ctest-*/build.py`), and
`toolchain._link_slateos` already links through `rust-lld`. Measured here:
clang **21**. So the "small and standard install" is an install of **nothing**.

**2. "The only C we compile is `scripts/create-ext4-rootfs.sh`, with gcc" is
stale.** Nine C fixtures are compiled by `zig cc` today — `ctest-ctty`,
`ctest-fortify`, `ctest-jobctl`, `ctest-libc-float`, `ctest-libm`,
`ctest-longdouble`, `ctest-pgroup`, `ctest-scanf` and `ctest-tls-thread` —
each `services/ctest-*/build.py` carrying its own hand-written copy of the same
flag list. They are already clang, already lld, already ring-3 SlateOS
binaries. So the cross-lane objection is narrower than stated: it is not
"install a compiler in Lane B's tree", it is "add flags to nine files that
already invoke clang".

**Those nine copies have already drifted**, which is a separate and smaller
problem worth fixing whatever happens to CFI. All nine share `-c -O2
-mcmodel=large -fno-pic -fno-pie -Wall -Wextra -Werror`, but only **seven**
carry `-fno-builtin` — `ctest-libc-float` and `ctest-tls-thread` omit it, with
no comment saying why. That flag's entire job is to stop clang constant-folding
the libc call the fixture exists to exercise, and `ctest-libc-float` is
precisely a test of `double` returns and varargs *through the sysroot*, so its
omission looks like drift rather than intent. (`ctest-tls-thread`'s extra
`-fstack-protector-all` is **not** drift — its docstring explains it forces a
`%fs:0x28` canary read into every function, which is the thing under test.) A
shared C-flags helper is the fix; it is Lane B's tree, and it does not need CFI
to be worth doing.

**3. Feasibility is measured, not assumed.** The working flag set, established
end-to-end against our own target:

```
-flto -fvisibility=hidden -fsanitize=cfi-icall
-fsanitize-trap=cfi-icall -fno-sanitize-ignorelist
```

with these findings attached:

- **`-fsanitize=cfi-icall` is the only CFI scheme that applies to C at all.**
  The others (`cfi-vcall`, `cfi-derived-cast`, …) are C++ vtable checks.
- **`-flto` is not optional**: `-fsanitize=cfi-icall` without it is rejected
  outright — *"invalid argument '-fsanitize=cfi-icall' only allowed with
  '-flto'"*. So the LTO cost noted above is real and unavoidable, not a
  preference.
- **`-fno-sanitize-ignorelist` is required under `zig cc`**, which otherwise
  fails with *"missing sanitizer ignorelist:
  'D:/utils/lib/clang/21/share/cfi_ignorelist.txt'"* — a file zig does not
  ship. Supplying our own via `-fsanitize-ignorelist=<file>` does **not** work;
  clang still looks for the default as well.
- **Do not link zig's prebuilt musl under LTO**: `ld.lld: error: inconsistent
  LTO Unit splitting (recompile with -fsplit-lto-unit)`. This does not affect
  us — our fixtures link `-nostdlib` against our own Rust `libc.a` through
  `toolchain._link_slateos` — but it will bite anyone who tests CFI with a
  stock hosted link and concludes it is broken.
- **`zig cc -c -flto` emits LLVM bitcode, not an ELF object.** The `.o` starts
  `BC C0 DE`; `rust-lld` runs the LTO codegen at link time. It works — but any
  tool that inspects the `.o` between compile and link (a size check, an
  objdump, a symbol scan) sees bitcode and must be taught to expect it.

**This was verified end-to-end against our real toolchain, not a toy link.** A
fixture was compiled with `zig cc --target=x86_64-slateos` using the exact flag
list `services/ctest-*/build.py` uses, then linked by `toolchain._link_slateos`
(rust-lld, `-nostdlib`, our own `libc.a`), once with CFI and once without. The
CFI build contains **two `ud1` reason-2 traps — one per indirect call site —
and the non-CFI build contains none.** So the scheme survives `-mcmodel=large`,
`relocation-model=static`, our linker and our sysroot.

**Three ways that verification silently produced a false negative first.** All
three are the project's recurring shape, and any future attempt will hit them
again in the same order:

1. **Scanning `.text` finds nothing, because `-mcmodel=large` puts our code in
   `.ltext`.** The first scans reported "0 traps" while reading only libc's
   `.text` and never examining the fixture at all. A section-name scan must
   accept `.ltext`.
2. **A fixture whose indirect call clang can devirtualise proves nothing.**
   `f = cond ? a : b; f(x)` is turned back into two *direct* calls by
   indirect-call promotion at `-O2`, so there is no indirect call left to
   check and the CFI build is byte-identical in the ways that matter. The
   pointer must be `volatile` (or otherwise opaque) for the check to exist.
3. **`ctest-fortify/main.c` makes no indirect calls whatsoever**, so linking
   *it* with CFI succeeds and instruments zero call sites. "It linked" is not
   "it is protected".

The invariant for anyone repeating this: **the test is that the CFI build has
traps the non-CFI build lacks** — never that the CFI build merely compiles,
links, or contains some absolute number.

**4. Verifying that CFI is actually on: scan for `ud1`, never `ud2`.** This is
the trap worth writing down. The emitted code is:

```
CFI off:  mov rax,[rip+..] ; call rax                    (unchecked)
CFI on:   mov rax,[rip+..]
          48 3d b0 12 00 01   cmp rax, <jump-table entry>
          75 28               jne  -> trap
          ff d0               call rax
   trap:  67 0f b9 40 02      ud1        <-- NOT ud2
```

Clang's `-fsanitize-trap` emits **`ud1` (`0F B9`)**, not `ud2` (`0F 0B`), with
the failing check's ordinal in the ModRM displacement (`02` = `cfi_check_fail`).
The obvious verification — grep the binary for `ud2` — therefore reports "no
traps found" on a **fully instrumented** binary. That is this project's
recurring failure shape: *a check that cannot fire is indistinguishable from a
check that passes.* It cost a false negative here before being caught.

**5. The kernel side is already done, and did not wait for this decision.**
`kernel/src/idt.rs` now decodes both `ud2` and clang's `ud1` on the ring-0
*and* ring-3 paths and names the failing sanitizer (`decode_ud_trap`,
`sanitizer_trap_name`, `ud_trap_decode_self_test`). Before that, a ring-3 #UD
was reported with no cause at all and a ring-0 one decoded only `ud2` — so a
CFI violation, the exact event CFI exists to produce, would have surfaced as an
anonymous bad opcode. That work stands on its own merits (it also names
out-of-bounds, shift, divide-by-zero and sixteen other trap kinds) and implies
nothing about whether CFI gets switched on.

**Net effect on the decision:** the install cost is zero, the cross-lane cost is
nine flag lists that ought to be one, the LTO cost is confirmed mandatory, and
the diagnosis path is already built. The trigger is unchanged — **first
substantial C port** — and until then the payoff is still the thing that is
missing.
