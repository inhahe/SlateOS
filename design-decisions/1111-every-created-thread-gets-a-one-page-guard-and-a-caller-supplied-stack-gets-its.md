## 1111. Every created thread gets a one-page guard, and a caller-supplied stack gets its TLS in a mapping of its own

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a thread that uses more stack than it has now crashes at once
instead of quietly overwriting other memory, because `pthread_create` puts an
inaccessible page below every stack it makes (`known-issues.md` →
`B-D-PTHREAD-CREATE-IGNORED-ITS-ATTRIBUTE`). The page costs memory. And when
a program supplies its own stack, the thread's private library data goes in a
small separate mapping rather than being carved out of the program's stack.

| Choice | For | Against |
|---|---|---|
| **One-page guard by default (chosen)** | what glibc and musl do, and what `pthread_attr_init` reports; an overflow faults at the guard | one 16 KiB page per thread, and memory here is committed, not lazily allocated |
| No guard (the old behaviour) | 16 KiB less per thread | an overflow corrupts whatever is mapped below, silently |
| **TLS for a caller's stack in its own mapping (chosen)** | the caller's memory is used only as the thread's stack, all of it | one more small mapping per such thread |
| Carve the TLS from the top of the caller's stack (musl, when it fits in an eighth of it) | no extra mapping | takes part of the stack the caller sized, and needs musl's "does it fit" rule and a fallback anyway |

The default stack stays 64 KiB (musl's is 128 KiB, glibc's usually 8 MiB):
what changed is that an explicit size is honoured. Whether the default
should grow is a separate question, weighed against committed memory.
