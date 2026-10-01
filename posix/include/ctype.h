/*
 * SlateOS: what this C library's <ctype.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <ctype.h>

#ifndef _SLATEOS_CTYPE_H
#define _SLATEOS_CTYPE_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* glibc's character class bits, which isctype's mask is made of. */
enum {
	_ISupper = 0x100,
	_ISlower = 0x200,
	_ISalpha = 0x400,
	_ISdigit = 0x800,
	_ISxdigit = 0x1000,
	_ISspace = 0x2000,
	_ISprint = 0x4000,
	_ISgraph = 0x8000,
	_ISblank = 0x1,
	_IScntrl = 0x2,
	_ISpunct = 0x4,
	_ISalnum = 0x8
};

#ifdef _GNU_SOURCE
/* A character's classes that are in the mask. */
int isctype(int, int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_CTYPE_H */
