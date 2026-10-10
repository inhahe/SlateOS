/*
 * SlateOS: <sys/profil.h>, which musl does not have -- glibc's sprofil, the
 * program-counter profiler for several regions at once. It cannot start
 * here, as profil cannot: there is no profiling timer (posix/src/legacy.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_SYS_PROFIL_H
#define _SLATEOS_SYS_PROFIL_H

#include <sys/time.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

struct prof {
    void *pr_base;               /* buffer base */
    size_t pr_size;              /* buffer size */
    size_t pr_off;               /* pc offset */
    unsigned long int pr_scale;  /* pc scaling (fixed-point number) */
};

enum {
    PROF_USHORT = 0,     /* use 16-bit counters (default) */
    PROF_UINT = 1 << 0,  /* use 32-bit counters */
    PROF_FAST = 1 << 1   /* profile faster than usual */
};

int sprofil(struct prof *__profp, int __profcnt, struct timeval *__tvp, unsigned int __flags);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_PROFIL_H */
