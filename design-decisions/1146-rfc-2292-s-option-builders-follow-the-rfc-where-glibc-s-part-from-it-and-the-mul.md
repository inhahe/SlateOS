## 1146. RFC 2292's option builders follow the RFC where glibc's part from it, and the multicast source filters are refused

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** two groups of `<netinet/in.h>` calls needed a choice. The
older IPv6 option builders (RFC 2292, 1998) build a header of options for
a packet; glibc's builds a valid header but pads it more than the RFC's
own examples do, reserves too little room for one call's option, and
reports the end of a header as an error. This library does what the RFC
says. The multicast source filters ("only accept this group's packets from
these senders") have nothing under them here; this library says so rather
than accept the filter and apply nothing.

| Call | Here | glibc 2.39 | Why |
|---|---|---|---|
| `inet6_option_append`, `inet6_option_alloc` | the least padding that puts an option on `xn + y`; the header's tail padding moved to the new end | rounds up to a multiple of `x`, then adds `y`; keeps each tail padding | RFC 2292 section 6.3.7's example puts an `8n + 2` option straight after the two header bytes, which glibc's would pad 8 more; both are valid headers |
| `inet6_option_alloc(cmsg, datalen, ...)` | room for `datalen` data bytes and the type and length bytes | `datalen` bytes | the RFC: `datalen` "is the value of the option data length byte"; a caller following it overruns glibc's reservation |
| `inet6_option_next` at the end | -1, `*tptrp` NULL | -1, `*tptrp` past the last option | the RFC: NULL means no more; not NULL means an error |
| `getsourcefilter` and its three kin | -1, `ENOPROTOOPT`, after the descriptor and address checks | the kernel's answer | the sockets keep no source filters, and `setsockopt` accepts options it does not know without acting on them; §1144's rule |

RFC 3542's builders (`inet6_opt_*`, `inet6_rth_*`) are glibc's exactly;
there the two agree.

**Where:** `posix/src/inet6.rs`.
