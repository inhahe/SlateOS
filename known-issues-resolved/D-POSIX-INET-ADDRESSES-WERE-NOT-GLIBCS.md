### [D] D-POSIX-INET-ADDRESSES-WERE-NOT-GLIBCS — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/inet.rs` (was `posix/src/socket.rs`).

**What it was.** The address-conversion functions were this library's own,
and disagreed with glibc at the edges programs meet:

| Call | Was | glibc (now) |
|---|---|---|
| `inet_aton("127.1")`, `inet_addr("0x7f000001")`, `"0177.0.0.1"` | refused: dotted quads only | 127.0.0.1: the BSD forms, a C number per part |
| `inet_aton("10.0.0.1 junk")` | refused | accepted: white space ends the address |
| `inet_aton(s, NULL)` | 0 | 1 for a valid address: the text is only checked |
| `inet_pton(AF_INET, "01.2.3.4")` | accepted | refused: no leading zeros |
| `inet_ntop` of `::ffff:1.2.3.4` | `::ffff:102:304` | `::ffff:1.2.3.4`, and `::1.2.3.4` for the compatible form |
| `inet_network`, `inet_makeaddr`, `inet_lnaof`, `inet_netof` | missing | glibc's, wrapping included |
| `ether_aton`, `ether_ntoa` and their `_r` forms, `ether_line` | missing | glibc's; `ether_ntoa` writes `0:11:...`, not `00:11:...` |

**Fix.** glibc 2.40's `inet_aton_end`, `inet_pton4`/`inet_pton6`,
`inet_ntop4`/`inet_ntop6`, `inet_network`, the classful helpers and the
`ether_*` functions, ported. The tests replay glibc 2.39's answers for 186
inputs (`posix/tools/oracle/addr_oracle.c`, run under WSL) and compare every one.
`ether_aton`'s and `inet_ntoa`'s buffers are the calling thread's.
