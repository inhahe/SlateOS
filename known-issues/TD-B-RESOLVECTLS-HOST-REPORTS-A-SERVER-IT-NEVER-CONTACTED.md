## TD-B-RESOLVECTLS-HOST-REPORTS-A-SERVER-IT-NEVER-CONTACTED (lane B, 2026-09-11)

**In short:** `resolvectl`'s `host` personality prints which DNS server answered
and how long the query took. It contacts no DNS server and times nothing. The
server name is read out of `/etc/resolv.conf` and the query time is the literal
`0`, while the actual lookup goes through `to_socket_addrs()` — the system
resolver.

**Where.** `userspace/resolvectl/src/main.rs`, `cmd_host`, verbose path:

    ;; Query time: 0 msec
    ;; SERVER: <first server in resolv.conf>#53

and `resolve_hostname`, which is `to_socket_addrs()` with a hardcoded
`localhost` fallback. `reverse_lookup` beside it is a `match` on a handful of
literal addresses under a comment reading "Simplified reverse lookup".

**Why it is not urgent, and why it is still here.** `host` is *unreachable*:
nothing in the tree produces an executable by that name, so no user can run it
today. It sits in the unreachable ledger
(`TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN`), and the
resolution for that ledger is to **give the name a producer** — which would
ship this defect to users the day it happens. So the entry exists to make sure
that whoever gives `host` a producer fixes the body first, rather than
discovering afterwards that the tool they just published invents its evidence.

**The proper fix** is the one `userspace/nslookup` already implements: build a
real DNS query per RFC 1035, send it to the chosen server over UDP, and report
the server and elapsed time that actually applied. Failing that, do not print a
`SERVER:` line at all — an omitted field is honest and a wrong one is not.

**Found** while removing `resolvectl`'s `nslookup` personality (2026-09-11),
which had the identical defect and *was* shadowing a real implementation.
