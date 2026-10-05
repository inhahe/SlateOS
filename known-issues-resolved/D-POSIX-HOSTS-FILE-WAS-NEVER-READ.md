### [D] D-POSIX-HOSTS-FILE-WAS-NEVER-READ — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/hosts.rs` (was `posix/src/socket.rs`).

**What it was.** Every host lookup went straight to the kernel's resolver:
the C library never read `/etc/hosts` or `/etc/host.conf`, so a name a
program's own container, chroot or administrator had written there was not
found. Beside that:

- `gethostbyname("1.2.3.4")` asked the resolver instead of answering the
  number; `gethostbyname2(..., AF_INET6)` answered "no data" for everything,
  `::1` included;
- `gethostbyname2_r`, `gethostent`, `gethostent_r`, `sethostent` and
  `endhostent` were missing;
- the `_r` functions returned musl's codes (`ENOENT` for "not found"); glibc
  returns 0 with a NULL result;
- `herror` wrote to the kernel console, not standard error, and
  `hstrerror`'s messages were not glibc's ("Host not found" for "Unknown
  host").

**Fix.** The hosts database as glibc 2.40 answers it with `hosts: files dns`,
the kernel's resolver standing in for DNS (design-decisions.md §1128):
numbers answered as themselves, then `/etc/hosts` (`multi`, `reorder` and
`trim` from `host.conf`), then the kernel -- whose failures are reported as
glibc's DNS module reports them. The tests replay glibc 2.39's answers to 66
lookups under two `host.conf` files, with the network down
(`posix/tools/oracle/hosts_oracle.c`), and its `host.conf` warnings byte for byte.

**What remains.** The kernel's resolver answers one IPv4 address and no
canonical name: see `requests/d-a-sys-dns-resolve-answers-one-ipv4-address.md`.
