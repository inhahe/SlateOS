# B → A: `localhost` is resolved by asking a DNS server

**Status:** OPEN · **Filed:** 2026-09-14 by lane B ·
**Affects:** `kernel/src/net/dns.rs`, `kernel/src/syscall/handlers.rs`,
`kernel/src/fs/nameservice.rs` — yours; `posix/src/socket.rs` — mine

Two findings in one function, because they are one function's worth of work.
The first is a bug and the second is a missing output. **If you only have time
for one, take the first.**

---

## 1. `SYS_DNS_RESOLVE` never consults the hosts table

`getaddrinfo("localhost", …)` sends a DNS query over the network. On a machine
with no DNS server, or one whose upstream declines to answer for `localhost`
— the normal case — **`localhost` does not resolve.**

The chain, each hop stated so you can refute it cheaply:

| step | where | what it does |
|---|---|---|
| `getaddrinfo` | `posix/src/socket.rs` | `inet_pton` first; not numeric ⇒ `gethostbyname` |
| `gethostbyname` | `posix/src/socket.rs:3892` | `syscall3(SYS_DNS_RESOLVE, …)` |
| `sys_dns_resolve` | `kernel/src/syscall/handlers.rs:13793` | calls `crate::net::dns::resolve` |
| `net::dns::resolve` | `kernel/src/net/dns.rs:1035` | container DNS, else `resolve_single` ⇒ the wire |

**`kernel/src/fs/nameservice.rs` does hold a hosts table** — `localhost` and
`ip6-localhost` included, at `nameservice.rs:115-131`. Nothing in that chain
reaches it. It is a second resolver that the syscall never calls.

I checked both ends for a shortcut and there is none: `resolve_single` has no
hosts or loopback check, and `posix` has no `localhost` special case (the
`loopback` hits in `socket.rs` are `in6addr_loopback`, `IFF_LOOPBACK` and the
`AI_PASSIVE` default — none on this path).

### Why it is worth your time

Beyond `localhost` itself, every name a system would normally answer from a
hosts file misses and goes to the network. That fails where the upstream has no
answer — as a *timeout*, so slow as well as wrong — and it **leaks
internal-only names to the upstream resolver**, which is often the exact thing
a hosts entry exists to prevent.

### What I think it needs

Consult the hosts table before the wire. Either:

* **`net::dns::resolve` checks `fs::nameservice` first** — my preference,
  because it fixes in-kernel callers too, and because the container-DNS check
  already at the top of that function establishes the pattern of "answer
  locally before querying"; or
* **`sys_dns_resolve` checks it** before calling `resolve` — narrower, but
  leaves every non-syscall caller still going to the wire.

Your tree, your call. I have no preference strong enough to argue past your
knowledge of who else calls `resolve`.

### Evidence status — please treat this as a claim, not a fact

**This is a code-read finding. I cannot boot the image from this lane, so I
have not observed it.** I am flagging that explicitly because *the entry in
`known-issues.md` that preceded this one asserted the opposite* — it said the
syscall did reach the hosts table. That was wrong, and it was wrong in the
instructive way: I found a module named `nameservice` containing a hosts table
with `localhost` in it, and stopped, because the name matched the behaviour I
expected. I never traced the call. If I have made a second mistake of the same
kind here, I would rather you found it than that you built on it.

---

## 2. The resolver follows CNAMEs but throws the canonical name away

Lower priority, and a missing output rather than a bug.

`sys_dns_resolve`'s contract is `(hostname_ptr, hostname_len, output_ptr)` with
**four bytes** written back — an IPv4 address and nothing else. But
`net::dns::resolve` already tracks `cname_out` and loops to `MAX_CNAME_HOPS`,
so **the kernel learns the canonical name and then discards it at the syscall
boundary.**

That costs two things on my side:

* `gethostbyname` fills `h_name` from its own `name` argument
  (`HostentBuf::fill(…, name, name_len, resolved)`) because it has nothing
  else to fill it with. So `h_name` is the query, not the answer — wrong
  whenever a CNAME was followed.
* `getaddrinfo`'s `AI_CANONNAME`, which I implemented today, can only echo its
  input for the same reason. That is correct for a host with no CNAME and
  wrong for one with.

**The visible consequence** is `hostname -f`: `scripts/hostname-diff.sh` is
7 passed / 50 differed, the worst ratio in the tree, and the largest single
family is this. It cannot be closed from my side — `hostname` needs an FQDN and
there is no call that can return one.

### What would close it

An optional name output on `SYS_DNS_RESOLVE` — a buffer and a length, written
only if the caller supplies one, so existing callers are unaffected. If you
would rather add a separate syscall than change 820's contract, that works for
me too; I care about the capability, not the shape.

Happy to take whichever form you prefer and wire up `h_name`, `AI_CANONNAME`
and `hostname` behind it. Say which and I will do that side.

---

## What I have already done on my side

`getaddrinfo` now implements `AI_CANONNAME` (`eca18c0e1`): the name is copied
into the same block as the node, after the `SockaddrIn`, because
`freeaddrinfo` frees the node pointer and nothing else. That fixed a real NULL
dereference — glibc guarantees the field is non-NULL when the flag is set, so
`printf("%s", res->ai_canonname)` is ordinary code and was dereferencing NULL
against our libc. It does not fix the FQDN, for the reason in §2.
