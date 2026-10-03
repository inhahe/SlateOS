## 73. YSH port strategy — **defer YSH; obtain it by cross-compiling genuine Oils once a C++/slateos toolchain exists, NOT by hand-porting or auto-translating**

**Date:** 2026-07-19
**Decided by:** Operator (Claude recommended this option; operator agreed).

**Context.** §72 covers **OSH** (the bash-compatible half of Oils), reimplemented
in Rust as `userspace/oils` and now very mature (~26k lines, 480 passing tests,
byte-for-byte vs. bash across extensive probing). Oils is **two languages in one
binary**: OSH *and* **YSH** (formerly "Oil") — the genuinely new, typed shell
language (real `Int/Float/Str/List/Dict/Obj` values, an expression sublanguage,
`var/const/setvar`, `proc`/`func`, closures, J8/JSON, eggex, structured error
handling). YSH is **not built at all**. The operator asked whether the full YSH
language should also be ported, and by what mechanism.

**Key technical facts that drove the decision.** Oils' source of truth is a
statically-typed subset of **Python** ("mycpp"); the shipping `oils-for-unix`
binary is **machine-generated C++** (from that Python) riding Oils' own
garbage-collected runtime. There is **no realistic automated path** to turn
either form into good Rust: Python→Rust transpilers (`py2many`, etc.) are
toy-grade; `c2rust` is C-only (negligible C++ support) and, even if it worked,
would emit an unmaintainable unsafe blob modeling Oils' GC. Rust *refactoring*
libraries (`syn`/`quote`, rust-analyzer-as-lib, `comby`, `cargo fix`) only
rewrite Rust we already have — they do not port another language *in*.

**Decision.** Do **not** hand-port or auto-translate YSH into Rust. Instead:
1. **Now** — keep hardening the Rust **OSH** shell (§72); it is the high-value
   bash-superset and nearly complete.
2. **Later** — once a **C++/slateos cross-toolchain** exists (a prerequisite the
   Mesa/GPU, Chromium, and WINE initiatives all need anyway) plus enough
   SlateOS POSIX/libc surface, obtain YSH by **cross-compiling genuine upstream
   Oils C++** — which yields faithful **OSH *and* YSH at once**, no
   reimplementation. Track YSH as **blocked-on-C++-toolchain**, not
   blocked-on-effort.

**Deferred sub-decision (revisit when the toolchain lands).** Once real Oils can
cross-compile, choose between: (a) keep the lightweight Rust OSH as the default
shell and ship genuine Oils as an *installable package* for YSH users; or
(b) retire the Rust OSH in favor of upstream Oils entirely. Not settled now.

**Alternatives considered.**
- **Hand-reimplement YSH in Rust** (mirroring the OSH approach). Pro: runs on
  SlateOS today with no new toolchain; consistent with §72. Con: YSH is a whole
  second language (typed value system + expression parser + `proc`/`func` +
  eggex + J8 + YSH builtins) — on the order of the entire OSH effort again — and
  it would perpetually chase upstream YSH, which is still evolving. Rejected as
  the *primary* plan: once a C++ toolchain exists anyway, a faithful cross-compile
  gets both languages for far less work and with exact semantics. (Left available
  as a fallback if the C++ toolchain never materializes and YSH becomes urgent.)
- **Automated source translation** (Python→Rust or generated-C++→Rust). Rejected:
  no production-grade tooling exists; the GC-runtime-generated C++ is
  especially hostile to `c2rust`. This corrects an earlier assumption that the
  C++ toolchain would unlock an *automated* YSH port — it unlocks a faithful
  *cross-compile*, not a translation.

**How to reverse.** Symmetric with §72: the strategy is a sequencing/prerequisite
call, not a code commitment. If YSH becomes urgent before the C++ toolchain
lands, fall back to a Rust reimplementation; the `userspace/oils` crate is
isolated so either a YSH-in-Rust module or a swap to genuine Oils is a local
change.

### ANNOTATION 2026-09-09 (lane B) — the prerequisite has fired. Not flipped: this is an Operator decision.

**The C++/slateos cross-toolchain this entry waits on now demonstrably
exists.** Measured today, not inferred:

| Step | Result |
|---|---|
| `zig c++ --target=x86_64-linux-musl -std=c++17 -c` on a TU using `<string>`, `<vector>`, `<memory>` and a `throw`/`catch` | compiles, 245 KB object |
| the same, with the slateos codegen flags (`-mcmodel=large -fno-pic -fno-pie -fno-builtin -O2`) | compiles, 185 KB object |
| linking that object against **our** `toolchain/sysroot/lib/libc.a` with `rust-lld` | 8 undefined symbols, **all** of them libc++/C++-ABI (`operator new`, `std::__1::basic_string::append`, `typeinfo for std::length_error`, …) and **zero** of them libc |
| `zig c++ --target=x86_64-linux-musl -static` end to end | builds libc++ from source and links a 3.4 MB `ET_EXEC` x86-64 binary |

The reading: the compiler exists, the C++ standard library exists, and our libc
already satisfies everything a C++ program asks of C. What is missing is the
*link line* — combining zig's `libc++` with our `libc.a` — which is wiring, not
a toolchain.

**That wiring was then measured, and then done**, both on 2026-09-09. A C++
translation unit using `<string>`, `<vector>` and a real `throw`/`catch` links
for `x86_64-slateos` against our own `libc.a` with **zero undefined symbols and
zero duplicates** — a 3.6 MB static `ET_EXEC`. The remaining obstacles were a
missing `swprintf`/`wcstold` (implemented) and what looked like an
ABI-ownership decision but turned out to be a packaging defect: our C++ ABI
stubs shared an object file with `__libc_start_main`, so every program dragged
them in and they collided with any real runtime. They now have their own
archive member. See `known-issues.md` →
`B-THE-C-PLUS-PLUS-LINK-LINE-NEEDS-TWO-DECISIONS-AND-ONE-MISSING-FAMILY`.

**Still not established: nothing C++ has been *run* on SlateOS.** This is the
link stage, exactly where the CPython and bash spikes each stood before their
ring-3 rungs, and it carries the same caveat those did — linking proves the
symbol surface, not the behaviour.

**What this does NOT establish**, stated because the gap matters: nobody has
cross-compiled genuine Oils, and nobody has run a C++ binary on SlateOS. This
says the **prerequisite** named here has fired, so YSH moves from
"blocked-on-C++-toolchain" to "blocked-on-effort" — which is exactly the
distinction this entry drew, and the only thing it made conditional.

**Why this was worth going and looking for.** §305's root cause, recorded in
`todo.txt` as a standing rule: §72 rejected cross-compiling bash because no
C-to-slateos toolchain existed, the clause fired four days later when `zig cc`
landed, nobody checked for 25 days, and ~1,100 commits went onto a dead
premise. `zig cc` and `zig c++` are *the same binary*. The C half was noticed
in July; the C++ half sat unnoticed in the same executable for seven weeks,
and `todo.txt` recorded it as "the C++ half has NOT [fired]" — a statement
nobody had tested. This is that failure caught by the rule that was written
after it.

**The deferred sub-decision above is now live** and is the operator's:
`open-questions.md` → **B-Q9**.
