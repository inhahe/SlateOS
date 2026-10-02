### [D] TD-D-TLS-NEEDS-MAPPED-PROGRAM-HEADERS — 2026-09-25 — OPEN

**Status:** OPEN — the narrow residual of the fix for
`requests/a-bd-coreutils-cannot-start-two-link-faults.md` fault 1; the proper
fix is a kernel ABI addition (lane A), requested at low priority in
`requests/d-a-native-processes-could-be-told-where-their-program-headers-are.md`.

**In short:** the C library finds a program's thread-local variables through the
program's own ELF header in memory. A linker script that leaves the header out
of the loaded image used to crash every program linked with it on its first
instructions; since 2026-09-25 the library treats "no header" as "no
thread-local variables", as glibc and musl do. That is right for every such
program today. A program that had both an unmapped header *and* C `__thread`
variables would start those variables at zero instead of their initial values,
silently.

**Where:** `posix/src/tls.rs` → `image()` (design-decisions §1102).

**Proper fix:** give native processes the program headers' address the way
Linux does — `AT_PHDR`/`AT_PHNUM` in an auxiliary vector (the kernel already
builds one for Linux-ABI processes, `kernel/src/proc/linux_stack.rs`), or the
headers copied onto the new stack when no segment maps them — and read that
here before `__ehdr_start`. Until then, every native linker script must map
`FILEHDR PHDRS` into its first `PT_LOAD`; lld's default layout does.
