## 1128. Host lookups are glibc's `files dns`, with the kernel's resolver as the DNS

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a program asking for a host's address gets, in order: the
number itself if the name is one, `/etc/hosts`, then the kernel's resolver
-- the order every Linux distribution's `nsswitch.conf` gives. The kernel is
this system's DNS: it holds the cache, its own hosts table, the servers DHCP
gave and each container's names, so the C library asks it rather than
speaking DNS itself, the way glibc on a desktop asks `systemd-resolved`.

**Why not this library's own DNS client** (`res_query`, which exists). It
would bypass everything the kernel's resolver knows -- a container's peers
by name, the cache -- and has no servers to ask on a booted system with no
`/etc/resolv.conf`. It would buy `AAAA` records and several addresses a
name, which the kernel does not answer yet; that belongs in the kernel
(`requests/d-a-sys-dns-resolve-answers-one-ipv4-address.md`).

**What the kernel cannot say, and what is said instead:**

| Question | Answer |
|---|---|
| an IPv6 (`AAAA`) address | the IPv4 address is asked for: found means "no address of that kind" (`NO_DATA`), not found means "no such host" |
| every address of a name | the one the kernel gives |
| the canonical name | the name asked |
| a failure | glibc's DNS module's words for it: unreachable is "try again", refused or timed out is "unavailable, try again", not found is `HOST_NOT_FOUND` -- and a name that is not a host name (`res_hnok`) is never asked |
