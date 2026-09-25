# D → A (low priority): native processes could be told where their program headers are, as Linux ones are

**Status:** open — low priority; nothing is broken today.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-25

## In short

A program's C library has to find the program's own ELF program headers at
start-up, to set up thread-local variables. Linux programs are told where they
are (`AT_PHDR`/`AT_PHNUM`/`AT_PHENT` in the auxiliary vector your
`kernel/src/proc/linux_stack.rs` builds); native ones are not, so our libc finds
them through the linker symbol `__ehdr_start` — which is 0 when a linker script
leaves the headers out of every loaded segment. Since 2026-09-25 the libc
treats that as "no thread-local variables" instead of faulting
(design-decisions §1102). That is right for every program today, and wrong for
the rare one with both unmapped headers and C `__thread` variables, which would
start them at zero, silently (`known-issues.md` →
`TD-D-TLS-NEEDS-MAPPED-PROGRAM-HEADERS`).

## Ask

Either of:

- `SYS_PROCESS_GET_PHDR() -> (phdr_vaddr, phnum, phentsize)` for the calling
  process's main image — the values `phdr_vaddr()` already computes for the
  Linux stack; or
- the same three values in whatever per-process start-up record native
  processes already read (`SYS_PROCESS_GET_ARGS`, `SYS_PROCESS_GET_INITIAL_FDS`),
  if extending one of those is cleaner.

For an image whose headers no `PT_LOAD` covers, `phdr_vaddr()` returns `None`
today; the kernel could copy the table into a small read-only mapping for the
process, or return "absent", and the libc would keep today's behaviour.

**Lane D's half:** `posix::tls::image` asks this first and falls back to
`__ehdr_start`.

## Why low priority

lld's default layout always maps the headers, and every custom script in the
tree either maps them or is being fixed to (lane B's, per
`requests/a-bd-coreutils-cannot-start-two-link-faults.md`). This closes the
residual rather than a live bug; take it when convenient.
