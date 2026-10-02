## 1102. A program whose ELF headers are not mapped runs with no TLS image, rather than aborting

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous) — lane A's request left the choice to lane D
("your call whether a null header means 'no TLS image' or a loud abort").

**In short:** the C library finds a program's thread-local variables by reading
the program's own ELF header in memory. Some linker scripts leave that header
out of memory, and every program linked that way crashed on its first
instructions with an unexplained fault. It now carries on as if the program had
no thread-local variables — correct for every such program today — rather than
stopping with an error message.

**Background.** `posix::tls::image` reads the program headers through
`__ehdr_start`. When no loaded segment contains the ELF header, lld resolves
that symbol to 0, and the read faulted at address 0x36
(`requests/a-bd-coreutils-cannot-start-two-link-faults.md`, fault 1 —
`coreutils`, `oils` and `shell`). Native processes get no auxiliary vector, so
there is no second source for the headers.

| Option | For | Against |
|---|---|---|
| **No TLS image** (chosen) | what glibc (`_dl_aux_init`, weak `__ehdr_start`) and musl (no `AT_PHDR` → its `PT_TLS` walk runs zero times) both do; right for every program without `__thread`, which is every program linked this way today | a program with unmapped headers *and* C `__thread` starts its thread-locals at zero, silently |
| Abort, naming the link | a wrong link can never run wrong | every such program stops at startup, though almost none has anything to lose; turns a latent script flaw into a hard outage |

**Mitigation.** The compiler is kept from folding the null check away (the
address goes through an empty `asm!`), the residual is
`known-issues.md` → `TD-D-TLS-NEEDS-MAPPED-PROGRAM-HEADERS`, and its proper fix
— the kernel passing `AT_PHDR`/`AT_PHNUM` to native processes, as it already
does to Linux ones — makes the question moot.
