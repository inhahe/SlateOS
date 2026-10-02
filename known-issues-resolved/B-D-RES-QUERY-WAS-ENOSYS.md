### [D] B-D-RES-QUERY-WAS-ENOSYS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/resolv.rs`.

**What it was.** `res_query`, `res_search`, `res_mkquery` and `res_send`
checked their arguments and returned `ENOSYS`: a program looking up anything
but an address -- a mail exchanger, a service record, a text record -- got
nothing. `getaddrinfo` works only because the kernel's network stack
answers address lookups itself (`SYS_DNS_RESOLVE`). Beside them, `dn_comp`
wrote no compression pointers and ignored its table, `dn_expand` did not
escape special characters and refused a compression pointer that pointed
forward, and neither set `errno`.

**Fix.** A resolver: `resolv.conf` read into `_res` (`__res_state()`) as
glibc reads it; names converted by glibc's `ns_name_*` rules, `dn_comp`
compressing against its table; `res_mkquery` building glibc's query;
`res_send` asking every nameserver over UDP, again at intervals, waiting past
`SERVFAIL`/`NOTIMP`/`REFUSED` as glibc moves past them, and asking again over
TCP when the answer is truncated -- musl's transport; `res_query` and
`res_search` reporting through `h_errno` as glibc does. The transport is
tested against a scripted network; everything else is pure and tested on the
host.

**What remains.** IPv6 nameservers are read and skipped; `sortlist`, EDNS0,
`HOSTALIASES` and DNSSEC are not done. Nothing has queried a live server yet:
that needs a ring-3 fixture against the boot's network.
