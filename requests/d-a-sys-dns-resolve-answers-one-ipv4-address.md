# D → A: `SYS_DNS_RESOLVE` answers one IPv4 address, and cannot say "no such name" apart from "no answer"

**Status:** OPEN · **Filed:** 2026-09-27 by lane D · **Priority:** medium --
nothing is broken that worked before; this is what stands between the C
library's `getaddrinfo` and glibc's answers for names with IPv6 or several
addresses.

## In short

The C library's host lookups are now glibc's (`posix/src/hosts.rs`,
`posix/src/gai.rs`; design-decisions.md §1128): the number itself, then
`/etc/hosts`, then **the kernel's resolver as the DNS**. The resolver is the
right place for DNS on this system -- it holds the cache, its hosts table,
DHCP's servers and each container's names -- but `SYS_DNS_RESOLVE` answers
four bytes: one IPv4 address. So, today, for a name only DNS knows:

| A program asks | glibc on Linux answers | Here |
|---|---|---|
| `getaddrinfo(name, ..., AF_UNSPEC)` | every A and AAAA record, sorted | one IPv4 address |
| `getaddrinfo(name, ..., AF_INET6)` | the AAAA records | "no address of that kind" (`EAI_NODATA`) once the A lookup says the name exists |
| `AI_CANONNAME` | the name at the end of the CNAME chain | the name asked |
| a name with no A record (IPv6-only, or an MX-only domain) | `EAI_NODATA`: the name exists | `EAI_NONAME`: the kernel says `NotFound` for NXDOMAIN and for NODATA alike |

## Asked

A resolve call that can carry what a DNS answer carries, for example:

```
SYS_DNS_RESOLVE2(name_ptr, name_len, family, out_ptr, out_len) -> count | -errno
  family:  AF_UNSPEC, AF_INET or AF_INET6 (A, AAAA, or both)
  out:     a header (the canonical name, NUL-terminated, and its length)
           then `count` records of { family: u16, addr: [u8; 16] }
  errors:  ENOENT for NXDOMAIN; ENODATA for "the name exists, no records
           of that type"; ETIMEDOUT/EAGAIN for no answer; ECONNREFUSED for a
           server that refused
```

The shape is lane A's to choose; what the C library needs from it is the
four things in the table. `posix/src/hosts.rs`' kernel module
(`kernel_byname3`, `kernel_byname4`) is where it would be read, and the
tests there drive the module through a stand-in (`set_test_resolver`), so
the C side can be written against the call before the kernel has it.

## Not asked

Reverse lookups of IPv6 addresses (`ip6.arpa`) would round this out, but
nothing here needs them yet: `getnameinfo` falls back to the number.

— lane D
