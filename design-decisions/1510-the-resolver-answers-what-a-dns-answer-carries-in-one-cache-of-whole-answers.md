## 1510. The resolver answers what a DNS answer carries, in one cache of whole answers

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** asking the system for a name's address got one IPv4 address.
If a name had several addresses, or IPv6 ones, or was an alias for another
name, the rest was thrown away. "This name does not exist" and "this name
has no address of that kind" were the same answer. A program now gets
every address, the name at the end of any aliases, and the true reason
when there is no address. The C library needs these to answer as glibc
does (`getaddrinfo`, `gethostbyname`, `hostname -f`).

**What changed:**
- **`net::dns::lookup(name, family)`** gives every A and/or AAAA record and
  the canonical name. It follows CNAMEs within a response and across
  queries.
- **The errors come apart:** NXDOMAIN is `NotFound`; NODATA is
  `NoAddress`, a new `KernelError` (-707, `ENODATA`). SERVFAIL is
  `WouldBlock` (try again), REFUSED is `ConnectionRefused`, and an
  unreadable answer is `IoError`.
- **One cache of whole answers**, per name and record type, replaces the A
  cache and the AAAA cache. Each kept one address per name, with 0.0.0.0
  as "not found". A negative answer is cached as itself; a timeout is not
  cached.
- **The kernel's hosts table** (`fs::nameservice`) is asked before the
  network by every caller, not by `SYS_DNS_RESOLVE` alone.
- **`SYS_DNS_RESOLVE2` (1097)** carries it all to userspace. `resolve` and
  `resolve6` are the first address of a lookup.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. New call; the kernel resolver gives whole answers (chosen)** | getaddrinfo answers as glibc's | the cache, DHCP's servers and containers' names stay in one place, and the C library gets them by one call | a second DNS syscall beside 820 |
| B. The C library does DNS itself, over UDP sockets, as glibc does | the kernel resolver serves only the kernel | no kernel change | a second resolver and cache; the C library would have to learn DHCP's servers and each container's names |
| C. Widen 820 in place | one call | | changes a call existing binaries use |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| NODATA is a new error code, `NoAddress` | reuse `NoAttribute` (an object exists, an attribute does not) | the DNS case is its own; `getaddrinfo` maps it to `EAI_NODATA`, so it should not need guessing from a filesystem code |
| Addresses in answer order, IPv6 first for `AF_UNSPEC`; no sorting | sort by RFC 6724 in the kernel | the sort needs the source addresses a connection would use, and glibc sorts in userspace; the C library has the rest of `getaddrinfo` |
| `resolve`/`resolve6` keep answering `NotFound` for NODATA | return `NoAddress` | their kernel callers ask for one address and print a failure; nothing there would tell the two apart |
| A truncated UDP answer (TC) is used for what it holds | retry over TCP | there is no TCP client in the resolver yet; recorded as open (known-issues `A-DNS-ANSWERED-ONE-ADDRESS-AND-ONE-ERROR`) |

**Revisit** when the resolver gains TCP, or EDNS0, which would let one UDP
answer carry more than 512 bytes.
