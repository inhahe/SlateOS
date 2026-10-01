/*
 * SlateOS: what this C library's <netinet/in.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <netinet/in.h>

#ifndef _SLATEOS_NETINET_IN_H
#define _SLATEOS_NETINET_IN_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Bind to a free privileged port, 512 to 1023. */
int bindresvport(int, struct sockaddr_in *);
#endif

#ifdef _GNU_SOURCE
struct cmsghdr;

/* RFC 2292's Hop-by-Hop and Destination options, over ancillary data. */
_SLATEOS_DEPRECATED("use inet6_opt_*") int inet6_option_space(int);
_SLATEOS_DEPRECATED("use inet6_opt_*") int inet6_option_init(void *, struct cmsghdr **, int);
_SLATEOS_DEPRECATED("use inet6_opt_*") int inet6_option_append(struct cmsghdr *, const uint8_t *, int, int);
_SLATEOS_DEPRECATED("use inet6_opt_*") uint8_t *inet6_option_alloc(struct cmsghdr *, int, int, int);
_SLATEOS_DEPRECATED("use inet6_opt_*") int inet6_option_next(const struct cmsghdr *, uint8_t **);
_SLATEOS_DEPRECATED("use inet6_opt_*") int inet6_option_find(const struct cmsghdr *, uint8_t **, int);

/* RFC 3542's: the options of a Hop-by-Hop or Destination Options header. */
int inet6_opt_init(void *, socklen_t);
int inet6_opt_append(void *, socklen_t, int, uint8_t, socklen_t, uint8_t, void **);
int inet6_opt_finish(void *, socklen_t, int);
int inet6_opt_set_val(void *, int, void *, socklen_t);
int inet6_opt_next(void *, socklen_t, int, uint8_t *, socklen_t *, void **);
int inet6_opt_find(void *, socklen_t, int, uint8_t, socklen_t *, void **);
int inet6_opt_get_val(void *, int, void *, socklen_t);

/* RFC 3542's: a Type 0 Routing header. */
socklen_t inet6_rth_space(int, int);
void *inet6_rth_init(void *, socklen_t, int, int);
int inet6_rth_add(void *, const struct in6_addr *);
int inet6_rth_reverse(const void *, void *);
int inet6_rth_segments(const void *);
struct in6_addr *inet6_rth_getaddr(const void *, int);

/* RFC 3678's multicast source filters: refused here (ENOPROTOOPT), the
 * sockets keeping none. */
int getipv4sourcefilter(int, struct in_addr, struct in_addr, uint32_t *, uint32_t *,
                        struct in_addr *);
int setipv4sourcefilter(int, struct in_addr, struct in_addr, uint32_t, uint32_t,
                        const struct in_addr *);
int getsourcefilter(int, uint32_t, const struct sockaddr *, socklen_t, uint32_t *, uint32_t *,
                    struct sockaddr_storage *);
int setsourcefilter(int, uint32_t, const struct sockaddr *, socklen_t, uint32_t, uint32_t,
                    const struct sockaddr_storage *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_NETINET_IN_H */
