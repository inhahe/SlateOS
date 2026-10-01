/*
 * SlateOS: <sgtty.h>, which musl does not have -- Version 7's terminal
 * calls, which glibc keeps and refuses (ENOSYS), as this library does;
 * tcgetattr and tcsetattr are the calls that work. struct sgttyb is
 * declared and never defined, as in glibc's.
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_SGTTY_H
#define _SLATEOS_SGTTY_H

#include <sys/ioctl.h>

#ifdef __cplusplus
extern "C" {
#endif

struct sgttyb;

int gtty(int, struct sgttyb *);
int stty(int, const struct sgttyb *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SGTTY_H */
