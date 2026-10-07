## D-POSIX-GETIFADDRS-SHARED-ONE-STATIC-LIST — every `getifaddrs` caller got the same list, rewritten by the next call (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** asking the C library for the machine's network addresses
returned a list kept in one fixed place in memory. A second request --
another thread's, or a library's -- rewrote that list while the first
caller might still be reading it, and freeing the list did nothing. The
interface numbers were also muddled: the loopback and the network card
were both "interface 1".

| Call | Was | Now |
|---|---|---|
| `getifaddrs` | static list, `eth0` then `lo`; `eth0` flagged `IFF_BROADCAST` with a NULL broadcast address | one allocation per call; `lo` then `eth0` (Linux's order); `eth0`'s broadcast address set |
| `freeifaddrs` | nothing | `free` of that allocation, as glibc |
| `if_nameindex` / `if_freenameindex` | static array holding `eth0` alone; free did nothing | one allocation per call, `lo` 1 and `eth0` 2; freed |
| `if_nametoindex` | "lo" 1, "eth0" 1 | 1 and 2; 0 with `ENODEV` for any other name |
| `if_indextoname` | 1 "eth0" | 1 "lo", 2 "eth0"; NULL with `ENXIO` otherwise |

**How it was found:** reviewing `ifaddrs.rs`, a facade nothing reached, for
tests worth keeping (`todo.txt`, the islands item): its one test was the only
test `getifaddrs` had, and reading the function behind it showed the static
storage. Found by reading, not by a failure -- no program in the tree calls it
from two threads.

**Since closed:** the `AF_PACKET` entries it lacked, with three more
differences from glibc that an oracle found --
`D-POSIX-GETIFADDRS-WAS-NOT-GLIBCS`.
