/*
 * SlateOS: what this C library's <sys/timex.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists -- and
 * glibc's struct ntptimeval in place of musl's.
 *
 * musl's struct ntptimeval is the struct as it was before the TAI offset:
 * time, maxerror, esterror. glibc's adds the offset and four reserved words,
 * and ntp_gettimex fills them all -- the function this header sends
 * ntp_gettime to, as glibc's does. A struct is defined once, so musl's
 * header is made to define its own under a name nothing uses (musl declares
 * no function that takes one), and glibc's is defined here.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#define ntptimeval __slateos_musl_ntptimeval
#include_next <sys/timex.h>
#undef ntptimeval

#ifndef _SLATEOS_SYS_TIMEX_H
#define _SLATEOS_SYS_TIMEX_H


#ifdef __cplusplus
extern "C" {
#endif

/* The clock's time and error bounds, as ntp_gettimex reads them. */
struct ntptimeval {
	struct timeval time; /* the time; tv_usec is nanoseconds under STA_NANO */
	long maxerror;       /* the maximum error, in microseconds */
	long esterror;       /* the estimated error, in microseconds */
	long tai;            /* the TAI offset, in seconds */
	long __glibc_reserved1;
	long __glibc_reserved2;
	long __glibc_reserved3;
	long __glibc_reserved4;
};

/* adjtimex by the NTP interface's name. */
int ntp_adjtime(struct timex *);

/* The time, its error bounds and the TAI offset: the clock's state (TIME_OK
 * ... TIME_ERROR), or -1. ntp_gettime is ntp_gettimex, as glibc's header
 * makes it: the library's own ntp_gettime fills only the struct as it was
 * before the TAI offset, for what calls it by name. */
int ntp_gettimex(struct ntptimeval *);
int ntp_gettime(struct ntptimeval *) __asm__("ntp_gettimex");

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_TIMEX_H */
