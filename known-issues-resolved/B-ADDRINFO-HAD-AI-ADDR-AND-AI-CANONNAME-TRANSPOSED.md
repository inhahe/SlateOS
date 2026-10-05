## B-ADDRINFO-HAD-AI-ADDR-AND-AI-CANONNAME-TRANSPOSED (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure `getaddrinfo` returns had two of its pointers the
wrong way round. A program that resolved a hostname and then connected to it
passed the *name text* to `connect()` where the address should have been.

**Where.** `posix/src/socket.rs`, `struct Addrinfo`.

**Not a glibc/musl divergence** -- both put `ai_addr` before `ai_canonname`.
Ours was simply wrong.

**Effect.** The canonical loop is

```c
for (p = res; p; p = p->ai_next)
    if (connect(fd, p->ai_addr, p->ai_addrlen) == 0) break;
```

`p->ai_addr` read the canonical-hostname string pointer, so `connect()` was
handed `ai_addrlen` bytes of a NUL-terminated name and read the first two as a
`sa_family_t`. Every C network client that resolves a name was affected.

**Fixed** by putting the fields in the C order. Found by
`scripts/check-libc-abi.py` (design-decisions 1011) on the day that gate was
written.
