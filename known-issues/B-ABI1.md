### B-ABI1. A *bare* static Linux ELF (no OSABI/PT_INTERP/PT_GNU_PROPERTY) is misclassified as Native-ABI on `exec` — KNOWN LIMITATION (escalated as open-questions.md Q9)

**Symptom:** A Linux binary with none of the markers `elf::detect_linux_abi`
keys off — e.g. the output of `tcc -nostdlib -static`, or a hand-rolled static
musl/asm program (OSABI=`SYSV`/0, no `PT_INTERP`, no `PT_GNU_PROPERTY`; the only
GNU-ish artifact is a `PT_GNU_RELRO` segment, deliberately rejected as a signal)
— is classified as a **Native-ABI** process. Its raw `syscall`s are then routed
through the native dispatch table instead of `kernel::syscall::linux`, so e.g.
`write(1, …)` produces 0 observable bytes and `exit(n)` loses its status. This
bites the **`exec` path** (a shell or `make` exec'ing a freshly-built bare tool),
which re-detects the ABI from the ELF with no way for the caller to override.

**Root cause:** A bare SYSV static ELF carrying only generic GNU-toolchain
artifacts is genuinely ambiguous between "Linux binary" and "SlateOS-native
binary built with a GNU/LLVM toolchain." No automatic heuristic separates them
reliably; disambiguation needs an explicit marker on one side.

**Worked around (spawn only):** `spawn::spawn_process_with_abi(elf, options,
AbiMode::Linux)` lets an in-kernel caller that *knows* the binary's ABI state it
explicitly (used by `self_test_linux_real_glibc_cc`, which just compiled the
binary as a Linux program). This does **not** cover the general `exec` path.

**Proper fix (deferred — needs operator decision):** open-questions.md **Q9** —
recommendation is to default unmarked bare ELFs to the Linux ABI and stamp
SlateOS-native binaries with an explicit OSABI value / `.note.slateos`, plus add
`NT_GNU_ABI_TAG` note-walking as a positive Linux signal. Where it bites:
`kernel/src/proc/elf.rs::detect_linux_abi`, `spawn.rs::spawn_process_inner`, and
the `exec` `new_abi_mode` path.
