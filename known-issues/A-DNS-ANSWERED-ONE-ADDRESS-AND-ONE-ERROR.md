### A-DNS-ANSWERED-ONE-ADDRESS-AND-ONE-ERROR -- 2026-10-01 -- FIXED the same day (lane A); TCP for a truncated answer OPEN

**In short:** asking for a name's address got one IPv4 address. The rest
of the answer was thrown away: a name's other addresses, its IPv6
addresses, and the name at the end of its aliases (CNAMEs). "No such name"
and "no address of that kind" were one error. So `getaddrinfo` could not
answer as glibc does (lane D's request), and `hostname -f` could not learn
a machine's full name (lane B's).

**Where:** `kernel/src/net/dns.rs`, `resolve` and `resolve6`; each had a
one-address cache. `SYS_DNS_RESOLVE` (820) wrote four bytes.

**Fixed** (design-decisions §1510):
- `net::dns::lookup` gives every address and the canonical name.
- NXDOMAIN is `NotFound`; NODATA is the new `NoAddress` (-707);
  SERVFAIL is `WouldBlock`; REFUSED is `ConnectionRefused`.
- One cache keeps whole answers, negative ones as themselves.
- The kernel's hosts table is asked first, by every caller.
- `SYS_DNS_RESOLVE2` (1097) carries it to userspace.
- Tests:
  - `net::dns::self_test`: parsing, the CNAME chain, each refusal, the
    cache, the `AF_UNSPEC` merge, the hosts table;
  - `dispatch`'s `test_dispatch_dns_resolve2`: the layout, through
    `localhost`.

**Still open:** a UDP answer longer than 512 bytes comes truncated (the
TC bit), and the resolver uses what it holds. A name with many records
loses the ones past the cut. glibc retries over TCP; the resolver has no
TCP client. The fix is that retry, or EDNS0 to raise the UDP size.
