## B-POSIX-LOCALHOST-IS-RESOLVED-BY-ASKING-A-DNS-SERVER (lane B, 2026-09-14) — OPEN, fix is lane A's

**Status:** FIXED 2026-09-27 -- the kernel's resolver consults its hosts table (`requests/a-b-dns-resolve-now-consults-the-hosts-table.md`), and the C library reads `/etc/hosts` before it asks the kernel (lane D, `D-POSIX-HOSTS-FILE-WAS-NEVER-READ`).

`getaddrinfo("localhost", …)` and `gethostbyname("localhost")` send a DNS query
over the network. There is no hosts-file lookup anywhere in the path, so on a
machine with no DNS server — or one whose upstream declines to answer for
`localhost`, which is the normal configuration — **`localhost` does not
resolve.**

**The call chain, which is the whole evidence:**

| step | where | what it does |
|---|---|---|
| `getaddrinfo` | `posix/src/socket.rs` | `inet_pton` first; not numeric ⇒ `gethostbyname` |
| `gethostbyname` | `posix/src/socket.rs:3892` | `syscall3(SYS_DNS_RESOLVE, …)` |
| `sys_dns_resolve` | `kernel/src/syscall/handlers.rs:13793` | calls `crate::net::dns::resolve` |
| `net::dns::resolve` | `kernel/src/net/dns.rs:1035` | container DNS, else `resolve_single` ⇒ **the wire** |

`kernel/src/fs/nameservice.rs` *does* hold a hosts table with `localhost` and
`ip6-localhost` in it. **Nothing in that chain reaches it.** It is a second,
unused resolver; the syscall goes to `net::dns`, which contains no hosts table
and no loopback special case (its only `localhost` is inside a test at
`dns.rs:1775`). Checked for a shortcut on both sides and there is none:
`resolve_single` has no hosts/loopback check, and `posix` has no `localhost`
special case — the `loopback` hits there are `in6addr_loopback`, `IFF_LOOPBACK`
and the `AI_PASSIVE` default, none of them on this path.

**Why it matters beyond `localhost`.** Every name a normal system would answer
from `/etc/hosts` — build-machine aliases, a pinned service name, an offline
development host, the `127.0.1.1 <hostname>` line Debian writes — misses here
and goes to the network instead. Two consequences worth separating:

* **It fails** where the upstream has no answer, and the failure is a network
  timeout rather than a prompt "no such host", so it is slow as well as wrong.
* **It leaks.** Internal-only names get transmitted to whatever upstream
  resolver is configured. A hosts file is often used precisely to keep a name
  off the network.

**Status of the evidence.** This is a code-read finding, not an observed one: I
cannot boot the image from this lane. The chain above is four greps and I have
stated each hop, so it should be cheap to confirm or refute — and refuting it
is welcome, because *this entry's own predecessor claimed the opposite and was
wrong.* See the correction in
`B-POSIX-HOSTNAME-INVENTS-AN-FQDN` above for how that happened.

**The fix is lane A's** (`kernel/**`), and is filed as
`requests/b-a-sys-dns-resolve-never-consults-the-hosts-table.md`. Either
`net::dns::resolve` consults `fs::nameservice` before going to the wire, or
`sys_dns_resolve` does so before calling it. The first is better — it fixes
in-kernel callers too — but that is lane A's call, not mine.

**Not fixable in libc**, which is why this is a request rather than work:
reading `/etc/hosts` from `posix` would need the file to exist in the image and
would still leave in-kernel resolution wrong, and it would duplicate a table
the kernel already has.
