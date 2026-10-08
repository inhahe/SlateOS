### [D] TD-D-TLS-NEEDS-MAPPED-PROGRAM-HEADERS — 2026-09-25 — OPEN (both halves written; closes when lane A's kernel is on `main`)

**Status:** OPEN -- both halves are written: lane A's `SYS_PROCESS_GET_PHDR`
(1102, on `lane-a`), which says where the table is and maps a read-only copy
when no segment holds it, and lane D's use of it (2026-10-07,
`posix/src/tls.rs`, `program_headers`: the kernel's answer first, kept, then
`__ehdr_start`). It closes when lane A's kernel is on `main`: every native
program asks it at start-up for its TLS template, so a green boot on that
kernel is its test. The residual below then holds only on an older kernel.

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
