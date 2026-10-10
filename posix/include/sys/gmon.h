/*
 * SlateOS: <sys/gmon.h>, which musl does not have -- monstartup, which
 * starts the profiling a program built with -pg keeps for gprof. It cannot
 * start here: it counts through profil, which has no profiling timer, so
 * monstartup does nothing (posix/src/legacy.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_SYS_GMON_H
#define _SLATEOS_SYS_GMON_H

#ifdef __cplusplus
extern "C" {
#endif

void monstartup(unsigned long __lowpc, unsigned long __highpc);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_GMON_H */
