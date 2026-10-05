## TD-B-PROC-NET-IF-INET-HAS-TWO-READERS-AND-NO-WRITER (lane B, 2026-09-16) — **open**

**In short:** two programs read `/proc/net/if_inet` to find this machine's IP
addresses. Nothing in the tree creates that file, and on SlateOS it cannot
exist: `/proc/net` is a **file**, not a directory — `procfs.rs`'s `ROOT_FILES`
lists `net` and `gen_net()` writes a readable block — so nothing can live
beneath it.

    readers:  userspace/coreutils/src/bin/hostname.rs  (until 2026-10-01)
              userspace/ifconfig/src/main.rs
    writers:  none

**`hostname` is no longer a reader (2026-10-01).** It is a port of Debian's
`hostname` now, and asks the C library: `-I` and `-A` walk `getifaddrs`, `-i`
takes the addresses `getaddrinfo` gives for the host name. Which also settles
the `-i` versus `-I` point at the end of this entry. `ifconfig` is the one
reader left.

This is the inverse of §946's publisher-with-no-subscriber: a **subscriber
with no publisher**, and it is invisible to the compiler because the
dependency is a path in a string.

### The half that was a wrong answer, fixed 2026-09-16

`hostname -i` and `-I` fell through from the missing file to a scan of
`/sys/class/net/<if>/address` — which is the **link-layer** address. So:

    $ hostname -I
    bc:a8:a6:f8:91:20 00:ff:5f:e2:d5:fd ...        exit 0

A caller asking for an IP address got MAC addresses, confidently and with a
success status. The old code even filtered `00:00:00:00:00:00`, a MAC-shaped
sentinel, so what it was reading was never in doubt.

Both now read `/proc/net`'s `IPv4:` line, which is the real source on this
system, and the MAC fallback is **deleted** rather than repaired: printing
nothing is the right answer when no address source is readable, because a
caller who gets nothing can tell and a caller who gets a MAC will put it in a
URL. With no source, `-I` now says `hostname: no addresses found` and exits 1.

The parser checks the `IPv4:` KEY rather than "the value after a colon",
because the `MAC:` line sits two lines above it in the same block — a
shape-matching parser would reintroduce exactly the bug it replaces. There is
a test for that specific confusion.

### What is still open

**`userspace/ifconfig` has the same fault and is not fixed here.** It reads
`/proc/net/if_inet` at `main.rs:429` for IP, netmask and broadcast, so on
SlateOS it reports none of them. It is a different program with a different
output format and belongs in its own change.

**IPv6 is not covered.** `gen_net()` emits one `IPv4:` line and no v6, so
`hostname -I` can never list a v6 address on SlateOS however it is parsed.
`/proc/net` growing v6 is lane A's.

**`-i` is not the same question as `-I`.** Measured: GNU's `-i` answers
`127.0.1.1` here — the address the HOST NAME resolves to, out of `/etc/hosts`
— while `-I` lists every interface address. Ours treats them as the same
query. Now that `canonical_in_hosts` exists, `-i` could take the address field
from the same line it already finds the FQDN on, which would match GNU exactly
on any host whose name is in the hosts table.
