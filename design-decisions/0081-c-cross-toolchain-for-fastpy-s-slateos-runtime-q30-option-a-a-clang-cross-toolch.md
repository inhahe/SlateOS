## 81. C cross-toolchain for fastpy's SlateOS runtime (Q30) — **option A (a clang cross-toolchain to musl), realized via `zig cc`**

**Date:** 2026-07-21
**Decided by:** Operator (operator said "do A"; Claude proposed and implemented
`zig cc` as the concrete mechanism for A) — resolves open-question **Q30**,
delivering the runtime-port + full-program-link increments of initiative F.

**Context.** With §80's pure-mode-first strategy chosen and the codegen +
link halves already built and tested, the one remaining prerequisite for a
*runnable* SlateOS binary was compiling fastpy's C runtime (`runtime/*.c`) to
`x86_64-slateos` (musl) ELF objects. That needs a C compiler emitting
`x86_64-unknown-linux-musl` ELF **and** musl C headers to compile against. The
dev host had neither: no `clang` on PATH, MSVC `cl.exe` emits COFF/MSVC-ABI, and
the OS sysroot (`toolchain/sysroot/lib`) ships only Rust-built archives with no C
headers. Q30 offered: **A** install a clang cross-toolchain + vendor musl
headers; **B** reimplement the runtime as fastpy-generated IR; **C** wait for the
CPython bridge. Claude recommended A.

**Decision.** **A**, realized with **`zig cc --target=x86_64-linux-musl`**. `zig
cc` is a self-contained clang plus bundled musl headers and musl libc in one
portable download (zig 0.16.0 unpacked at `D:\utils\zig-x86_64-windows-0.16.0\`,
~97 MB, no installer, not system-wide). This *is* option A (a clang cross-
toolchain to musl) but its packaging sidesteps **both** of A's listed cons: no
heavyweight system-wide LLVM install, and no separately vendored/maintained musl
header set (zig ships a consistent musl). rust-lld (already used for the link
half, same LLVM family) links the result.

**Rationale.** *Pro:* directly unblocks the runtime port with the proper,
robust musl cross-compile path; portable and reproducible; avoids the large,
error-prone rework of option B (reimplementing GC/bigint/threading as generated
IR) and the stranding of option C. *Con:* introduces a zig dependency for the
SlateOS runtime build (mitigated: located via `$FASTPY_ZIG`/PATH/portable-dir
fallback, and only needed for the SlateOS target — the host build is unchanged);
zig's bundled musl must stay ABI-compatible with the OS `posix` crate's libc
shape (it does — both are standard x86-64 System V / musl-shaped, and a
successful static non-PIE link proves symbol resolution).

**Operator's exact words.** "okay, do A."

**Outcome (implemented same day).** fastpy `compiler/toolchain.py` gained
`_find_zig_cc()`, `_find_slateos_sysroot_lib()`, `_compile_shared_runtime_slateos()`,
and `ensure_slateos_runtime_built()`; `link_executable(target=SLATEOS_TARGET)` now
builds the six pure-mode TUs (`runtime`, `objects`, `threading`, `gc`, `bigint`,
and a new **`bridge_stub.c`** substituting for the CPython bridge) and links
program + runtime + sysroot `libc.a` via rust-lld. Pure mode is selected with
`-DFPY_PURE_MODE` (compiles out the JIT symbol table). A real fastpy program
(lists/iteration/print) links to a **~2.9 MB SlateOS ET_EXEC ELF with zero
undefined symbols**. Tests in fastpy `tests/test_cross_target.py` (skip without
zig/rust-lld/sysroot). **Crt startup wired (same day):** the link entry is
`_start` (the real ELF entry), so rust-lld pulls the crt0 from `libc.a` — the
`posix` crate is built for `x86_64-unknown-none`, so its `#[cfg(target_os =
"none")]` crt (`_start` → `__libc_start_main` → retrieve argv/envp via kernel
syscall, init environ/signals, run ELF constructors → `main` → `exit`) is
present. The ELF's entry is nonzero `_start`, so startup/args/clean-exit are all
in place. Remaining: compile one real OS component and boot it under the kernel
to confirm on-target behavior — the "first real component" milestone.

**Where it lives.** fastpy `compiler/toolchain.py`, `runtime/bridge_stub.c` (new),
`runtime/{runtime,objects,objects.h,threading.h}.c/.h` (pure-mode guard + latent
MSVC-ism fixes for clang), `tests/test_cross_target.py`; OS `toolchain/sysroot/lib`
(`libc.a` linked against). Supersedes Q30 in open-questions.md (now removed).
