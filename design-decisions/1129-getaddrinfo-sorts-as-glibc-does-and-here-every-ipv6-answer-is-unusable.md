## 1129. `getaddrinfo` sorts as glibc does, and here every IPv6 answer is unusable

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** when a name has several addresses, glibc orders them by RFC
3484's rules so a program that tries them in order tries the best first. The
first rule is "avoid addresses you cannot reach", decided by connecting a
datagram socket. This system has no IPv6 sockets, so every IPv6 address sorts
after every IPv4 one -- which is what glibc does on a Linux machine with IPv6
switched off, and what a program here needs.

- **The rules are glibc's**, down to its tables, `/etc/gai.conf`, and its
  use of prefix lengths only on machines with IPv6 (which this is not).
- **An IPv4 answer mapped into an IPv6 question** (`AI_V4MAPPED`) is
  unusable here too, and sorts with the IPv6 ones; Linux's dual-stack
  sockets would reach it and put it first.
- **musl's `NI_NUMERICSCOPE`** (0x100) is accepted by `getnameinfo`, where
  glibc, which has no such flag, answers `EAI_BADFLAGS`: a program built
  against musl's header passes it meaning "a numeric scope" (§1119).
- **`AI_IDN` without `libidn2`**: an ASCII name is itself and another is
  `EAI_IDN_ENCODE`, as glibc answers when the library is absent.
